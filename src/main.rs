use rune::{
    Chrome, Color, Cx, El, KeyEvent, Latest, ScrollSpec, TextEdit, TextStyle, Tx, col, el, oklch,
    row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, SegmentedControl, Switch, TextInput};

mod avatars;
mod db;
mod github;
mod model;
mod notify;
mod reorder;
mod stacks;

use db::Database;
use github::{SyncOutput, SyncStatus};
use model::{
    CiStatus, InboxFilter, NotificationEvent, Ownership, PrAttention, PullRequest, RepoConfig,
    WatchRule,
};

/// The Inbox tab's groups, most urgent first, until the user drags them elsewhere.
/// How long a pressed Merge button waits for its confirming press.
const ARMED_SECS: f32 = 4.0;
/// How long a merged PR's row celebrates before it leaves the list.
const CELEBRATION_SECS: f32 = 1.9;

const DEFAULT_GROUP_ORDER: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];

fn current_epoch_hour() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64 / 3600
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Onboarding,
    Preferences,
    Inbox,
}

pub struct App {
    avatars: avatars::Avatars,
    settings_open: bool,
    logo: Option<rune::Image>,
    pub page: Page,
    pub prs: Vec<PullRequest>,
    pub repos: Vec<RepoConfig>,
    pub rules: Vec<WatchRule>,
    pub notifications: Vec<NotificationEvent>,
    /// Automatic ranking of the unpinned repositories, refreshed at launch and on Sync so the
    /// list doesn't reshuffle under the pointer.
    pub rank_scores: Vec<(String, u32)>,
    pub rank_refresh_pending: bool,
    pub inbox_show_all: bool,
    /// The PR whose Merge button was pressed once and waits for a second press.
    pub merge_armed: Option<(String, std::time::Instant)>,
    pub merging: Vec<String>,
    /// PRs just merged, with the moment they were: the row celebrates, then leaves the list.
    pub celebrations: Vec<(String, std::time::Instant)>,
    pub merge_worker: Latest<(String, Result<(), String>)>,
    pub last_synced: Option<std::time::Instant>,
    /// The status text as last drawn, to redraw only when it changes.
    pub sync_label_shown: String,
    /// Order of the Inbox tab's action groups (`PrAttention::action_group` positions).
    pub group_order: Vec<u8>,
    pub collapsed_groups: Vec<u8>,
    pub group_reorder: reorder::Reorder,
    pub max_age_input: TextEdit,
    pub filter: InboxFilter,
    pub selected_repo: Option<String>,
    pub draft_stale_hours: u64,
    pub open_stale_hours: u64,
    pub onboarded: bool,
    pub default_filter: InboxFilter,
    pub native_notifications: bool,
    pub sync_options: github::SyncOptions,
    pub loading: bool,
    pub preview: bool,
    pub sync_status: SyncStatus,
    pub last_notif_modified: Option<String>,
    pub search_query: TextEdit,
    pub new_rule_name: TextEdit,
    pub new_rule_query: TextEdit,
    pub new_rule_exclude_drafts: bool,
    pub new_rule_exclude_mine: bool,
    pub status_banner: Option<String>,
    pub boot_worker: Latest<Result<db::AppSnapshot, rusqlite::Error>>,
    pub sync_worker: Latest<Result<SyncOutput, String>>,
    pub auth_worker: Latest<Result<(), String>>,
    pub fast_notif_worker: Latest<Result<github::NotificationPoll, String>>,
    pub discovery_worker: Latest<Result<github::Discovery, String>>,
    pub verify_worker: Latest<Result<String, String>>,
    pub discovery: Option<github::Discovery>,
    pub discovery_error: Option<String>,
    pub add_repo_open: bool,
    pub confirm_remove: Option<String>,
    pub repo_reorder: reorder::Reorder,
    pub organization: TextEdit,
    pub new_repo: TextEdit,
    pub next_fast_poll_at: f64,
    pub notification_interval: u64,
    pub next_full_sync_at: f64,
    pub allow_os_side_effects: bool,
}

fn init() -> App {
    let headless = !cfg!(test) && rune::is_headless();
    let headless_or_silent = std::env::var("MEAT_PROXY_SILENT_OS_NOTIFY").is_ok() || headless;

    App {
        avatars: avatars::Avatars::default(),
        settings_open: false,
        logo: None,
        page: Page::Onboarding,
        prs: Vec::new(),
        repos: Vec::new(),
        rules: Vec::new(),
        notifications: Vec::new(),
        rank_scores: Vec::new(),
        rank_refresh_pending: true,
        inbox_show_all: false,
        merge_armed: None,
        merging: Vec::new(),
        celebrations: Vec::new(),
        merge_worker: Latest::default(),
        last_synced: None,
        sync_label_shown: String::new(),
        group_order: DEFAULT_GROUP_ORDER.to_vec(),
        collapsed_groups: Vec::new(),
        group_reorder: reorder::Reorder::new(),
        max_age_input: TextEdit::new(&github::DEFAULT_MAX_AGE_DAYS.to_string()),
        filter: InboxFilter::Inbox,
        selected_repo: None,
        draft_stale_hours: 72,
        open_stale_hours: 48,
        onboarded: false,
        default_filter: InboxFilter::Inbox,
        native_notifications: true,
        sync_options: github::SyncOptions::default(),
        loading: true,
        preview: headless,
        sync_status: SyncStatus::Checking,
        last_notif_modified: None,
        search_query: TextEdit::new(""),
        new_rule_name: TextEdit::new(""),
        new_rule_query: TextEdit::new(""),
        new_rule_exclude_drafts: true,
        new_rule_exclude_mine: true,
        status_banner: None,
        boot_worker: Latest::default(),
        sync_worker: Latest::default(),
        auth_worker: Latest::default(),
        fast_notif_worker: Latest::default(),
        discovery_worker: Latest::default(),
        verify_worker: Latest::default(),
        discovery: None,
        discovery_error: None,
        add_repo_open: false,
        confirm_remove: None,
        repo_reorder: reorder::Reorder::new(),
        organization: TextEdit::new(""),
        new_repo: TextEdit::new(""),
        next_fast_poll_at: 0.0,
        notification_interval: github::DEFAULT_POLL_INTERVAL,
        next_full_sync_at: 60.0,
        allow_os_side_effects: !headless_or_silent,
    }
}

impl App {
    /// Pinned repositories first, in the order the user dragged them; the rest by automatic rank.
    pub fn ordered_repos(&self) -> Vec<RepoConfig> {
        let live = self.repos.iter().filter(|r| !r.removed);
        let mut pinned: Vec<RepoConfig> = live.clone().filter(|r| r.pinned).cloned().collect();
        pinned.sort_by(|a, b| {
            a.manual_rank
                .cmp(&b.manual_rank)
                .then_with(|| a.name.cmp(&b.name))
        });
        let score = |r: &RepoConfig| {
            self.rank_scores
                .iter()
                .find(|(name, _)| *name == r.name)
                .map_or_else(
                    || {
                        r.smart_score(
                            &self.prs,
                            self.draft_stale_hours,
                            self.open_stale_hours,
                            current_epoch_hour(),
                        )
                    },
                    |(_, score)| *score,
                )
        };
        let mut rest: Vec<RepoConfig> = live.filter(|r| !r.pinned).cloned().collect();
        rest.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.name.cmp(&b.name)));
        pinned.extend(rest);
        pinned
    }

    /// What the top bar says about the connection: how fresh the data is.
    pub fn sync_label(&self) -> String {
        if self.sync_worker.is_pending() {
            return "Fetching more for you to approve…".to_owned();
        }
        match (&self.sync_status, self.last_synced) {
            (SyncStatus::NeedsAuth { .. }, _) => "Disconnected".to_owned(),
            (SyncStatus::Connected { .. }, Some(at)) => {
                let secs = at.elapsed().as_secs();
                match secs {
                    0..=4 => "Synced just now".to_owned(),
                    5..=59 => format!("Synced {secs}s ago"),
                    60..=3599 => format!("Synced {}m ago", secs / 60),
                    _ => format!("Synced {}h ago", secs / 3600),
                }
            }
            _ => "Fetching more for you to approve…".to_owned(),
        }
    }

    /// Loads what the last run saved: PRs, repositories, rules and every setting.
    pub fn apply_snapshot(&mut self, snap: db::AppSnapshot) {
        self.prs = snap.prs;
        self.repos = snap.repos;
        self.rules = snap.rules;
        self.notifications = snap.notifications;
        self.draft_stale_hours = snap.draft_stale_hours;
        self.open_stale_hours = snap.open_stale_hours;
        self.onboarded = snap.onboarded;
        self.default_filter = snap.default_filter;
        self.filter = snap.default_filter;
        self.native_notifications = snap.native_notifications;
        self.sync_options = snap.sync_options;
        self.group_order = snap.group_order;
        self.collapsed_groups = snap.collapsed_groups;
        self.normalize_group_order();
        self.max_age_input = TextEdit::new(&self.sync_options.max_age_days.to_string());
        self.last_notif_modified = snap.last_notif_modified;
        self.organization = TextEdit::new(&snap.organization);
        self.drop_removed_prs();
        if self.onboarded {
            self.page = Page::Inbox;
        }
    }

    pub fn refresh_ranks(&mut self) {
        let now = current_epoch_hour();
        self.rank_scores = self
            .repos
            .iter()
            .map(|r| {
                (
                    r.name.clone(),
                    r.smart_score(
                        &self.prs,
                        self.draft_stale_hours,
                        self.open_stale_hours,
                        now,
                    ),
                )
            })
            .collect();
        self.rank_refresh_pending = false;
    }

    /// Re-rank the automatic section and sync.
    pub fn sync_now(&mut self) {
        self.rank_refresh_pending = true;
        self.trigger_sync();
    }

    /// A drag ended: `order` is the displayed repositories, pinned first, and the dragged one
    /// moved from `from` to `to`. Dropping among the pinned ones pins it there; dragging a pinned
    /// one below them unpins it. The automatic section can't be reordered by hand.
    /// Shown the first time notifications are on, and whenever they are switched back on, so
    /// the user sees what to expect and macOS asks for permission right then.
    pub fn send_welcome_notification(&self) {
        notify::post(
            "notifications-enabled",
            "meat proxy",
            "Notifications are on. You'll see a note here when a review is requested of you or a PR needs a nudge.",
            None,
            self.allow_os_side_effects,
        );
    }

    /// Keeps the saved order a full list of the known groups.
    fn normalize_group_order(&mut self) {
        let mut order: Vec<u8> = Vec::new();
        for g in self.group_order.iter().chain(DEFAULT_GROUP_ORDER.iter()) {
            if DEFAULT_GROUP_ORDER.contains(g) && !order.contains(g) {
                order.push(*g);
            }
        }
        self.group_order = order;
    }

    pub fn group_rank(&self, group: u8) -> usize {
        self.group_order
            .iter()
            .position(|g| *g == group)
            .unwrap_or(usize::from(group) + 100)
    }

    fn persist_groups(&self) {
        if self.preview {
            return;
        }
        let join = |list: &[u8]| list.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
        if let Ok(db) = Database::open_default() {
            let _ = db.set_setting("inbox_group_order", &join(&self.group_order));
            let _ = db.set_setting("inbox_collapsed_groups", &join(&self.collapsed_groups));
        }
    }

    pub fn toggle_collapse_group(&mut self, group: u8) {
        if let Some(at) = self.collapsed_groups.iter().position(|g| *g == group) {
            self.collapsed_groups.remove(at);
        } else {
            self.collapsed_groups.push(group);
        }
        self.persist_groups();
    }

    /// A group was dragged from `from` to `to` among the `displayed` ones.
    pub fn drop_group(&mut self, displayed: &[u8], from: usize, to: usize) {
        if from >= displayed.len() || to >= displayed.len() || from == to {
            return;
        }
        self.normalize_group_order();
        let (moved, anchor) = (displayed[from], displayed[to]);
        self.group_order.retain(|g| *g != moved);
        if let Some(at) = self.group_order.iter().position(|g| *g == anchor) {
            self.group_order
                .insert(if to < from { at } else { at + 1 }, moved);
        }
        self.persist_groups();
    }

    /// First press arms the button, the second merges. A merge can't be taken back, and the
    /// button sits in a dense list.
    pub fn press_merge(&mut self, pr_id: &str) {
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        if !pr.merge_readiness.is_ready() || !self.merging.is_empty() {
            return;
        }
        let armed = self.merge_armed.as_ref().is_some_and(|(id, _)| id == pr_id);
        if !armed {
            self.merge_armed = Some((pr_id.to_owned(), std::time::Instant::now()));
            return;
        }
        self.merge_armed = None;
        self.merging.push(pr_id.to_owned());
        let (id, url, method) = (pr.id.clone(), pr.url.clone(), pr.merge_method);
        self.merge_worker
            .request((), move |()| (id, github::merge_pr(&url, method)));
    }

    pub fn set_max_age_days(&mut self, days: u32) {
        self.sync_options.max_age_days = days;
        if days > 0 {
            self.prs.retain(|pr| pr.idle_hours < u64::from(days) * 24);
        }
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.set_setting("max_age_days", &days.to_string());
        }
        self.trigger_sync();
    }

    /// The repositories currently drawn as sections on the Mine, Theirs and All tabs.
    pub fn visible_repos(&self) -> Vec<String> {
        self.ordered_repos()
            .into_iter()
            .filter(|repo| {
                let keep_empty = (repo.pinned || repo.activity > 0)
                    && self.filter == InboxFilter::All
                    && self.search_query.text.trim().is_empty();
                keep_empty || !self.prs_for_repo(&repo.name).is_empty()
            })
            .map(|repo| repo.name)
            .collect()
    }

    /// The Inbox tab's action groups that have something in them right now.
    pub fn visible_groups(&self) -> Vec<u8> {
        let now = current_epoch_hour();
        let mut groups: Vec<u8> = self
            .prs
            .iter()
            .filter(|pr| {
                InboxFilter::Inbox.matches(pr, self.draft_stale_hours, self.open_stale_hours, now)
                    && self.matches_search(pr)
            })
            .map(|pr| {
                pr.attention(self.draft_stale_hours, self.open_stale_hours, now)
                    .action_group()
                    .0
            })
            .collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    }

    /// True when every section on screen is folded, so the button should offer to unfold.
    pub fn all_sections_collapsed(&self) -> bool {
        if self.filter == InboxFilter::Inbox {
            let groups = self.visible_groups();
            !groups.is_empty() && groups.iter().all(|g| self.collapsed_groups.contains(g))
        } else {
            let names = self.visible_repos();
            !names.is_empty()
                && names
                    .iter()
                    .all(|n| self.repos.iter().any(|r| r.name == *n && r.collapsed))
        }
    }

    /// Fold every section on screen, or unfold them all when they are all folded already.
    pub fn toggle_collapse_all(&mut self) {
        let collapse = !self.all_sections_collapsed();
        if self.filter == InboxFilter::Inbox {
            for g in self.visible_groups() {
                self.collapsed_groups.retain(|c| *c != g);
                if collapse {
                    self.collapsed_groups.push(g);
                }
            }
            self.persist_groups();
        } else {
            let names = self.visible_repos();
            for r in self.repos.iter_mut().filter(|r| names.contains(&r.name)) {
                r.collapsed = collapse;
            }
            self.persist_repos_now();
        }
    }

    pub fn drop_repo(&mut self, order: &[String], from: usize, to: usize) {
        if from >= order.len() || to >= order.len() || from == to {
            return;
        }
        let is_pinned = |s: &App, name: &str| s.repos.iter().any(|r| r.name == name && r.pinned);
        let pinned_shown = order.iter().filter(|n| is_pinned(self, n)).count();
        let name = order[from].clone();
        if is_pinned(self, &name) {
            if to >= pinned_shown {
                self.set_pinned(&name, false);
                return;
            }
        } else if to < pinned_shown {
            self.set_pinned(&name, true);
        } else {
            return;
        }
        // Rebuild the pinned order with `name` next to the repo it was dropped on.
        let anchor = order[to].clone();
        if anchor == name {
            return;
        }
        let mut names: Vec<(usize, String)> = self
            .repos
            .iter()
            .filter(|r| r.pinned && !r.removed && r.name != name)
            .map(|r| (r.manual_rank, r.name.clone()))
            .collect();
        names.sort();
        let mut names: Vec<String> = names.into_iter().map(|(_, n)| n).collect();
        if let Some(at) = names.iter().position(|n| *n == anchor) {
            let moving_up = to < from || from >= pinned_shown;
            names.insert(if moving_up { at } else { at + 1 }, name);
        } else {
            names.push(name);
        }
        for (rank, n) in names.iter().enumerate() {
            if let Some(r) = self.repos.iter_mut().find(|r| r.name == *n) {
                r.manual_rank = rank;
            }
        }
        self.persist_repos_now();
    }

    fn set_pinned(&mut self, repo_name: &str, pinned: bool) {
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.pinned = pinned;
            if pinned {
                r.manual_rank = usize::MAX / 2;
            }
        }
        self.persist_repos_now();
    }

    fn matches_search(&self, pr: &PullRequest) -> bool {
        let q = self.search_query.text.trim().to_ascii_lowercase();
        q.is_empty()
            || pr.title.to_ascii_lowercase().contains(&q)
            || pr.author.to_ascii_lowercase().contains(&q)
            || pr.repo.to_ascii_lowercase().contains(&q)
            || pr.number.to_string().contains(&q)
    }

    pub fn prs_for_repo(&self, repo_name: &str) -> Vec<PullRequest> {
        let mut list: Vec<PullRequest> = self
            .prs
            .iter()
            .filter(|pr| pr.repo == repo_name)
            .filter(|pr| {
                self.filter.matches(
                    pr,
                    self.draft_stale_hours,
                    self.open_stale_hours,
                    current_epoch_hour(),
                )
            })
            .filter(|pr| self.matches_search(pr))
            .cloned()
            .collect();

        // Teammates before outsiders, then newest activity first: whatever has gone stale usually
        // has a reason.
        list.sort_by(|a, b| {
            a.external
                .cmp(&b.external)
                .then_with(|| a.idle_hours.cmp(&b.idle_hours))
                .then_with(|| b.number.cmp(&a.number))
        });
        list
    }

    pub fn trigger_sync(&mut self) {
        if self.loading || self.preview {
            return;
        }
        let repos = self.repos.clone();
        let rules = self.rules.clone();
        let options = self.sync_options;
        self.sync_worker
            .request((), move |()| github::fetch_live(&repos, &rules, options));
    }

    pub fn persist_repos(&self) {
        if self.onboarded
            && self.page == Page::Inbox
            && !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.save_repos(&self.repos);
        }
    }

    pub fn persist_thresholds(&self) {
        if self.onboarded
            && self.settings_open
            && let Ok(db) = Database::open_default()
        {
            let _ = db.set_setting("draft_stale_hours", &self.draft_stale_hours.to_string());
            let _ = db.set_setting("open_stale_hours", &self.open_stale_hours.to_string());
        }
    }

    pub fn complete_onboarding(&mut self) {
        if !matches!(self.sync_status, SyncStatus::Connected { .. }) {
            self.page = Page::Onboarding;
            return;
        }
        if !self.preview {
            let saved = Database::open_default().and_then(|db| {
                db.save_onboarding(
                    &self.repos,
                    &self.prs,
                    &[
                        ("organization", self.organization.text.trim()),
                        ("default_tab", self.default_filter.name()),
                        ("draft_stale_hours", &self.draft_stale_hours.to_string()),
                        ("open_stale_hours", &self.open_stale_hours.to_string()),
                        (
                            "native_notifications",
                            if self.native_notifications { "1" } else { "0" },
                        ),
                    ],
                )
            });
            if let Err(error) = saved {
                self.status_banner = Some(format!("Could not save setup. {error}"));
                return;
            }
        }
        self.onboarded = true;
        self.filter = self.default_filter;
        self.status_banner = None;
        self.page = Page::Inbox;
        self.next_fast_poll_at = 0.0;
        if self.native_notifications {
            self.send_welcome_notification();
        }
        self.trigger_sync();
    }

    pub fn toggle_pin_repo(&mut self, repo_name: &str) {
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.pinned = !r.pinned;
        }
        self.persist_repos();
    }

    pub fn is_removed(&self, repo_name: &str) -> bool {
        self.repos.iter().any(|r| r.name == repo_name && r.removed)
    }

    pub fn drop_removed_prs(&mut self) {
        let removed: Vec<_> = self
            .repos
            .iter()
            .filter(|r| r.removed)
            .map(|r| r.name.clone())
            .collect();
        self.prs.retain(|pr| !removed.contains(&pr.repo));
    }

    pub fn remove_repo(&mut self, repo_name: &str) {
        self.confirm_remove = None;
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.removed = true;
            r.pinned = false;
        }
        if self.selected_repo.as_deref() == Some(repo_name) {
            self.selected_repo = None;
        }
        self.drop_removed_prs();
        self.persist_repos_now();
        if let Ok(db) = Database::open_default() {
            let _ = db.save_prs(&self.prs);
        }
    }

    pub fn add_repo(&mut self, repo_name: &str) {
        let activity = self
            .discovery
            .as_ref()
            .and_then(|d| {
                d.repos
                    .iter()
                    .find(|r| r.name.eq_ignore_ascii_case(repo_name))
            })
            .map_or(0, |r| r.activity());
        if let Some(r) = self
            .repos
            .iter_mut()
            .find(|r| r.name.eq_ignore_ascii_case(repo_name))
        {
            r.removed = false;
            r.activity = activity.max(1);
        } else {
            let rank = self.repos.len();
            self.repos.push(RepoConfig {
                name: repo_name.to_owned(),
                manual_rank: rank,
                pinned: false,
                my_pr_volume: 0,
                assigned_review_volume: 0,
                collapsed: false,
                removed: false,
                // Never zero, so a repo added by hand stays listed before it has any PRs.
                activity: activity.max(1),
            });
        }
        self.persist_repos_now();
        self.trigger_sync();
    }

    pub fn trigger_discovery(&mut self) {
        if self.preview || self.discovery_worker.is_pending() {
            return;
        }
        self.discovery_error = None;
        self.discovery_worker
            .request((), |()| github::discover_repos());
    }

    /// Writes repositories and the organization even before setup is confirmed.
    fn persist_repos_now(&self) {
        if self.preview {
            return;
        }
        if let Ok(db) = Database::open_default() {
            let _ = db.save_repos(&self.repos);
            let _ = db.set_setting("organization", self.organization.text.trim());
        }
    }

    pub fn toggle_collapse_repo(&mut self, repo_name: &str) {
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.collapsed = !r.collapsed;
        }
        self.persist_repos();
    }

    pub fn add_custom_rule(&mut self) {
        let name = self.new_rule_name.text.trim().to_string();
        let query = self.new_rule_query.text.trim().to_string();
        if name.is_empty() || query.is_empty() {
            self.status_banner =
                Some("Enter both a rule name and a GitHub filter query.".to_string());
            return;
        }
        let id = format!("rule-custom-{}", self.rules.len() + 1);
        let rule = WatchRule {
            id,
            name: name.clone(),
            query,
            exclude_drafts: self.new_rule_exclude_drafts,
            exclude_mine: self.new_rule_exclude_mine,
            enabled: true,
        };
        if let Ok(db) = Database::open_default() {
            let _ = db.upsert_rule(&rule);
        }
        self.rules.push(rule);
        self.new_rule_name.clear();
        self.new_rule_query.clear();
        self.status_banner = Some(format!("Added rule \"{name}\""));
        self.trigger_sync();
    }

    pub fn toggle_rule(&mut self, rule_id: &str) {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == rule_id) {
            r.enabled = !r.enabled;
            if let Ok(db) = Database::open_default() {
                let _ = db.upsert_rule(r);
            }
        }
    }

    pub fn remove_rule(&mut self, rule_id: &str) {
        self.rules.retain(|r| r.id != rule_id);
        if let Ok(db) = Database::open_default() {
            let _ = db.delete_rule(rule_id);
        }
    }
}

fn setup(s: &mut App, waker: rune::Waker) {
    let decoder = std::sync::Arc::new(rune::ImageCache::new(waker.clone()));
    let logo = decoder.load(&include_bytes!("../assets/logo.png")[..]);
    s.avatars.setup(decoder, waker.clone());
    if s.preview {
        let _ = logo.wait();
    }
    s.logo = Some(logo);
    if s.preview {
        s.avatars.load_preview(&s.prs);
    }
    if s.preview {
        return;
    }
    if s.allow_os_side_effects {
        rune_mac::notify::on_click(|click| {
            if let Some(url) = click
                .get("url")
                .filter(|url| url.starts_with("https://github.com/"))
            {
                notify::open_url_in_browser(url, true);
            }
        });
    }
    s.boot_worker.set_waker(waker.clone());
    s.sync_worker.set_waker(waker.clone());
    s.auth_worker.set_waker(waker.clone());
    s.fast_notif_worker.set_waker(waker.clone());
    s.discovery_worker.set_waker(waker.clone());
    s.verify_worker.set_waker(waker.clone());
    s.merge_worker.set_waker(waker);
    s.boot_worker.request((), |()| {
        Database::open_default().and_then(|db| db.load_snapshot())
    });
}

fn deliver_events(s: &mut App, events: Vec<NotificationEvent>) -> bool {
    if !s.onboarded || !s.native_notifications || !s.allow_os_side_effects || events.is_empty() {
        return false;
    }
    let mut delivered = false;
    if let Ok(db) = Database::open_default() {
        for event in events {
            if db.insert_notification(&event).unwrap_or(false) {
                notify::dispatch_system_notification(&event, true);
                s.notifications.insert(0, event);
                delivered = true;
            }
        }
    }
    delivered
}

fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    if s.preview {
        return false;
    }
    let mut changed = false;

    // The age limit is typed in Settings; apply it once it parses as a number.
    if let Ok(days) = s.max_age_input.text.trim().parse::<u32>()
        && days != s.sync_options.max_age_days
    {
        s.set_max_age_days(days);
        changed = true;
    }

    if let Some(snapshot) = s.boot_worker.take() {
        s.loading = false;
        match snapshot {
            Ok(snap) => s.apply_snapshot(snap),
            Err(error) => s.status_banner = Some(format!("Could not read saved settings. {error}")),
        }
        s.trigger_sync();
        s.trigger_discovery();
        changed = true;
    }

    if let Some(result) = s.discovery_worker.take() {
        match result {
            Ok(found) => {
                for repo in &mut s.repos {
                    if let Some(hit) = found.repos.iter().find(|r| r.name == repo.name) {
                        repo.activity = hit.activity();
                    }
                }
                if s.organization.text.trim().is_empty() && found.organizations.len() == 1 {
                    s.organization = TextEdit::new(&found.organizations[0]);
                }
                s.discovery = Some(found);
                if s.onboarded {
                    s.persist_repos_now();
                }
            }
            Err(error) => s.discovery_error = Some(error),
        }
        changed = true;
    }

    if let Some(result) = s.verify_worker.take() {
        match result {
            Ok(name) => {
                s.new_repo = TextEdit::new("");
                s.add_repo(&name);
            }
            Err(error) => s.status_banner = Some(error),
        }
        changed = true;
    }

    if let Some(result) = s.auth_worker.take() {
        s.status_banner = Some(match result {
            Ok(()) => "Finish signing in, then check the connection.".to_string(),
            Err(error) => format!("Could not open Terminal. {error}"),
        });
        changed = true;
    }

    if let Some(result) = s.sync_worker.take() {
        match result {
            Ok(mut out) => {
                out.prs.retain(|pr| !s.is_removed(&pr.repo));
                for pr in &mut out.prs {
                    if let Some(previous) = s.prs.iter().find(|p| p.id == pr.id) {
                        pr.snoozed_until_hour = previous.snoozed_until_hour;
                    }
                }
                let events = notify::evaluate_inbox_notifications(
                    &s.prs,
                    &out.prs,
                    &s.notifications,
                    s.draft_stale_hours,
                    s.open_stale_hours,
                    current_epoch_hour(),
                );
                if !matches!(s.sync_status, SyncStatus::Connected { .. }) {
                    s.next_fast_poll_at = clock;
                }
                s.sync_status = out.status;
                s.last_synced = Some(std::time::Instant::now());
                s.repos = github::infer_repos_from_prs(&out.prs, &s.repos);
                s.prs = out.prs;
                if s.rank_refresh_pending {
                    s.refresh_ranks();
                }
                if s.onboarded {
                    deliver_events(s, events);
                    if let Ok(db) = Database::open_default() {
                        let _ = db.save_prs(&s.prs);
                        let _ = db.save_repos(&s.repos);
                    }
                }
            }
            Err(reason) => s.sync_status = SyncStatus::NeedsAuth { reason },
        }
        changed = true;
    }

    if let Some(Ok(notifications)) = s.fast_notif_worker.take() {
        s.notification_interval = notifications.interval;
        s.next_fast_poll_at = clock + s.notification_interval as f64;
        let events: Vec<_> = notifications
            .events
            .into_iter()
            .filter(|e| {
                !s.repos.iter().any(|r| {
                    r.removed
                        && e.pr_url
                            .starts_with(&format!("https://github.com/{}/", r.name))
                })
            })
            .collect();
        if deliver_events(s, events) && !s.sync_worker.is_pending() {
            s.trigger_sync();
        }
        if let Some(modified) = notifications.modified
            && s.last_notif_modified.as_ref() != Some(&modified)
        {
            if let Ok(db) = Database::open_default() {
                let _ = db.set_setting("last_notif_modified", &modified);
            }
            s.last_notif_modified = Some(modified);
        }
    }

    if clock >= s.next_fast_poll_at {
        s.next_fast_poll_at = clock + s.notification_interval as f64;
        if s.onboarded
            && s.native_notifications
            && s.allow_os_side_effects
            && matches!(s.sync_status, SyncStatus::Connected { .. })
            && !s.fast_notif_worker.is_pending()
        {
            let lm = s.last_notif_modified.clone();
            s.fast_notif_worker
                .request((), move |()| github::check_notifications(lm.as_deref()));
        }
    }
    if clock >= s.next_full_sync_at {
        s.next_full_sync_at = clock + 60.0;
        if s.onboarded && !s.sync_worker.is_pending() {
            s.trigger_sync();
        }
    }
    if let Some((id, result)) = s.merge_worker.take() {
        s.merging.retain(|m| *m != id);
        match result {
            Ok(()) => {
                notify::play_sound("Hero", s.allow_os_side_effects);
                s.celebrations.push((id, std::time::Instant::now()));
            }
            Err(error) => s.status_banner = Some(format!("Could not merge. {error}")),
        }
        changed = true;
    }
    if s.merge_armed
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed().as_secs_f32() > ARMED_SECS)
    {
        s.merge_armed = None;
        changed = true;
    }
    if !s.celebrations.is_empty() {
        // Redraw every frame while the confetti flies, then let the merged PR go.
        let done: Vec<String> = s
            .celebrations
            .iter()
            .filter(|(_, at)| at.elapsed().as_secs_f32() > CELEBRATION_SECS)
            .map(|(id, _)| id.clone())
            .collect();
        if !done.is_empty() {
            s.celebrations.retain(|(id, _)| !done.contains(id));
            s.prs.retain(|pr| !done.contains(&pr.id));
            if !s.preview
                && let Ok(db) = Database::open_default()
            {
                let _ = db.save_prs(&s.prs);
            }
            s.trigger_sync();
        }
        tx.wake_after(std::time::Duration::from_millis(16));
        changed = true;
    }
    let label = s.sync_label();
    if label != s.sync_label_shown {
        s.sync_label_shown = label;
        changed = true;
    }
    if s.onboarded {
        // Wake each second at most, so "Synced 12s ago" keeps counting.
        tx.wake_after(std::time::Duration::from_secs_f64(
            (s.next_fast_poll_at.min(s.next_full_sync_at) - clock).clamp(0.01, 1.0),
        ));
    }

    if s.page == Page::Inbox {
        changed |= s.avatars.poll(&s.prs);
    }
    changed
}

// --- Minimal Document UI ---

const GEAR: rune::Icon = rune::Icon {
    name: "settings-gear",
    view: 24.0,
    body: r#"<path stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" d="M10 3h4l.6 2.4 1.5.9 2.4-.7 2 3.4-1.8 1.7v2.5l1.8 1.7-2 3.4-2.4-.7-1.5.9L14 21h-4l-.6-2.5-1.5-.9-2.4.7-2-3.4 1.8-1.7v-2.5L3.5 9l2-3.4 2.4.7 1.5-.9Z"/><circle cx="12" cy="12" r="3.1" stroke="currentColor" stroke-width="1.7"/>"#,
};
const STACK_BRANCH: rune::Icon = rune::Icon {
    name: "stack-branch",
    view: 16.0,
    body: r#"<path d="M3 1v6a3 3 0 0 0 3 3h6m-3-3 3 3-3 3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
const PR_ICON: rune::Icon = rune::Icon {
    name: "pull-request",
    view: 20.0,
    body: r#"<circle stroke="currentColor" stroke-width="1.6" cx="5" cy="4" r="2"/><circle stroke="currentColor" stroke-width="1.6" cx="5" cy="16" r="2"/><circle stroke="currentColor" stroke-width="1.6" cx="15" cy="16" r="2"/><path stroke="currentColor" stroke-width="1.6" d="M5 6v8m10 0V9a4 4 0 0 0-4-4h-1m3-3-3 3 3 3"/>"#,
};
const REPO_ICON: rune::Icon = rune::Icon {
    name: "repository",
    view: 20.0,
    body: r#"<path stroke="currentColor" stroke-width="1.6" d="M4 15V4a1 1 0 0 1 1-1h11v14H6a2 2 0 0 1 0-4h10M8 6h5"/>"#,
};

fn mint(k: &Kit) -> Color {
    oklch(if k.dark { 0.83 } else { 0.44 }, 0.10, 161.0)
}

fn surface(k: &Kit) -> Color {
    oklch(if k.dark { 0.185 } else { 1.0 }, 0.003, 175.0)
}

fn panel(k: &Kit) -> El {
    col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .radius(9.0)
        .border(1.0, k.c.border_weak)
        .fill(surface(k))
}

fn heading(k: &Kit, label: &str) -> El {
    text(label)
        .font(TextStyle::new(31.0, 660.0))
        .color(k.c.text_base)
}

fn top_bar(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let settings_btn = if s.page == Page::Inbox {
        rune_kit::IconButton::new(&GEAR)
            .variant(rune_kit::IconButtonVariant::Ghost)
            .control_size(ControlSize::Large)
            .label("Settings")
            .help("Settings (⌘,)")
            .on_press(|s: &mut App| s.settings_open = true)
            .build(k)
            .key("toggle-settings-btn")
    } else {
        el()
    };
    let context = match s.page {
        Page::Onboarding => "Setup  /  1 of 2".to_owned(),
        Page::Preferences => "Setup  /  2 of 2".to_owned(),
        _ => s.sync_label(),
    };
    let sync_status = row()
        .key("sync-status")
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .help("Sync now (⌘R)")
        .on_press(|s: &mut App| s.sync_now())
        .child(text(context).font(k.ty.mono).color(c.text_muted));
    let context = if matches!(s.page, Page::Inbox) {
        sync_status
    } else {
        row().center_y().child(
            text(match s.page {
                Page::Onboarding => "Setup  /  1 of 2",
                _ => "Setup  /  2 of 2",
            })
            .font(k.ty.mono)
            .color(c.text_muted),
        )
    };
    let keycap = if let Some(image) = &s.logo {
        el().size(22.0, 22.0).pixels(image.pixels())
    } else {
        el().size(22.0, 22.0)
    };
    col()
        .key("top-header")
        .w_full()
        .child(
            row()
                .w_full()
                .h(50.0)
                .pl(84.0)
                .pr(24.0)
                .center_y()
                .justify_between()
                .child(
                    row()
                        .center_y()
                        .gap(9.0)
                        .cursor(rune::Cursor::Pointer)
                        .on_press(|s: &mut App| {
                            if s.page == Page::Inbox {
                                s.page = Page::Inbox;
                                s.selected_repo = None;
                            }
                        })
                        .child(keycap)
                        .child(
                            text("meat proxy")
                                .font(TextStyle::new(14.0, 650.0))
                                .color(c.text_base),
                        ),
                )
                .child(
                    row()
                        .center_y()
                        .gap(14.0)
                        .child(context)
                        .child(settings_btn),
                ),
        )
        .child(el().w_full().h(1.0).fill(c.border_weak))
}

fn inbox_tools(s: &App, k: &Kit) -> El {
    let labels = InboxFilter::TABS.map(|(_, label)| label);
    let filter_seg = SegmentedControl::new(&labels, Some(s.filter.index()))
        .label("Filter")
        .width(Some(280.0))
        .on_pick(|s: &mut App, idx, _| {
            s.filter = InboxFilter::from_index(idx);
        })
        .build(k)
        .key("inbox-filter-segments");

    let search = TextInput::new(&s.search_query)
        .placeholder("Search pull requests…")
        .label("Search")
        .width(Some(170.0))
        .on_edit(|s: &mut App| &mut s.search_query)
        .build(k)
        .key("inbox-search-input");

    row()
        .w_full()
        .center_y()
        .justify_between()
        .gap(8.0)
        .child(filter_seg)
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child({
                    Button::new(if s.all_sections_collapsed() {
                        "Expand all"
                    } else {
                        "Collapse all"
                    })
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Large)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| {
                        tx.motion_curve(rune::Curve::tween(
                            0.12,
                            rune::Easing::cubic(0.2, 0.0, 0.0, 1.0),
                        ));
                        s.toggle_collapse_all()
                    })
                    .build(k)
                    .key("collapse-all")
                })
                .child(search)
                .child(
                    Button::new("Add repository")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Large)
                        .on_press(|s: &mut App| {
                            s.add_repo_open = true;
                            if s.discovery.is_none() {
                                s.trigger_discovery();
                            }
                        })
                        .build(k)
                        .key("add-repo-open"),
                ),
        )
}

fn onboarding_page_view(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let preferences = s.page == Page::Preferences;
    let connected = match &s.sync_status {
        SyncStatus::Connected { username } => Some(username),
        _ => None,
    };
    let mut doc = col()
        .key(if preferences {
            "setup-preferences"
        } else {
            "setup-github"
        })
        .w_full()
        .max_w(640.0)
        .py(32.0)
        .gap(16.0)
        .child(
            text(if preferences {
                "Set up your inbox"
            } else {
                "Connect GitHub"
            })
            .font(TextStyle::new(31.0, 660.0))
            .color(c.text_base),
        );

    if let Some(message) = &s.status_banner {
        doc = doc.child(text(message.clone()).font(k.ty.small).color(c.text_muted));
    }

    if !preferences {
        let status = if let Some(username) = connected {
            format!("Signed in as @{username} through GitHub CLI.")
        } else if s.loading
            || s.sync_worker.is_pending()
            || matches!(s.sync_status, SyncStatus::Checking)
        {
            "Checking GitHub CLI…".to_string()
        } else {
            "GitHub CLI needs attention.".to_string()
        };
        doc = doc
            .child(
                text("Track the pull requests you open and the ones waiting for your review.")
                    .font(k.ty.body)
                    .color(c.text_muted),
            )
            .child(
                row()
                    .w_full()
                    .px(16.0)
                    .py(14.0)
                    .radius(8.0)
                    .fill(surface(k))
                    .border(1.0, c.border_weak)
                    .center_y()
                    .gap(12.0)
                    .child(text("❯_").font(k.ty.mono).color(mint(k)))
                    .child(text(status).font(k.ty.body).color(c.text_base)),
            );
        if connected.is_some() {
            doc = doc.child(
                row().child(
                    Button::new("Continue")
                        .variant(ButtonVariant::Contrast)
                        .on_press(|s: &mut App| {
                            s.status_banner = None;
                            s.page = Page::Preferences;
                        })
                        .build(k)
                        .key("onboard-continue"),
                ),
            );
        } else if let SyncStatus::NeedsAuth { reason } = &s.sync_status {
            doc = doc
                .child(row().gap(8.0)
                    .child(Button::new("Sign in with gh")
                        .variant(ButtonVariant::Contrast)
                        .disabled(s.auth_worker.is_pending())
                        .on_press(|s: &mut App| {
                            if s.allow_os_side_effects {
                                s.auth_worker.request((), |()| notify::launch_gh_auth_in_terminal());
                            }
                        })
                        .build(k)
                        .key("onboard-run-gh-auth"))
                    .child(Button::new("Check connection")
                        .variant(ButtonVariant::Outline)
                        .disabled(s.sync_worker.is_pending())
                        .on_press(|s: &mut App| {
                            s.status_banner = None;
                            s.trigger_sync();
                        })
                        .build(k)
                        .key("onboard-recheck-gh")))
                .child(text("You can also run gh auth login --hostname github.com --web in your terminal.")
                    .font(k.ty.small).color(c.text_muted))
                .child(text(reason.clone()).wrap().font(k.ty.small).color(c.text_faint));
        }
    } else {
        let open_hours = [24, 48, 72, 120];
        let draft_hours = [48, 72, 120, 168];
        let choices = panel(k)
            .child(default_filter_row(s, k))
            .child(setup_row(
                k,
                "Open PR reminder",
                SegmentedControl::new(
                    &["1d", "2d", "3d", "5d"],
                    open_hours.iter().position(|&h| h == s.open_stale_hours),
                )
                .label("Open PR reminder")
                .width(Some(168.0))
                .on_pick(move |s: &mut App, idx, _| s.open_stale_hours = open_hours[idx])
                .build(k),
            ))
            .child(setup_row(
                k,
                "Draft reminder",
                SegmentedControl::new(
                    &["2d", "3d", "5d", "7d"],
                    draft_hours.iter().position(|&h| h == s.draft_stale_hours),
                )
                .label("Draft reminder")
                .width(Some(168.0))
                .on_pick(move |s: &mut App, idx, _| s.draft_stale_hours = draft_hours[idx])
                .build(k),
            ))
            .child(setup_row(
                k,
                "macOS notifications",
                Switch::new(s.native_notifications)
                    .label("macOS notifications")
                    .hide_label()
                    .on_toggle(|s: &mut App| s.native_notifications = !s.native_notifications)
                    .build(k),
            ));
        doc = doc.child(choices).child(repository_manager(s, k));
    }

    // The scroll sits on a column: a row clips its children to its own height and never overflows.
    let mut page = col().w_full().h_full().min_h(0.0).child(
        col()
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .grow(1.0)
            .min_h(0.0)
            .child(row().w_full().px(28.0).justify_center().child(doc)),
    );
    if preferences {
        page = page.child(el().w_full().h(1.0).fill(c.border_weak)).child(
            row().w_full().px(28.0).py(12.0).justify_center().child(
                row()
                    .w_full()
                    .max_w(640.0)
                    .justify_between()
                    .center_y()
                    .child(
                        Button::new("Back")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut App| s.page = Page::Onboarding)
                            .build(k),
                    )
                    .child(
                        Button::new("Confirm and open inbox")
                            .variant(ButtonVariant::Contrast)
                            .on_press(|s: &mut App| s.complete_onboarding())
                            .build(k)
                            .key("confirm-onboarding-btn"),
                    ),
            ),
        );
    }
    page
}

fn setup_row(k: &Kit, label: &str, control: El) -> El {
    row()
        .w_full()
        .gap(10.0)
        .center_y()
        .justify_between()
        .child(text(label).font(k.ty.body).color(k.c.text_base))
        .child(control)
}

fn icon_action(glyph: &'static rune::Icon, label: &str) -> rune_kit::IconButton {
    rune_kit::IconButton::new(glyph)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .label(label.to_owned())
        .help(label.to_owned())
}

fn repository_manager(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let mut section = col().w_full().gap(REPO_ROW_GAP).child(
        row()
            .w_full()
            .center_y()
            .justify_between()
            .child(text("Repositories").font(k.ty.ui_medium).color(c.text_base))
            .child(
                icon_action(
                    &PLUS,
                    if s.add_repo_open {
                        "Close add repository"
                    } else {
                        "Add repository"
                    },
                )
                .on_press(|s: &mut App| {
                    s.add_repo_open = !s.add_repo_open;
                    if s.add_repo_open && s.discovery.is_none() {
                        s.trigger_discovery();
                    }
                })
                .build(k)
                .key("add-repo-toggle"),
            ),
    );
    if s.add_repo_open {
        section = section.child(add_repository_panel(s, k, true));
    }

    let ordered = s.ordered_repos();
    let visible: Vec<_> = ordered.iter().enumerate().collect();
    if visible.is_empty() {
        section = section.child(
            text("No repositories yet. Add one with +.")
                .font(k.ty.body)
                .color(c.text_muted),
        );
    }
    let count = visible.len();
    let order: Vec<String> = visible.iter().map(|(_, r)| r.name.clone()).collect();
    for (position, &(_, repo)) in visible.iter().enumerate() {
        let remove = repo.name.clone();
        let order = order.clone();
        let lifted = s.repo_reorder.dragging() == Some(position);
        let grip = row()
            .key(format!("repo-grip-{}", repo.name))
            .role("button")
            .label(format!("Drag to reorder {}", repo.name))
            .size(20.0, 20.0)
            .center()
            .cursor(if lifted {
                rune::Cursor::Grabbing
            } else {
                rune::Cursor::Grab
            })
            .on_drag(
                move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                    rune::Phase::Down => {}
                    rune::Phase::Move => {
                        tx.animated();
                        s.repo_reorder.follow(
                            position,
                            count,
                            d.dy,
                            REPO_ROW_HEIGHT + REPO_ROW_GAP,
                        );
                    }
                    rune::Phase::Up => {
                        if let Some((from, to)) = s.repo_reorder.end(d.cancelled()) {
                            s.drop_repo(&order, from, to);
                        }
                    }
                },
            )
            .child(rune::icon(&GRIP, 14.0).color(c.text_faint).a11y_hidden());
        let mut line = row()
            .key(format!("onboard-repo-{}", repo.name))
            .w_full()
            .h(REPO_ROW_HEIGHT)
            .gap(8.0)
            .center_y()
            .offset(0.0, s.repo_reorder.shift(position))
            .child(grip)
            .child(
                row()
                    .grow(1.0)
                    .child(text(repo.name.clone()).font(k.ty.body).color(c.text_base)),
            )
            .child(if s.confirm_remove.as_deref() == Some(repo.name.as_str()) {
                row()
                    .center_y()
                    .gap(4.0)
                    .child(
                        Button::new("Remove")
                            .variant(ButtonVariant::Danger)
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut App| s.remove_repo(&remove))
                            .build(k)
                            .key(format!("confirm-remove-{}", repo.name)),
                    )
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .control_size(ControlSize::Small)
                            .on_press(|s: &mut App| s.confirm_remove = None)
                            .build(k),
                    )
            } else {
                icon_action(&TRASH, &format!("Remove {}", repo.name))
                    .on_press(move |s: &mut App| s.confirm_remove = Some(remove.clone()))
                    .build(k)
                    .key(format!("remove-repo-{}", repo.name))
            });
        if lifted {
            line = line
                .raise()
                .offset_motion(rune::Anim::INSTANT)
                .fill(c.layer_01);
        }
        section = section.child(line);
    }
    section
}

fn add_repository_panel(s: &App, k: &Kit, framed: bool) -> El {
    let c = &k.c;
    let org = s.organization.text.trim().to_ascii_lowercase();
    let mut panel = col().w_full().gap(10.0);
    if framed {
        panel = panel
            .padding(12.0)
            .radius(8.0)
            // One step off the repository rows (layer_01), still neutral.
            .fill(c.layer_02)
            .border(1.0, c.border_weak)
            .child(
                row()
                    .w_full()
                    .center_y()
                    .justify_between()
                    .child(
                        text("Add repositories")
                            .font(k.ty.ui_medium)
                            .color(c.text_base),
                    )
                    .child(
                        icon_action(&CLOSE, "Close")
                            .on_press(|s: &mut App| s.add_repo_open = false)
                            .build(k)
                            .key("add-repo-close"),
                    ),
            );
    }

    let mut org_row = row().w_full().center_y().gap(8.0).child(
        TextInput::new(&s.organization)
            .placeholder("Your organization, e.g. saleor")
            .width(Some(240.0))
            .on_edit(|s: &mut App| &mut s.organization)
            .build(k),
    );
    if let Some(found) = &s.discovery {
        for name in found.organizations.iter().take(4) {
            let pick = name.clone();
            org_row = org_row.child(
                Button::new(name.clone())
                    .variant(if name.to_ascii_lowercase() == org {
                        ButtonVariant::Contrast
                    } else {
                        ButtonVariant::Outline
                    })
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| {
                        s.organization = TextEdit::new(&pick);
                        s.persist_repos_now();
                    })
                    .build(k)
                    .key(format!("org-chip-{name}")),
            );
        }
    }
    panel = panel.child(org_row);

    match (&s.discovery, &s.discovery_error) {
        (_, Some(error)) => {
            panel = panel.child(
                text(error.clone())
                    .wrap()
                    .font(k.ty.small)
                    .color(c.text_muted),
            );
        }
        (None, None) => {
            panel = panel.child(
                text("Finding the repositories you work in…")
                    .font(k.ty.small)
                    .color(c.text_muted),
            );
        }
        (Some(found), None) => {
            let suggestions: Vec<_> = found
                .repos
                .iter()
                .filter(|r| org.is_empty() || r.owner().to_ascii_lowercase() == org)
                .filter(|r| {
                    !s.repos
                        .iter()
                        .any(|t| !t.removed && t.name.eq_ignore_ascii_case(&r.name))
                })
                .take(8)
                .collect();
            if suggestions.is_empty() {
                panel = panel.child(
                    text("No more recent repositories. Add one by name below.")
                        .font(k.ty.small)
                        .color(c.text_muted),
                );
            }
            for repo in suggestions {
                let name = repo.name.clone();
                panel = panel.child(
                    row()
                        .w_full()
                        .center_y()
                        .justify_between()
                        .child(text(repo.name.clone()).font(k.ty.body).color(c.text_base))
                        .child(
                            row()
                                .center_y()
                                .gap(8.0)
                                .child(
                                    text(format!(
                                        "{} PRs · {} reviews",
                                        repo.prs_90d, repo.reviews_90d
                                    ))
                                    .font(k.ty.small)
                                    .color(c.text_faint),
                                )
                                .child(
                                    icon_action(&PLUS, &format!("Add {}", repo.name))
                                        .on_press(move |s: &mut App| s.add_repo(&name))
                                        .build(k)
                                        .key(format!("add-repo-{}", repo.name)),
                                ),
                        ),
                );
            }
        }
    }

    let panel = panel.child(
        row()
            .w_full()
            .center_y()
            .gap(8.0)
            .child(
                TextInput::new(&s.new_repo)
                    .placeholder("Or add by name: owner/name")
                    .width(Some(240.0))
                    .on_edit(|s: &mut App| &mut s.new_repo)
                    .build(k),
            )
            .child(
                Button::new("Add")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .disabled(s.verify_worker.is_pending() || s.new_repo.text.trim().is_empty())
                    .on_press(|s: &mut App| {
                        let input = s.new_repo.text.clone();
                        s.verify_worker
                            .request((), move |()| github::verify_repo(&input));
                    })
                    .build(k)
                    .key("add-repo-by-name"),
            ),
    );
    match &s.status_banner {
        Some(message) if !framed => panel.child(
            text(message.clone())
                .wrap()
                .font(k.ty.small)
                .color(c.text_muted),
        ),
        _ => panel,
    }
}

fn default_filter_row(s: &App, k: &Kit) -> El {
    setup_row(
        k,
        "Default inbox",
        SegmentedControl::new(
            &InboxFilter::TABS.map(|(_, label)| label),
            Some(s.default_filter.index()),
        )
        .label("Default inbox")
        .width(Some(260.0))
        .on_pick(|s: &mut App, idx, _| {
            s.default_filter = InboxFilter::from_index(idx);
            if s.onboarded
                && s.settings_open
                && let Ok(db) = Database::open_default()
            {
                let _ = db.set_setting("default_tab", s.default_filter.name());
            }
        })
        .build(k)
        .key("default-inbox-filter"),
    )
}

const PLUS: rune::Icon = rune::Icon {
    name: "add",
    view: 16.0,
    body: r#"<path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#,
};

const TRASH: rune::Icon = rune::Icon {
    name: "remove",
    view: 16.0,
    body: r#"<path d="M3.5 4.8h9M6.4 4.8V3.4h3.2v1.4M4.8 4.8l.5 7.4h5.4l.5-7.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

const CLOSE: rune::Icon = rune::Icon {
    name: "close",
    view: 16.0,
    body: r#"<path d="M4.5 4.5l7 7M11.5 4.5l-7 7" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#,
};

const GRIP: rune::Icon = rune::Icon {
    name: "drag-handle",
    view: 16.0,
    body: r#"<circle cx="6" cy="4" r="1.1"/><circle cx="10" cy="4" r="1.1"/><circle cx="6" cy="8" r="1.1"/><circle cx="10" cy="8" r="1.1"/><circle cx="6" cy="12" r="1.1"/><circle cx="10" cy="12" r="1.1"/>"#,
};

const SECTION_GAP: f32 = 16.0;
/// Header plus its 1px rule and the section border.
const HEADER_HEIGHT: f32 = 40.0;
const SECTION_HEIGHT: f32 = HEADER_HEIGHT + 3.0;
const REPO_ROW_HEIGHT: f32 = 32.0;
const REPO_ROW_GAP: f32 = 4.0;

const PIN: rune::Icon = rune::Icon {
    name: "pin",
    view: 24.0,
    body: r#"<path d="M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

// Matching the resting surface keeps the glyph quiet. Keyboard focus paints a
// contrasting tile behind it even when the pointer is in another row.
fn author_badge(s: &App, pr: &PullRequest, k: &Kit) -> El {
    let size = 17.0;
    let mut avatar = row()
        .size(size, size)
        .radius(size / 2.0)
        .clip_rounded(size / 2.0)
        .center()
        .fill(k.c.layer_02)
        .a11y_hidden();
    avatar = if let Some(image) = s.avatars.get(&pr.author) {
        avatar.child(el().size(size, size).pixels(image.pixels()))
    } else {
        let initial = pr
            .author
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('?');
        avatar.child(
            text(initial.to_string())
                .font(TextStyle::new(10.0, 620.0))
                .color(k.c.text_muted),
        )
    };
    let handle = format!("@{}", pr.author);
    let author = row().center_y().gap(5.0).help(handle.clone()).child(avatar);
    if pr.ownership == Ownership::Mine {
        let tint = rune::oklch(if k.dark { 0.29 } else { 0.94 }, 0.045, 295.0);
        let ink = rune::oklch(if k.dark { 0.86 } else { 0.42 }, 0.12, 295.0);
        author
            .pl(2.0)
            .pr(6.0)
            .py(1.0)
            .radius(10.0)
            .fill(tint)
            .child(text("You").font(k.ty.small_medium).color(ink))
    } else {
        let author = author.child(text(handle).font(k.ty.small).color(k.c.text_muted));
        if pr.external {
            author.child(text("external").font(k.ty.small).color(k.c.text_faint))
        } else {
            author
        }
    }
}

fn minimal_pr_row(s: &App, item: &stacks::StackRow, k: &Kit, show_repo: bool) -> El {
    let pr = item.pr;
    let c = &k.c;
    let att = pr.attention(
        s.draft_stale_hours,
        s.open_stale_hours,
        current_epoch_hour(),
    );

    let (status, tone) = match att {
        PrAttention::IncomingAssigned => ("Review requested".to_owned(), mint(k)),
        PrAttention::IncomingViaTeam => ("Team review requested".to_owned(), c.text_muted),
        PrAttention::MyOpenNeedsNudge if pr.reviewers.is_empty() => {
            ("Needs reviewer".to_owned(), c.warning.fg)
        }
        PrAttention::MyOpenNeedsNudge => (
            format!("Unreviewed {}d", (pr.unreviewed_hours / 24).max(1)),
            c.warning.fg,
        ),
        PrAttention::MyDraftNeedsNudge => ("Stale draft".to_owned(), c.warning.fg),
        PrAttention::MyOpenChangesRequested => ("Changes requested".to_owned(), c.danger.fg),
        PrAttention::MyOpenApproved => ("Approved".to_owned(), mint(k)),
        PrAttention::MyOpenWaiting => ("Waiting for review".to_owned(), c.text_muted),
        PrAttention::MyDraftActive => ("Draft".to_owned(), c.text_muted),
        PrAttention::IncomingWatched => ("Watching".to_owned(), c.text_muted),
    };
    let pr_url_open = pr.url.clone();

    let mut right_side = row().center_y().gap(2.0);
    if pr.ownership == Ownership::Mine && !pr.is_draft && s.page == Page::Inbox {
        right_side = right_side.child(merge_control(s, k, pr));
    }
    let celebrating = s
        .celebrations
        .iter()
        .find(|(id, _)| *id == pr.id)
        .map(|(_, at)| at.elapsed().as_secs_f32());

    let mut metadata = row().center_y().gap(9.0);
    if show_repo {
        metadata = metadata.child(text(pr.repo.clone()).font(k.ty.small).color(c.text_muted));
    }
    metadata = metadata
        .child(
            text(format!("#{}", pr.number))
                .font(k.ty.mono)
                .color(c.text_muted),
        )
        .child(author_badge(s, pr, k))
        .child(text(pr.age_label()).font(k.ty.small).color(c.text_muted))
        .child(el().size(1.0, 10.0).fill(c.border_base))
        .child(text(status).font(k.ty.small_medium).color(tone));
    if let Some(parent) = item.parent {
        metadata = metadata.child(
            text(format!("↳ #{parent}"))
                .font(k.ty.small)
                .color(c.text_muted)
                .help(format!("Stacked on #{parent}")),
        );
    }
    if pr.ci_status == CiStatus::Failing {
        metadata = metadata.child(
            text("CI failing")
                .font(k.ty.small_medium)
                .color(c.danger.fg),
        );
    }
    let mut line = row()
        .key(format!("pr-row-{}", pr.id))
        .w_full()
        .pr(14.0)
        .pl(14.0 + item.depth.min(5) as f32 * 18.0)
        .py(10.0)
        .center_y()
        .gap(12.0)
        .hover(c.hover)
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut App| {
            notify::open_url_in_browser(&pr_url_open, s.allow_os_side_effects);
        })
        .child(
            rune::icon(
                if item.depth == 0 {
                    &PR_ICON
                } else {
                    &STACK_BRANCH
                },
                16.0,
            )
            .color(if pr.is_draft {
                c.text_faint
            } else {
                c.text_muted
            }),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    text(pr.title.clone())
                        .font(TextStyle::new(14.0, 530.0))
                        .color(c.text_base)
                        .truncate(),
                )
                .child(metadata),
        )
        .child(right_side);
    if let Some(t) = celebrating {
        // A wash of mint that fades as the confetti flies.
        line = line.fill(mint(k).alpha((0.2 * (1.0 - t / 0.9)).max(0.0)));
    }
    line
}

/// The Merge button on one of the user's PRs: off until GitHub says it can merge, armed by the
/// first press, and replaced by a celebration once the merge lands.
fn merge_control(s: &App, k: &Kit, pr: &PullRequest) -> El {
    if let Some((_, at)) = s.celebrations.iter().find(|(id, _)| *id == pr.id) {
        return celebration(k, at.elapsed().as_secs_f32());
    }
    let id = pr.id.clone();
    let ready = pr.merge_readiness.is_ready();
    let merging = s.merging.contains(&pr.id);
    let armed = s
        .merge_armed
        .as_ref()
        .is_some_and(|(armed, _)| *armed == pr.id);
    let busy = !s.merging.is_empty();

    let button = if merging {
        Button::new("Merging…")
            .variant(ButtonVariant::Outline)
            .disabled(true)
    } else if armed {
        Button::new("Confirm merge").variant(ButtonVariant::Submit)
    } else {
        Button::new("Merge")
            .variant(if ready {
                ButtonVariant::Contrast
            } else {
                ButtonVariant::Outline
            })
            .disabled(!ready || busy)
    }
    .control_size(ControlSize::Small)
    .on_press(move |s: &mut App| s.press_merge(&id))
    .build(k)
    .key(format!("merge-{}", pr.id));

    row()
        .center_y()
        .help(if armed {
            format!(
                "Press again to {} merge {}#{}",
                pr.merge_method.label(),
                pr.repo,
                pr.number
            )
        } else if ready && pr.merge_readiness == model::MergeReadiness::Unstable {
            format!(
                "Merge with {}. Some checks haven't passed, but none is required",
                pr.merge_method.label()
            )
        } else if ready {
            format!("Merge with {}", pr.merge_method.label())
        } else {
            pr.merge_readiness.blocked_reason().to_owned()
        })
        .child(button)
}

/// "Thumb exercised ✓", fading as the row leaves. It never changes size, so nothing around it moves.
fn celebration(k: &Kit, t: f32) -> El {
    let fade = (1.0 - t / CELEBRATION_SECS).clamp(0.0, 1.0);
    row()
        .key("merge-celebration")
        .size(150.0, 24.0)
        .center()
        .child(
            text("Thumb exercised ✓")
                .font(TextStyle::new(13.0, 700.0))
                .color(mint(k))
                .opacity(fade.sqrt()),
        )
}

/// One light icon button on a repository header, tinted `hue` (oklch degrees).
fn header_action(
    k: &Kit,
    glyph: &'static rune::Icon,
    label: &str,
    key: String,
    hue: f32,
    active: bool,
) -> El {
    let (l, chroma) = match (k.dark, active) {
        (false, false) => (0.66, 0.06),
        (false, true) => (0.5, 0.14),
        (true, false) => (0.62, 0.06),
        (true, true) => (0.8, 0.12),
    };
    row()
        .key(key)
        .role("button")
        .label(label.to_owned())
        .help(label.to_owned())
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .size(24.0, 24.0)
        .radius(5.0)
        .center()
        .hover(k.c.layer_02)
        .child(
            rune::icon(glyph, 15.0)
                .color(oklch(l, chroma, hue))
                .a11y_hidden(),
        )
}

/// Hover actions on a repository header. Idle buttons show while the header is hovered; one
/// that is switched on stays visible.
fn repository_actions(s: &App, k: &Kit, repo: &RepoConfig) -> El {
    let (pin, remove) = (repo.name.clone(), repo.name.clone());
    let confirming = s.confirm_remove.as_deref() == Some(repo.name.as_str());
    let reveal = |el: El, on: bool| if on { el } else { el.group_opacity(0.0, 1.0) };

    row()
        .center_y()
        .gap(8.0)
        .child(reveal(
            header_action(
                k,
                &PIN,
                if repo.pinned {
                    "Unpin repository"
                } else {
                    "Pin repository"
                },
                format!("repo-pin-{}", repo.name),
                70.0,
                repo.pinned,
            )
            .on_press(move |s: &mut App| s.toggle_pin_repo(&pin)),
            repo.pinned,
        ))
        .child(reveal(
            header_action(
                k,
                &TRASH,
                "Remove repository",
                format!("repo-remove-{}", repo.name),
                25.0,
                confirming,
            )
            .on_press(move |s: &mut App| s.confirm_remove = Some(remove.clone())),
            confirming,
        ))
}

fn document_inbox_view(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let ordered = s.ordered_repos();

    let mut doc = col()
        .key("document-column")
        .w_full()
        .max_w(960.0)
        .py(24.0)
        .gap(16.0);
    let attention = s
        .prs
        .iter()
        .filter(|p| {
            p.attention(
                s.draft_stale_hours,
                s.open_stale_hours,
                current_epoch_hour(),
            )
            .requires_action()
        })
        .count();
    doc = doc
        .child(
            col()
                .gap(5.0)
                .child(heading(k, "Pull requests"))
                .child(if s.prs.is_empty() {
                    el()
                } else {
                    text(format!(
                        "{} open  ·  {} need your thumb",
                        s.prs.len(),
                        attention
                    ))
                    .font(k.ty.body)
                    .color(c.text_muted)
                }),
        )
        .child(inbox_tools(s, k));
    if let Some(msg) = &s.status_banner {
        doc = doc.child(
            row()
                .key("toast-status-banner")
                .w_full()
                .px(12.0)
                .py(6.0)
                .radius(6.0)
                .fill(c.layer_01)
                .border(1.0, c.border_weak)
                .center_y()
                .justify_between()
                .child(text(msg.clone()).font(k.ty.small).color(c.text_muted))
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut App| s.status_banner = None)
                        .build(k),
                ),
        );
    }

    if let Some(name) = s.confirm_remove.clone().filter(|_| s.page == Page::Inbox) {
        let confirmed = name.clone();
        doc = doc.child(
            row()
                .key("confirm-remove-repo")
                .w_full()
                .px(12.0)
                .py(6.0)
                .radius(6.0)
                .fill(c.layer_01)
                .border(1.0, c.border_weak)
                .center_y()
                .justify_between()
                .child(
                    text(format!("Remove {name} from your list?"))
                        .font(k.ty.small)
                        .color(c.text_base),
                )
                .child(
                    row()
                        .center_y()
                        .gap(4.0)
                        .child(
                            Button::new("Cancel")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Small)
                                .on_press(|s: &mut App| s.confirm_remove = None)
                                .build(k),
                        )
                        .child(
                            Button::new("Remove")
                                .variant(ButtonVariant::Danger)
                                .control_size(ControlSize::Small)
                                .on_press(move |s: &mut App| s.remove_repo(&confirmed))
                                .build(k)
                                .key("confirm-remove-repo-btn"),
                        ),
                ),
        );
    }

    if s.page == Page::Inbox
        && let SyncStatus::NeedsAuth { reason } = &s.sync_status
    {
        doc = doc.child(setup_row(
            k,
            &format!("GitHub sync unavailable. {reason}"),
            Button::new("Check GitHub")
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut App| {
                    s.page = Page::Onboarding;
                    s.status_banner = None;
                    s.trigger_sync();
                })
                .build(k),
        ));
    }

    let flat = s.filter == InboxFilter::Inbox;
    let groups: Vec<_> = ordered
        .iter()
        .enumerate()
        .filter(|(_, repo)| {
            !flat
                && s.selected_repo
                    .as_ref()
                    .is_none_or(|name| name == &repo.name)
        })
        .filter_map(|(idx, repo)| {
            let prs = s.prs_for_repo(&repo.name);
            // Repos the user is engaged in stay listed even when nothing is open right now.
            let keep_empty = (repo.pinned || repo.activity > 0)
                && s.filter == InboxFilter::All
                && s.search_query.text.trim().is_empty();
            (!prs.is_empty() || keep_empty).then_some((idx, repo, prs))
        })
        .collect();
    let order: Vec<String> = groups.iter().map(|(_, r, _)| r.name.clone()).collect();
    let group_count = groups.len();
    // While a section is dragged every section folds to its header, so all of them share a height.
    let dragging = s.repo_reorder.dragging().is_some();
    let pinned_shown = groups.iter().filter(|(_, r, _)| r.pinned).count();
    for (position, (_, repo, prs)) in groups.iter().enumerate() {
        if position == 0 && pinned_shown > 0 && !dragging {
            doc = doc.child(text("Pinned").font(k.ty.small).color(c.text_faint));
        }
        if position == pinned_shown && pinned_shown > 0 && !dragging {
            doc = doc.child(
                text("Ranked automatically")
                    .font(k.ty.small)
                    .color(c.text_faint),
            );
        }
        let order = order.clone();
        let lifted = s.repo_reorder.dragging() == Some(position);
        let folded = repo.collapsed || dragging;
        let grip = row()
            .key(format!("repo-grip-{}", repo.name))
            .role("button")
            .label(format!("Drag to reorder {}", repo.name))
            .size(20.0, 20.0)
            .center()
            .cursor(if lifted {
                rune::Cursor::Grabbing
            } else {
                rune::Cursor::Grab
            })
            .on_drag(
                move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                    rune::Phase::Down => {}
                    rune::Phase::Move => {
                        tx.animated();
                        s.repo_reorder.follow(
                            position,
                            group_count,
                            d.dy,
                            SECTION_HEIGHT + SECTION_GAP,
                        );
                    }
                    rune::Phase::Up => {
                        if let Some((from, to)) = s.repo_reorder.end(d.cancelled()) {
                            s.drop_repo(&order, from, to);
                        }
                    }
                },
            )
            .child(rune::icon(&GRIP, 14.0).color(c.text_faint).a11y_hidden());
        let repo_name_collapse = repo.name.clone();
        let repo_url = format!("https://github.com/{}", repo.name);

        let section_header = col()
            .key(format!("repo-header-{}", repo.name))
            .w_full()
            .child(
                row()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .px(14.0)
                    .fill(c.layer_01)
                    .hover_group()
                    .center_y()
                    .justify_between()
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        tx.motion_curve(rune::Curve::tween(
                            0.12,
                            rune::Easing::cubic(0.2, 0.0, 0.0, 1.0),
                        ));
                        s.toggle_collapse_repo(&repo_name_collapse)
                    })
                    .child(
                        row()
                            .center_y()
                            .gap(8.0)
                            .child(grip)
                            .child(rune::icon(&REPO_ICON, 15.0).color(c.text_muted))
                            .child(
                                row()
                                    .center_y()
                                    .cursor(rune::Cursor::Pointer)
                                    .help("Open on GitHub")
                                    .on_press(move |s: &mut App| {
                                        notify::open_url_in_browser(
                                            &repo_url,
                                            s.allow_os_side_effects,
                                        )
                                    })
                                    .child(
                                        text(repo.name.clone())
                                            .font(TextStyle::new(13.0, 650.0))
                                            .color(c.text_muted),
                                    ),
                            )
                            .child(if folded {
                                text(model::summarize_prs(prs))
                                    .font(k.ty.small)
                                    .color(c.text_faint)
                            } else {
                                el()
                            }),
                    )
                    .child(if s.page == Page::Inbox {
                        repository_actions(s, k, repo)
                    } else {
                        el()
                    }),
            )
            .child(el().w_full().h(1.0).fill(c.border_weak));

        let mut list = col()
            .w_full()
            .radius(9.0)
            .clip_rounded(9.0)
            .border(1.0, c.border_weak)
            .fill(surface(k))
            .offset(0.0, s.repo_reorder.shift(position))
            .child(section_header);
        if lifted {
            list = list.raise().offset_motion(rune::Anim::INSTANT);
        }
        if folded {
            doc = doc.child(list);
            continue;
        }
        if prs.is_empty() {
            list = list.child(
                text("No open pull requests")
                    .font(k.ty.small)
                    .color(c.text_muted)
                    .padding(12.0),
            );
        }
        for (idx, item) in stacks::rows(prs, &s.prs).iter().enumerate() {
            if idx > 0 {
                list = list.child(el().w_full().h(1.0).fill(c.border_weak));
            }
            list = list.child(minimal_pr_row(s, item, k, false));
        }

        doc = doc.child(list);
    }

    if flat {
        doc = doc.child(flat_inbox(s, k));
    } else if groups.is_empty() {
        doc = doc.child(
            col()
                .key("empty-inbox-state")
                .w_full()
                .py(48.0)
                .center()
                .gap(6.0)
                .child(text("Nothing here.").font(k.ty.body).color(c.text_muted))
                .child(
                    text("Switch filters or clear your search.")
                        .font(k.ty.small)
                        .color(c.text_faint),
                ),
        );
    }

    col()
        .key("inbox-viewport")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(
            // The scroll sits on a column: a row clips its children to its own height.
            col()
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0)
                .min_h(0.0)
                .child(row().w_full().px(28.0).justify_center().child(doc)),
        )
}

/// How many PRs the Inbox tab shows before "Show more".
const INBOX_LIMIT: usize = 100;

/// The Inbox tab: one list of the PRs that matter most right now, ranked by the app.
fn flat_inbox(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let now = current_epoch_hour();
    let mut items: Vec<&PullRequest> = s
        .prs
        .iter()
        .filter(|pr| {
            InboxFilter::Inbox.matches(pr, s.draft_stale_hours, s.open_stale_hours, now)
                && s.matches_search(pr)
        })
        .collect();
    let group = |pr: &PullRequest| {
        pr.attention(s.draft_stale_hours, s.open_stale_hours, now)
            .action_group()
    };
    items.sort_by(|a, b| {
        s.group_rank(group(a).0)
            .cmp(&s.group_rank(group(b).0))
            .then_with(|| a.external.cmp(&b.external))
            .then_with(|| a.idle_hours.cmp(&b.idle_hours))
            .then_with(|| a.id.cmp(&b.id))
    });

    if items.is_empty() {
        return col()
            .key("empty-inbox-state")
            .w_full()
            .py(48.0)
            .center()
            .gap(6.0)
            .child(match &s.logo {
                Some(image) => el().size(72.0, 72.0).pixels(image.pixels()),
                None => el(),
            })
            .child(
                text("Nothing to approve. For now.")
                    .font(k.ty.body)
                    .color(c.text_muted),
            )
            .child(
                text("Mine, Theirs and All show everything else.")
                    .font(k.ty.small)
                    .color(c.text_faint),
            );
    }
    let total = items.len();
    let shown = if s.inbox_show_all {
        total
    } else {
        total.min(INBOX_LIMIT)
    };
    // One section per kind of action, like a repository section: drag to reorder, click to fold.
    let mut section = col().key("flat-inbox").w_full().gap(SECTION_GAP);
    let shown_items = &items[..shown];
    let mut displayed: Vec<(u8, &'static str)> = Vec::new();
    for pr in shown_items {
        let g = group(pr);
        if !displayed.iter().any(|(p, _)| *p == g.0) {
            displayed.push(g);
        }
    }
    let order: Vec<u8> = displayed.iter().map(|(p, _)| *p).collect();
    let group_count = displayed.len();
    let dragging = s.group_reorder.dragging().is_some();
    for (index, (position, heading)) in displayed.iter().copied().enumerate() {
        let in_group = items.iter().filter(|pr| group(pr).0 == position).count();
        let lifted = s.group_reorder.dragging() == Some(index);
        let folded = dragging || s.collapsed_groups.contains(&position);
        let drop_order = order.clone();
        let grip = row()
            .key(format!("group-grip-{position}"))
            .role("button")
            .label(format!("Drag to reorder {heading}"))
            .size(20.0, 20.0)
            .center()
            .cursor(if lifted {
                rune::Cursor::Grabbing
            } else {
                rune::Cursor::Grab
            })
            .on_drag(
                move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                    rune::Phase::Down => {}
                    rune::Phase::Move => {
                        tx.animated();
                        s.group_reorder.follow(
                            index,
                            group_count,
                            d.dy,
                            SECTION_HEIGHT + SECTION_GAP,
                        );
                    }
                    rune::Phase::Up => {
                        if let Some((from, to)) = s.group_reorder.end(d.cancelled()) {
                            s.drop_group(&drop_order, from, to);
                        }
                    }
                },
            )
            .child(rune::icon(&GRIP, 14.0).color(c.text_faint).a11y_hidden());
        let header = col()
            .key(format!("inbox-group-{position}"))
            .w_full()
            .child(
                row()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .px(14.0)
                    .fill(c.layer_01)
                    .center_y()
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        tx.motion_curve(rune::Curve::tween(
                            0.12,
                            rune::Easing::cubic(0.2, 0.0, 0.0, 1.0),
                        ));
                        s.toggle_collapse_group(position)
                    })
                    .child(
                        row()
                            .center_y()
                            .gap(8.0)
                            .child(grip)
                            .child(
                                text(heading)
                                    .font(TextStyle::new(13.0, 650.0))
                                    .color(c.text_muted),
                            )
                            .child(
                                text(in_group.to_string())
                                    .font(k.ty.small)
                                    .color(c.text_faint),
                            ),
                    ),
            )
            .child(el().w_full().h(1.0).fill(c.border_weak));
        let mut list = col()
            .w_full()
            .radius(9.0)
            .clip_rounded(9.0)
            .border(1.0, c.border_weak)
            .fill(surface(k))
            .offset(0.0, s.group_reorder.shift(index))
            .child(header);
        if lifted {
            list = list.raise().offset_motion(rune::Anim::INSTANT);
        }
        if !folded {
            for (idx, pr) in shown_items
                .iter()
                .filter(|pr| group(pr).0 == position)
                .enumerate()
            {
                if idx > 0 {
                    list = list.child(el().w_full().h(1.0).fill(c.border_weak));
                }
                let item = stacks::StackRow {
                    pr,
                    depth: 0,
                    parent: None,
                };
                list = list.child(minimal_pr_row(s, &item, k, true));
            }
        }
        section = section.child(list);
    }
    if total > INBOX_LIMIT {
        section = section.child(
            row().child(
                Button::new(if s.inbox_show_all {
                    "Show less".to_owned()
                } else {
                    format!("Show {} more", total - shown)
                })
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| s.inbox_show_all = !s.inbox_show_all)
                .build(k)
                .key("inbox-show-more"),
            ),
        );
    }
    section
}

/// A switch for one choice about which pull requests the sync brings in.
fn sync_option_row(
    k: &Kit,
    label: &'static str,
    on: bool,
    key: &'static str,
    field: fn(&mut github::SyncOptions) -> &mut bool,
) -> El {
    setup_row(
        k,
        label,
        Switch::new(on)
            .label(label)
            .hide_label()
            .on_toggle(move |s: &mut App| {
                let flag = field(&mut s.sync_options);
                *flag = !*flag;
                let value = *flag;
                if let Ok(db) = Database::open_default() {
                    let _ = db.set_setting(key, if value { "1" } else { "0" });
                }
                s.trigger_sync();
            })
            .build(k)
            .key(format!("option-{key}")),
    )
}

fn settings_content(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let open_idx = match s.open_stale_hours {
        24 => 0,
        48 => 1,
        72 => 2,
        _ => 3,
    };
    let draft_idx = match s.draft_stale_hours {
        48 => 0,
        72 => 1,
        120 => 2,
        _ => 3,
    };

    let thresholds_section = col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(text("Reminders").font(k.ty.ui_medium).color(c.text_base))
                .child(
                    Button::new("Open setup")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut App| {
                            s.settings_open = false;
                            s.page = Page::Onboarding;
                            s.status_banner = None;
                            s.trigger_sync();
                        })
                        .build(k)
                        .key("rerun-onboarding-btn"),
                ),
        )
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text("Nudge my open PRs if unreviewed after")
                        .font(k.ty.body)
                        .color(c.text_muted),
                )
                .child(
                    SegmentedControl::new(&["1d", "2d", "3d", "5d"], Some(open_idx))
                        .label("Open PR Stale")
                        .width(Some(200.0))
                        .on_pick(|s: &mut App, idx, _| {
                            s.open_stale_hours = match idx {
                                0 => 24,
                                1 => 48,
                                2 => 72,
                                _ => 120,
                            };
                            s.persist_thresholds();
                        })
                        .build(k)
                        .key("open-threshold-seg"),
                ),
        )
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text("Nudge my draft PRs if untouched after")
                        .font(k.ty.body)
                        .color(c.text_muted),
                )
                .child(
                    SegmentedControl::new(&["2d", "3d", "5d", "7d"], Some(draft_idx))
                        .label("Draft PR Stale")
                        .width(Some(200.0))
                        .on_pick(|s: &mut App, idx, _| {
                            s.draft_stale_hours = match idx {
                                0 => 48,
                                1 => 72,
                                2 => 120,
                                _ => 168,
                            };
                            s.persist_thresholds();
                        })
                        .build(k)
                        .key("draft-threshold-seg"),
                ),
        );

    let mut rules_list = col().w_full().gap(6.0);
    for rule in &s.rules {
        let rid_toggle = rule.id.clone();
        let rid_del = rule.id.clone();
        rules_list = rules_list.child(
            row()
                .key(format!("rule-row-{}", rule.id))
                .w_full()
                .py(6.0)
                .px(10.0)
                .radius(6.0)
                .fill(c.layer_01)
                .border(1.0, c.border_weak)
                .center_y()
                .justify_between()
                .child(
                    row()
                        .center_y()
                        .gap(10.0)
                        .child(
                            Switch::new(rule.enabled)
                                .on_toggle(move |s: &mut App| s.toggle_rule(&rid_toggle))
                                .build(k),
                        )
                        .child(text(rule.name.clone()).font(k.ty.body).color(c.text_base)),
                )
                .child(
                    Button::new("Remove")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.remove_rule(&rid_del))
                        .build(k),
                ),
        );
    }

    let add_rule_bar = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            TextInput::new(&s.new_rule_name)
                .placeholder("Name")
                .width(Some(180.0))
                .on_edit(|s: &mut App| &mut s.new_rule_name)
                .build(k),
        )
        .child(
            TextInput::new(&s.new_rule_query)
                .placeholder("GitHub search, e.g. repo:owner/name label:urgent")
                .width(Some(340.0))
                .on_edit(|s: &mut App| &mut s.new_rule_query)
                .build(k),
        )
        .child(
            Button::new("Add")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Regular)
                .on_press(|s: &mut App| s.add_custom_rule())
                .build(k)
                .key("add-rule-btn"),
        );

    let rules_section = col()
        .w_full()
        .gap(10.0)
        .child(
            text("Also include pull requests matching")
                .font(k.ty.body)
                .color(c.text_base),
        )
        .child(rules_list)
        .child(add_rule_bar);

    let doc = col()
        .key("settings-column")
        .w_full()
        .max_w(740.0)
        .pb(16.0)
        .gap(12.0)
        .child(
            panel(k).child(default_filter_row(s, k)).child(setup_row(
                k,
                "macOS notifications",
                row().center_y().gap(10.0).child(
                    Switch::new(s.native_notifications)
                        .label("macOS notifications")
                        .hide_label()
                        .on_toggle(|s: &mut App| {
                            s.native_notifications = !s.native_notifications;
                            if s.native_notifications {
                                s.send_welcome_notification();
                            }
                            if let Ok(db) = Database::open_default() {
                                let _ = db.set_setting(
                                    "native_notifications",
                                    if s.native_notifications { "1" } else { "0" },
                                );
                            }
                            s.next_fast_poll_at = 0.0;
                        })
                        .build(k),
                ),
            )),
        )
        .child(
            panel(k)
                .child(
                    text("Pull requests")
                        .font(k.ty.ui_medium)
                        .color(c.text_base),
                )
                .child(sync_option_row(
                    k,
                    "Show archived repositories",
                    !s.sync_options.hide_archived,
                    "hide_archived",
                    |o| &mut o.hide_archived,
                ))
                .child(setup_row(
                    k,
                    "Show PRs active in the last",
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(
                            text("days · 0 = no limit")
                                .font(k.ty.small)
                                .color(c.text_faint),
                        )
                        .child(
                            TextInput::new(&s.max_age_input)
                                .label("Show PRs active in the last (days)")
                                .width(Some(64.0))
                                .on_edit(|s: &mut App| &mut s.max_age_input)
                                .build(k)
                                .key("option-max-age-days"),
                        ),
                ))
                .child(rules_section),
        )
        .child(panel(k).child(thresholds_section));

    col()
        .key("settings-viewport")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(16.0)
        .child(doc)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
}

fn settings_modal(s: &App, k: &Kit) -> El {
    let close = rune_kit::IconButton::new(k.glyph.close)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label("Close settings")
        .help("Close (Esc)")
        .on_press(|s: &mut App| s.settings_open = false)
        .build(k)
        .key("close-settings");
    let dialog = col()
        .key("settings-dialog")
        .role("dialog")
        .label("Settings")
        .w_full()
        .max_w(740.0)
        .h_full()
        .max_h(550.0)
        .min_h(0.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .child(
            row()
                .w_full()
                .padding(16.0)
                .center_y()
                .justify_between()
                .child(
                    text("Settings")
                        .font(TextStyle::new(22.0, 650.0))
                        .color(k.c.text_base),
                )
                .child(close),
        )
        .child(settings_content(s, k))
        .modal(true, |s: &mut App| s.settings_open = false)
        .modal_click_outside(true);
    el().key("settings-overlay")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .padding(24.0)
        .fill(k.c.scrim)
        .center()
        .child(dialog)
}

fn repositories_modal(s: &App, k: &Kit) -> El {
    let close = rune_kit::IconButton::new(k.glyph.close)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label("Close")
        .help("Close (Esc)")
        .on_press(|s: &mut App| s.add_repo_open = false)
        .build(k)
        .key("close-add-repo");
    let dialog = col()
        .key("add-repo-dialog")
        .role("dialog")
        .label("Add repositories")
        .w_full()
        .max_w(560.0)
        .h_full()
        .max_h(560.0)
        .min_h(0.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .child(
            row()
                .w_full()
                .padding(16.0)
                .center_y()
                .justify_between()
                .child(
                    text("Add repositories")
                        .font(TextStyle::new(22.0, 650.0))
                        .color(k.c.text_base),
                )
                .child(close),
        )
        .child(
            col()
                .w_full()
                .px(16.0)
                .pb(16.0)
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(add_repository_panel(s, k, false)),
        )
        .modal(true, |s: &mut App| s.add_repo_open = false)
        .modal_click_outside(true);
    el().key("add-repo-overlay")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .padding(24.0)
        .fill(k.c.scrim)
        .center()
        .child(dialog)
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let mut k = Kit::neutral(cx.system_dark());
    k.c.bg_base = oklch(if k.dark { 0.15 } else { 0.985 }, 0.003, 175.0);
    // The off state of a switch is this color; the stock one vanishes on a white panel.
    k.c.layer_04 = oklch(if k.dark { 0.36 } else { 0.86 }, 0.004, 175.0);

    let body = match s.page {
        Page::Onboarding | Page::Preferences => onboarding_page_view(s, &k),
        Page::Inbox => document_inbox_view(s, &k),
    };

    let root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(top_bar(s, &k))
        .child(body.grow(1.0).min_h(0.0));
    if s.settings_open && s.page == Page::Inbox {
        root.child(settings_modal(s, &k))
    } else if s.add_repo_open && s.page == Page::Inbox {
        root.child(repositories_modal(s, &k))
    } else {
        root
    }
}

fn on_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key == rune::Key::Escape && s.merge_armed.is_some() {
        s.merge_armed = None;
        return true;
    }
    if ev.key == rune::Key::Escape && s.add_repo_open {
        s.add_repo_open = false;
        return true;
    }
    if ev.is_command(',') && s.page == Page::Inbox {
        s.settings_open = !s.settings_open;
        return true;
    }
    if ev.is_command('r') {
        s.sync_now();
        return true;
    }
    false
}

fn seed_preview(s: &mut App) {
    s.loading = false;
    s.sync_status = SyncStatus::Connected {
        username: "octocat".to_string(),
    };
    s.prs = github::demo_prs();
    s.repos = github::infer_repos_from_prs(&s.prs, &[]);
    s.last_synced = Some(std::time::Instant::now());
}

fn main() {
    rune::prewarm_gpu();
    let chrome = Chrome {
        full_size_content: true,
        traffic_lights: Some((16.0, 18.0)),
        drag_regions: true,
    };
    let scenes: &[(&str, fn(&mut App))] = &[
        ("onboarding", |s| {
            s.loading = false;
            s.sync_status = SyncStatus::NeedsAuth {
                reason: "GitHub CLI is signed out.".to_string(),
            };
        }),
        ("signed-in", seed_preview),
        ("preferences", |s| {
            seed_preview(s);
            s.page = Page::Preferences;
        }),
        ("inbox", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
        }),
        ("settings", |s| {
            seed_preview(s);
            s.rules = vec![WatchRule {
                id: "rule-custom-1".into(),
                name: "Release blockers".into(),
                query: "label:release-blocker".into(),
                exclude_drafts: true,
                exclude_mine: true,
                enabled: true,
            }];
            s.page = Page::Inbox;
            s.settings_open = true;
        }),
        ("rules", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.settings_open = true;
        }),
        // The saved data of a real run: point MEAT_PROXY_DB at a copy (scripts/snap-live.sh).
        ("live", |s| {
            s.loading = false;
            s.sync_status = SyncStatus::Connected {
                username: "you".to_string(),
            };
            if let Ok(snap) = Database::open_default().and_then(|db| db.load_snapshot()) {
                s.apply_snapshot(snap);
            }
            s.page = Page::Inbox;
            s.last_synced = Some(std::time::Instant::now());
            s.refresh_ranks();
        }),
        ("empty", |s| {
            seed_preview(s);
            s.prs.clear();
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
        }),
        ("merged", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            if let Some(pr) = s.prs.iter().find(|p| p.id.contains("908")) {
                s.celebrations
                    .push((pr.id.clone(), std::time::Instant::now()));
            }
        }),
        ("action-needed", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
        }),
    ];

    rune::App::new(rune::AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes,
        ..rune::AppSpec::new("meat proxy", (920.0, 680.0), init, view)
    })
    .tab_visits_controls(true)
    .chrome(chrome)
    .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn teammates_sort_ahead_of_outsiders_then_newest_first() {
        let mut s = init();
        s.filter = InboxFilter::All;
        let mut prs = github::demo_prs();
        prs.retain(|p| p.repo == "acme/cli");
        for pr in &mut prs {
            pr.external = false;
            pr.idle_hours = 10;
        }
        // The newest pull request is an outsider's; the oldest is a teammate's.
        let newest = prs.iter().position(|p| p.number == 904).unwrap();
        prs[newest].external = true;
        prs[newest].idle_hours = 1;
        s.prs = prs;
        let order: Vec<u64> = s
            .prs_for_repo("acme/cli")
            .iter()
            .map(|p| p.number)
            .collect();
        assert_eq!(order.last(), Some(&904), "outsiders go last: {order:?}");
    }

    #[test]
    fn unified_inbox_sorts_assigned_and_stale_nudges_first_and_persists_to_sqlite() {
        let db = Database::open_in_memory().unwrap();
        let prs = github::demo_prs();
        db.save_prs(&prs).unwrap();
        db.save_repos(&github::infer_repos_from_prs(&prs, &[]))
            .unwrap();

        let snap = db.load_snapshot().unwrap();
        assert_eq!(snap.prs.len(), 9);
        assert_eq!(snap.repos.len(), 3);

        let pr_418 = snap.prs.iter().find(|p| p.number == 418).unwrap();
        let pr_412 = snap.prs.iter().find(|p| p.number == 412).unwrap();
        let pr_397 = snap.prs.iter().find(|p| p.number == 397).unwrap();
        let pr_421 = snap.prs.iter().find(|p| p.number == 421).unwrap();

        assert_eq!(
            pr_418.attention(72, 48, current_epoch_hour()),
            PrAttention::IncomingAssigned
        );
        assert_eq!(
            pr_412.attention(72, 48, current_epoch_hour()),
            PrAttention::MyOpenNeedsNudge
        );
        assert_eq!(
            pr_397.attention(72, 48, current_epoch_hour()),
            PrAttention::MyDraftNeedsNudge
        );
        assert_eq!(
            pr_421.attention(72, 48, current_epoch_hour()),
            PrAttention::IncomingWatched
        );
    }

    #[test]
    fn starts_on_onboarding_and_confirms_into_flat_document_inbox() {
        let mut state = init();
        state.allow_os_side_effects = false;
        state.preview = true;
        state.loading = false;
        state.sync_status = SyncStatus::NeedsAuth {
            reason: "Signed out".to_string(),
        };
        state.complete_onboarding();
        assert_eq!(state.page, Page::Onboarding);
        assert!(!state.onboarded);

        let mut rt = Runtime::new(state, view, (920.0, 680.0), Env::new(true, 2.0));
        rt.frame(0.0);
        assert!(rt.find("onboard-run-gh-auth").is_some());
        assert!(rt.find("confirm-onboarding-btn").is_none());

        seed_preview(&mut rt.state);
        rt.update(|_, _| {});
        rt.frame(1.0 / 60.0);
        let (x, y) = rt.find("onboard-continue").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0 / 60.0);
        assert_eq!(rt.state.page, Page::Preferences);
        rt.state.default_filter = InboxFilter::Inbox;

        let (x, y) = rt.find("confirm-onboarding-btn").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(3.0 / 60.0);
        assert_eq!(rt.state.page, Page::Inbox);
        assert_eq!(rt.state.filter, InboxFilter::Inbox);
        assert!(rt.find("pr-row-acme/engine#418").is_some());
    }

    fn repo_named(name: &str, rank: usize, pinned: bool) -> RepoConfig {
        RepoConfig {
            name: name.into(),
            manual_rank: rank,
            pinned,
            my_pr_volume: 0,
            assigned_review_volume: 0,
            collapsed: false,
            removed: false,
            activity: 0,
        }
    }

    #[test]
    fn the_inbox_scrolls_with_the_wheel() {
        let mut state = init();
        state.preview = true;
        state.loading = false;
        state.onboarded = true;
        state.page = Page::Inbox;
        state.filter = InboxFilter::All;
        state.sync_status = SyncStatus::Connected {
            username: "octocat".into(),
        };
        let demo = github::demo_prs();
        state.prs = (0..40)
            .map(|n| {
                let mut pr = demo[0].clone();
                pr.id = format!("o/r{}#{n}", n % 4);
                pr.repo = format!("o/r{}", n % 4);
                pr.number = n;
                pr
            })
            .collect();
        state.repos = github::infer_repos_from_prs(&state.prs, &[]);
        let mut rt = Runtime::new(state, view, (920.0, 680.0), Env::new(true, 2.0));
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);
        let before = rt.find("repo-header-o/r3");
        rt.wheel(460.0, 300.0, -300.0);
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        let after = rt.find("repo-header-o/r3");
        assert!(
            after.unwrap().1 < before.unwrap().1 - 100.0,
            "the inbox did not scroll: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn inbox_groups_can_be_dragged_and_folded() {
        let mut state = init();
        state.preview = true;
        assert_eq!(state.group_rank(0), 0);
        // Groups 0, 2 and 4 are on screen; drag the last to the top.
        state.drop_group(&[0, 2, 4], 2, 0);
        assert_eq!(state.group_order, [4, 0, 1, 2, 3, 5, 6]);
        // Drag the first to the bottom of what is shown: it lands after group 2.
        state.drop_group(&[4, 0, 2], 0, 2);
        assert_eq!(state.group_order, [0, 1, 2, 4, 3, 5, 6]);

        state.toggle_collapse_group(2);
        assert_eq!(state.collapsed_groups, [2]);
        state.toggle_collapse_group(2);
        assert!(state.collapsed_groups.is_empty());

        state.filter = InboxFilter::Inbox;
        state.prs = github::demo_prs();
        let shown = state.visible_groups();
        assert!(!shown.is_empty());
        state.toggle_collapse_all();
        assert!(state.all_sections_collapsed());
        assert!(shown.iter().all(|g| state.collapsed_groups.contains(g)));
        state.toggle_collapse_all();
        assert!(state.collapsed_groups.is_empty());
    }

    #[test]
    fn merge_needs_a_ready_pr_and_a_second_press() {
        let mut state = init();
        state.preview = true;
        let mut blocked = github::demo_prs()[1].clone();
        blocked.merge_readiness = model::MergeReadiness::NeedsReview;
        let mut ready = blocked.clone();
        ready.id = "o/r#9".into();
        ready.merge_readiness = model::MergeReadiness::Ready;
        state.prs = vec![blocked.clone(), ready.clone()];

        state.press_merge(&blocked.id);
        assert!(state.merge_armed.is_none(), "a blocked PR can't be armed");
        state.press_merge(&ready.id);
        assert_eq!(
            state.merge_armed.as_ref().map(|(id, _)| id.as_str()),
            Some("o/r#9")
        );
        assert!(state.merging.is_empty(), "one press only arms the button");
    }

    #[test]
    fn old_prs_are_hidden_after_the_limit_and_kept_when_it_is_zero() {
        let mut state = init();
        state.preview = true;
        let demo = github::demo_prs();
        let (fresh, stale) = (demo[0].clone(), {
            let mut pr = demo[1].clone();
            pr.idle_hours = 24 * 40;
            pr
        });
        state.prs = vec![fresh.clone(), stale];
        state.set_max_age_days(30);
        assert_eq!(state.prs, vec![fresh]);

        // Only the repositories on screen count: "o/hidden" has nothing to show.
        let (mut a, mut b) = (repo_named("o/a", 0, false), repo_named("o/b", 1, false));
        (a.activity, b.activity) = (1, 1);
        state.filter = InboxFilter::All;
        state.repos = vec![a, b, repo_named("o/hidden", 2, false)];
        assert!(!state.all_sections_collapsed());
        state.toggle_collapse_all();
        assert!(state.all_sections_collapsed());
        assert!(state.repos[0].collapsed && state.repos[1].collapsed && !state.repos[2].collapsed);
        state.toggle_collapse_all();
        assert!(!state.all_sections_collapsed());
    }

    #[test]
    fn dragging_pins_and_unpins_while_the_rest_stays_automatic() {
        let mut state = init();
        state.preview = true;
        state.repos = vec![
            repo_named("o/a", 0, true),
            repo_named("o/b", 1, true),
            repo_named("o/c", 2, false),
            repo_named("o/d", 3, false),
        ];
        state.rank_scores = vec![("o/c".into(), 5), ("o/d".into(), 50)];
        let names =
            |s: &App| -> Vec<String> { s.ordered_repos().into_iter().map(|r| r.name).collect() };
        assert_eq!(
            names(&state),
            ["o/a", "o/b", "o/d", "o/c"],
            "pinned, then by score"
        );

        // Drag a pinned repo within the pinned zone.
        state.drop_repo(&names(&state), 0, 1);
        assert_eq!(names(&state), ["o/b", "o/a", "o/d", "o/c"]);

        // Drag an automatic repo to the top: it is pinned there.
        state.drop_repo(&names(&state), 3, 0);
        assert_eq!(names(&state), ["o/c", "o/b", "o/a", "o/d"]);

        // Drag a pinned repo below the pinned zone: it is unpinned and ranked again.
        state.drop_repo(&names(&state), 0, 3);
        assert_eq!(names(&state), ["o/b", "o/a", "o/d", "o/c"]);

        // The automatic zone can't be reordered by hand.
        state.drop_repo(&names(&state), 2, 3);
        assert_eq!(names(&state), ["o/b", "o/a", "o/d", "o/c"]);
    }

    #[test]
    fn saves_confirmed_preferences_for_next_launch() {
        let db = Database::open_in_memory().unwrap();
        db.save_onboarding(
            &[],
            &[],
            &[
                ("default_tab", "theirs"),
                ("draft_stale_hours", "120"),
                ("open_stale_hours", "72"),
                ("native_notifications", "0"),
            ],
        )
        .unwrap();

        let snapshot = db.load_snapshot().unwrap();
        assert!(snapshot.onboarded);
        assert_eq!(snapshot.default_filter, InboxFilter::Theirs);
        assert_eq!(snapshot.draft_stale_hours, 120);
        assert_eq!(snapshot.open_stale_hours, 72);
        assert!(!snapshot.native_notifications);
    }
}
