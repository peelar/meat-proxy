use rusqlite::{Connection, params};
use std::path::PathBuf;

use crate::github::SyncOptions;
use crate::judge::Judgment;
use crate::model::{
    CiStatus, InboxFilter, IncomingReason, NotificationEvent, NotificationKind, Ownership,
    PullRequest, RepoConfig, ReviewState, Tracked, TrackedState, WatchRule,
};

#[derive(Clone, Debug)]
pub struct AppSnapshot {
    pub prs: Vec<PullRequest>,
    pub repos: Vec<RepoConfig>,
    pub rules: Vec<WatchRule>,
    pub notifications: Vec<NotificationEvent>,
    pub draft_stale_hours: u64,
    pub open_stale_hours: u64,
    pub onboarded: bool,
    pub default_filter: InboxFilter,
    pub native_notifications: bool,
    pub sync_options: SyncOptions,
    pub group_order: Vec<u8>,
    pub collapsed_groups: Vec<u8>,
    pub last_notif_modified: Option<String>,
    pub organization: String,
    pub judgments: std::collections::HashMap<String, Judgment>,
    /// The user's choice of AI or rules for sorting, if they made one.
    pub smart_sort: Option<bool>,
    /// Hide outside contributors' pull requests from the Inbox tab.
    pub hide_external: bool,
    pub show_team_reviews: bool,
    pub ai_enabled: bool,
    /// Autonomous: keep my branches moving; ping my reviewers. Both off until chosen.
    pub autonomy_branches: bool,
    pub autonomy_reviewers: bool,
    /// Actions already taken, one per pull request, action and commit.
    pub autonomy_done: std::collections::HashSet<String>,
    /// Pace: keep score of quick approvals and merges (on unless switched off), whether its
    /// panel is folded, and what it has counted.
    pub pace_enabled: bool,
    pub pace_collapsed: bool,
    pub pace_events: Vec<crate::pace::Event>,
    /// Pull requests the user moved to Ignored.
    pub ignored: std::collections::HashSet<String>,
    /// Pull requests with auto-merge on, and what became of them.
    pub tracked: Vec<Tracked>,
    /// Which sections the saved group order was made for.
    pub layout_key: Option<String>,
}

pub fn default_db_path() -> PathBuf {
    if let Ok(custom) = std::env::var("MEAT_PROXY_DB") {
        return PathBuf::from(custom);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let dir = PathBuf::from(home).join(".config/meat-proxy");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("meat-proxy.sqlite3");
    }
    PathBuf::from("meat-proxy.sqlite3")
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_default() -> Result<Self, rusqlite::Error> {
        let path = default_db_path();
        Self::open(&path)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open(path: &PathBuf) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pull_requests (
                id TEXT PRIMARY KEY,
                repo TEXT NOT NULL,
                number INTEGER NOT NULL,
                title TEXT NOT NULL,
                url TEXT NOT NULL,
                author TEXT NOT NULL,
                ownership TEXT NOT NULL,
                is_draft INTEGER NOT NULL,
                review_state TEXT NOT NULL,
                ci_status TEXT NOT NULL,
                incoming_reason TEXT NOT NULL,
                reviewers_json TEXT NOT NULL,
                idle_hours INTEGER NOT NULL,
                unreviewed_hours INTEGER NOT NULL,
                head_ref TEXT NOT NULL,
                matched_rule TEXT,
                snoozed_until_hour INTEGER NOT NULL DEFAULT 0,
                additions INTEGER NOT NULL DEFAULT 0,
                deletions INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS repos (
                name TEXT PRIMARY KEY,
                manual_rank INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                my_pr_volume INTEGER NOT NULL DEFAULT 0,
                assigned_review_volume INTEGER NOT NULL DEFAULT 0,
                collapsed INTEGER NOT NULL DEFAULT 0,
                removed INTEGER NOT NULL DEFAULT 0,
                activity INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS rules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                query TEXT NOT NULL,
                exclude_drafts INTEGER NOT NULL DEFAULT 1,
                exclude_mine INTEGER NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 1
            );


            CREATE TABLE IF NOT EXISTS notifications (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                pr_url TEXT NOT NULL,
                kind TEXT NOT NULL,
                timestamp_label TEXT NOT NULL,
                unread INTEGER NOT NULL DEFAULT 1
            );
            ",
        )?;
        let migrated: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('pull_requests') WHERE name='stack_base')", [], |r| r.get(0))?;
        if !migrated {
            let tx = rusqlite::Transaction::new_unchecked(
                &self.conn,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            let already_done: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('pull_requests') WHERE name='stack_base')", [], |r| r.get(0))?;
            if !already_done {
                tx.execute_batch(
                    "ALTER TABLE pull_requests ADD COLUMN head_repo TEXT;
                                  ALTER TABLE pull_requests ADD COLUMN stack_base TEXT;",
                )?;
            }
            tx.commit()?;
        }
        let has_opened: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('pull_requests') WHERE name='opened_hours')",
            [],
            |r| r.get(0),
        )?;
        if !has_opened {
            self.conn.execute(
                "ALTER TABLE pull_requests ADD COLUMN opened_hours INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        let has_approvers: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('pull_requests') WHERE name='approved_by')",
            [],
            |r| r.get(0),
        )?;
        if !has_approvers {
            self.conn.execute(
                "ALTER TABLE pull_requests ADD COLUMN approved_by TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        let has_removed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('repos') WHERE name='removed')",
            [],
            |r| r.get(0),
        )?;
        if !has_removed {
            let tx = rusqlite::Transaction::new_unchecked(
                &self.conn,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            let already_done: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('repos') WHERE name='removed')",
                [],
                |r| r.get(0),
            )?;
            if !already_done {
                tx.execute_batch(
                    "ALTER TABLE repos ADD COLUMN removed INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE repos ADD COLUMN activity INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            tx.commit()?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS auto_merges (
                pr_id TEXT PRIMARY KEY,
                pr_json TEXT NOT NULL,
                section TEXT NOT NULL,
                state TEXT NOT NULL,
                changed_at INTEGER NOT NULL
            );",
        )?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pace_events (
                at INTEGER NOT NULL,
                kind TEXT NOT NULL,
                pr_id TEXT NOT NULL,
                wait_minutes INTEGER,
                lines INTEGER NOT NULL,
                verdict TEXT NOT NULL
            );",
        )?;
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ignored_prs (
                pr_id TEXT PRIMARY KEY,
                ignored_at INTEGER NOT NULL
            );",
        )?;
        // Judgments keep one answer per question, so questions can be added in the config. The
        // earlier fixed columns are dropped: the cache is cheap to fill again.
        let fixed_columns: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('judgments') WHERE name='trivial')",
            [],
            |r| r.get(0),
        )?;
        if fixed_columns {
            self.conn.execute("DROP TABLE judgments", [])?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS judgments (
                pr_id TEXT PRIMARY KEY,
                fingerprint TEXT NOT NULL,
                answers TEXT NOT NULL,
                judged_by TEXT NOT NULL,
                judged_at INTEGER NOT NULL
            );",
        )?;
        // Rules an early version seeded: one repeats the built-in searches, the other watched a
        // repository this app has no reason to know about.
        self.conn.execute(
            "DELETE FROM rules WHERE id IN ('rule-assigned', 'rule-team-urgent')",
            [],
        )?;
        Ok(())
    }

    pub fn load_snapshot(&self) -> Result<AppSnapshot, rusqlite::Error> {
        let draft_stale_hours = self
            .get_setting("draft_stale_hours")?
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(72); // 3 days default
        let open_stale_hours = self
            .get_setting("open_stale_hours")?
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(48); // 2 days default
        let onboarded = self
            .get_setting("onboarded")?
            .map(|v| v == "1")
            .unwrap_or(false);
        let default_filter = self
            .get_setting("default_tab")?
            .map_or(InboxFilter::Inbox, |name| InboxFilter::from_name(&name));
        let native_notifications =
            self.get_setting("native_notifications")?.as_deref() != Some("0");
        let last_notif_modified = self.get_setting("last_notif_modified")?;

        let prs = self.load_prs()?;
        let mut repos = self.load_repos()?;
        let rules = self.load_rules()?;
        let notifications = self.load_notifications()?;

        // Ensure every repo present in PRs has a RepoConfig entry.
        for pr in &prs {
            if !repos.iter().any(|r| r.name == pr.repo) {
                let rank = repos.len();
                let cfg = RepoConfig {
                    name: pr.repo.clone(),
                    manual_rank: rank,
                    pinned: false,
                    my_pr_volume: 0,
                    assigned_review_volume: 0,
                    collapsed: false,
                    removed: false,
                    activity: 0,
                };
                let _ = self.upsert_repo(&cfg);
                repos.push(cfg);
            }
        }

        Ok(AppSnapshot {
            prs,
            repos,
            rules,
            notifications,
            draft_stale_hours,
            open_stale_hours,
            onboarded,
            default_filter,
            native_notifications,
            group_order: self.number_list("inbox_group_order")?,
            collapsed_groups: self.number_list("inbox_collapsed_groups")?,
            sync_options: SyncOptions {
                hide_archived: self.flag("hide_archived")?,
                max_age_days: self
                    .get_setting("max_age_days")?
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(crate::github::DEFAULT_MAX_AGE_DAYS),
            },
            last_notif_modified,
            organization: self.get_setting("organization")?.unwrap_or_default(),
            judgments: self.load_judgments()?,
            smart_sort: self.get_setting("smart_sort")?.map(|v| v == "1"),
            hide_external: self.get_setting("hide_external")?.is_some_and(|v| v == "1"),
            show_team_reviews: self
                .get_setting("show_team_reviews")?
                .is_some_and(|v| v == "1"),
            ai_enabled: self.get_setting("ai_enabled")?.is_some_and(|v| v == "1"),
            autonomy_branches: self
                .get_setting("autonomy_branches")?
                .is_some_and(|v| v == "1"),
            autonomy_reviewers: self
                .get_setting("autonomy_reviewers")?
                .is_some_and(|v| v == "1"),
            autonomy_done: self
                .get_setting("autonomy_done")?
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect(),
            pace_enabled: self.flag("pace_enabled")?,
            pace_collapsed: self.get_setting("pace_collapsed")?.is_some_and(|v| v == "1"),
            pace_events: self.load_pace_events()?,
            layout_key: self.get_setting("inbox_layout")?,
            ignored: self.load_ignored()?,
            tracked: self.load_tracked()?,
        })
    }

    pub fn load_tracked(&self) -> Result<Vec<Tracked>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT pr_json, section, state FROM auto_merges ORDER BY changed_at")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (json, section, state) = row?;
            if let Ok(pr) = serde_json::from_str::<PullRequest>(&json) {
                out.push(Tracked {
                    pr,
                    section,
                    state: TrackedState::from_db(&state),
                });
            }
        }
        Ok(out)
    }

    pub fn save_tracked(&self, t: &Tracked) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO auto_merges (pr_id, pr_json, section, state, changed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(pr_id) DO UPDATE SET pr_json = ?2, section = ?3, state = ?4,
               changed_at = CASE WHEN state != ?4 THEN ?5 ELSE changed_at END",
            params![
                t.pr.id,
                serde_json::to_string(&t.pr).unwrap_or_default(),
                t.section,
                t.state.as_str(),
                chrono::Utc::now().timestamp()
            ],
        )?;
        Ok(())
    }

    pub fn delete_tracked(&self, pr_id: &str) -> Result<(), rusqlite::Error> {
        self.conn
            .execute("DELETE FROM auto_merges WHERE pr_id = ?1", params![pr_id])?;
        Ok(())
    }

    /// The last year of what Pace counted, oldest first.
    pub fn load_pace_events(&self) -> Result<Vec<crate::pace::Event>, rusqlite::Error> {
        let since = chrono::Utc::now().timestamp() - 366 * 86_400;
        let mut stmt = self.conn.prepare(
            "SELECT at, kind, pr_id, wait_minutes, lines, verdict FROM pace_events
             WHERE at >= ?1 ORDER BY at",
        )?;
        let rows = stmt.query_map(params![since], |row| {
            Ok(crate::pace::Event {
                at: row.get(0)?,
                kind: crate::pace::Kind::from_db(&row.get::<_, String>(1)?),
                pr_id: row.get(2)?,
                wait_minutes: row.get(3)?,
                lines: row.get(4)?,
                verdict: crate::pace::Verdict::from_db(&row.get::<_, String>(5)?),
            })
        })?;
        rows.collect()
    }

    pub fn add_pace_event(&self, e: &crate::pace::Event) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO pace_events (at, kind, pr_id, wait_minutes, lines, verdict)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![e.at, e.kind.as_str(), e.pr_id, e.wait_minutes, e.lines, e.verdict.as_str()],
        )?;
        Ok(())
    }

    pub fn load_ignored(&self) -> Result<std::collections::HashSet<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT pr_id FROM ignored_prs")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn set_ignored(&self, pr_id: &str, ignored: bool) -> Result<(), rusqlite::Error> {
        if ignored {
            self.conn.execute(
                "INSERT OR REPLACE INTO ignored_prs (pr_id, ignored_at) VALUES (?1, ?2)",
                params![pr_id, chrono::Utc::now().timestamp()],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM ignored_prs WHERE pr_id = ?1", params![pr_id])?;
        }
        Ok(())
    }

    pub fn load_judgments(
        &self,
    ) -> Result<std::collections::HashMap<String, Judgment>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT pr_id, fingerprint, answers, judged_by FROM judgments")?;
        let rows = stmt.query_map([], |row| {
            let answers: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                Judgment {
                    fingerprint: row.get(1)?,
                    answers: serde_json::from_str(&answers).unwrap_or_default(),
                    by: row.get(3)?,
                },
            ))
        })?;
        rows.collect()
    }

    /// Saves judgments and forgets the ones of pull requests that are no longer listed.
    pub fn save_judgments(
        &self,
        judged: &[(String, Judgment)],
        live_ids: &[String],
    ) -> Result<(), rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        let now = chrono::Utc::now().timestamp();
        for (id, j) in judged {
            tx.execute(
                "INSERT INTO judgments (pr_id, fingerprint, answers, judged_by, judged_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(pr_id) DO UPDATE SET fingerprint = ?2, answers = ?3,
                   judged_by = ?4, judged_at = ?5",
                params![
                    id,
                    j.fingerprint,
                    serde_json::to_string(&j.answers).unwrap_or_default(),
                    j.by,
                    now
                ],
            )?;
        }
        let keep: std::collections::HashSet<&String> = live_ids.iter().collect();
        let mut stmt = tx.prepare("SELECT pr_id FROM judgments")?;
        let stale: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter(|id| !keep.contains(id))
            .collect();
        drop(stmt);
        for id in stale {
            tx.execute("DELETE FROM judgments WHERE pr_id = ?1", params![id])?;
        }
        tx.commit()
    }

    /// Small numbers saved as `1,2,3`.
    fn number_list(&self, key: &str) -> Result<Vec<u8>, rusqlite::Error> {
        Ok(self
            .get_setting(key)?
            .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default())
    }

    /// A yes/no choice that is on unless the user switched it off.
    fn flag(&self, key: &str) -> Result<bool, rusqlite::Error> {
        Ok(self.get_setting(key)?.as_deref() != Some("0"))
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn load_prs(&self) -> Result<Vec<PullRequest>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, repo, number, title, url, author, ownership, is_draft,
                    review_state, ci_status, incoming_reason, reviewers_json,
                    idle_hours, unreviewed_hours, head_ref, matched_rule,
                    snoozed_until_hour, additions, deletions, head_repo, stack_base, opened_hours,
                    approved_by
             FROM pull_requests
             ORDER BY number DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let ownership_str: String = row.get(6)?;
            let review_str: String = row.get(8)?;
            let ci_str: String = row.get(9)?;
            let reason_str: String = row.get(10)?;
            let reviewers_json: String = row.get(11)?;
            let reviewers: Vec<String> = serde_json::from_str(&reviewers_json).unwrap_or_default();
            Ok(PullRequest {
                id: row.get(0)?,
                repo: row.get(1)?,
                number: row.get::<_, i64>(2)? as u64,
                title: row.get(3)?,
                url: row.get(4)?,
                author: row.get(5)?,
                ownership: if ownership_str == "MINE" {
                    Ownership::Mine
                } else {
                    Ownership::Theirs
                },
                is_draft: row.get::<_, i64>(7)? != 0,
                review_state: ReviewState::from_db(&review_str),
                ci_status: CiStatus::from_db(&ci_str),
                incoming_reason: IncomingReason::from_db(&reason_str),
                reviewers,
                idle_hours: row.get::<_, i64>(12)? as u64,
                unreviewed_hours: row.get::<_, i64>(13)? as u64,
                head_ref: row.get(14)?,
                head_repo: row.get(19)?,
                stack_base: row.get(20)?,
                opened_hours: row.get::<_, i64>(21)? as u64,
                approved_by: serde_json::from_str(&row.get::<_, String>(22)?).unwrap_or_default(),
                matched_rule: row.get(15)?,
                snoozed_until_hour: row.get::<_, i64>(16)? as u64,
                additions: row.get::<_, i64>(17)? as u32,
                deletions: row.get::<_, i64>(18)? as u32,
                // Merge state is fetched fresh on every sync, never stored.
                merge_readiness: Default::default(),
                merge_method: Default::default(),
                auto_merge: Default::default(),
                external: false,
                body: String::new(),
                description: String::new(),
                labels: Vec::new(),
                private_repo: false,
                files: Vec::new(),
                failing_checks: Vec::new(),
                can_update_branch: false,
                unanswered_threads: None,
                asked: None,
                head_oid: String::new(),
                last_commit_at: String::new(),
                requested_reviewers: Vec::new(),
                changes_requested: Vec::new(),
                approved_at: String::new(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_prs(&self, prs: &[PullRequest]) -> Result<(), rusqlite::Error> {
        // Preserve existing snoozed_until_hour if already snoozed in DB
        let tx = self.conn.unchecked_transaction()?;
        self.replace_prs(prs)?;
        tx.commit()
    }

    fn replace_prs(&self, prs: &[PullRequest]) -> Result<(), rusqlite::Error> {
        let existing = self.load_prs()?;
        self.conn.execute("DELETE FROM pull_requests", [])?;
        for pr in prs {
            let snooze = existing
                .iter()
                .find(|e| e.id == pr.id)
                .map(|e| e.snoozed_until_hour.max(pr.snoozed_until_hour))
                .unwrap_or(pr.snoozed_until_hour);
            let reviewers_json =
                serde_json::to_string(&pr.reviewers).unwrap_or_else(|_| "[]".to_string());
            self.conn.execute(
                "INSERT INTO pull_requests (
                    id, repo, number, title, url, author, ownership, is_draft,
                    review_state, ci_status, incoming_reason, reviewers_json,
                    idle_hours, unreviewed_hours, head_ref, matched_rule,
                    snoozed_until_hour, additions, deletions, head_repo, stack_base, opened_hours,
                    approved_by
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                params![
                    pr.id,
                    pr.repo,
                    pr.number as i64,
                    pr.title,
                    pr.url,
                    pr.author,
                    match pr.ownership {
                        Ownership::Mine => "MINE",
                        Ownership::Theirs => "THEIRS",
                    },
                    i64::from(pr.is_draft),
                    pr.review_state.as_str(),
                    pr.ci_status.as_str(),
                    pr.incoming_reason.as_str(),
                    reviewers_json,
                    pr.idle_hours as i64,
                    pr.unreviewed_hours as i64,
                    pr.head_ref,
                    pr.matched_rule,
                    snooze as i64,
                    i64::from(pr.additions),
                    i64::from(pr.deletions),
                    pr.head_repo,
                    pr.stack_base,
                    pr.opened_hours as i64,
                    serde_json::to_string(&pr.approved_by).unwrap_or_else(|_| "[]".to_string()),
                ],
            )?;
        }
        Ok(())
    }

    pub fn load_repos(&self) -> Result<Vec<RepoConfig>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT name, manual_rank, pinned,
                    my_pr_volume, assigned_review_volume, collapsed, removed, activity
             FROM repos
             ORDER BY manual_rank ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RepoConfig {
                name: row.get(0)?,
                manual_rank: row.get::<_, i64>(1)? as usize,
                pinned: row.get::<_, i64>(2)? != 0,
                my_pr_volume: row.get::<_, i64>(3)? as u32,
                assigned_review_volume: row.get::<_, i64>(4)? as u32,
                collapsed: row.get::<_, i64>(5)? != 0,
                removed: row.get::<_, i64>(6)? != 0,
                activity: row.get::<_, i64>(7)? as u32,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn upsert_repo(&self, repo: &RepoConfig) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO repos (
                name, manual_rank, pinned,
                my_pr_volume, assigned_review_volume, collapsed, removed, activity
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(name) DO UPDATE SET
                manual_rank = excluded.manual_rank,
                pinned = excluded.pinned,
                my_pr_volume = excluded.my_pr_volume,
                assigned_review_volume = excluded.assigned_review_volume,
                collapsed = excluded.collapsed,
                removed = excluded.removed,
                activity = excluded.activity",
            params![
                repo.name,
                repo.manual_rank as i64,
                i64::from(repo.pinned),
                i64::from(repo.my_pr_volume),
                i64::from(repo.assigned_review_volume),
                i64::from(repo.collapsed),
                i64::from(repo.removed),
                i64::from(repo.activity),
            ],
        )?;
        Ok(())
    }

    pub fn save_repos(&self, repos: &[RepoConfig]) -> Result<(), rusqlite::Error> {
        for r in repos {
            self.upsert_repo(r)?;
        }
        Ok(())
    }

    pub fn load_rules(&self) -> Result<Vec<WatchRule>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, query, exclude_drafts, exclude_mine, enabled FROM rules ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WatchRule {
                id: row.get(0)?,
                name: row.get(1)?,
                query: row.get(2)?,
                exclude_drafts: row.get::<_, i64>(3)? != 0,
                exclude_mine: row.get::<_, i64>(4)? != 0,
                enabled: row.get::<_, i64>(5)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn upsert_rule(&self, rule: &WatchRule) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO rules (id, name, query, exclude_drafts, exclude_mine, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                query = excluded.query,
                exclude_drafts = excluded.exclude_drafts,
                exclude_mine = excluded.exclude_mine,
                enabled = excluded.enabled",
            params![
                rule.id,
                rule.name,
                rule.query,
                i64::from(rule.exclude_drafts),
                i64::from(rule.exclude_mine),
                i64::from(rule.enabled),
            ],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> Result<(), rusqlite::Error> {
        self.conn
            .execute("DELETE FROM rules WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn load_notifications(&self) -> Result<Vec<NotificationEvent>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, pr_url, kind, timestamp_label, unread
             FROM notifications
             ORDER BY rowid DESC
             LIMIT 30",
        )?;
        let rows = stmt.query_map([], |row| {
            let kind_str: String = row.get(4)?;
            Ok(NotificationEvent {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                pr_url: row.get(3)?,
                kind: NotificationKind::from_db(&kind_str),
                timestamp_label: row.get(5)?,
                unread: row.get::<_, i64>(6)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_notification(&self, ev: &NotificationEvent) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO notifications (id, title, body, pr_url, kind, timestamp_label, unread)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                ev.id,
                ev.title,
                ev.body,
                ev.pr_url,
                ev.kind.as_str(),
                ev.timestamp_label,
                i64::from(ev.unread),
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn save_onboarding(
        &self,
        repos: &[RepoConfig],
        prs: &[PullRequest],
        settings: &[(&str, &str)],
    ) -> Result<(), rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        self.save_repos(repos)?;
        self.replace_prs(prs)?;
        for (key, value) in settings {
            self.set_setting(key, value)?;
        }
        self.set_setting("onboarded", "1")?;
        tx.commit()
    }
}
