use rune::{
    Chrome, Color, Cx, El, KeyEvent, Latest, ScrollSpec, TextEdit, TextStyle, Tx, col, el, oklch,
    row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Kit, Markdown, MarkdownStyle, SegmentedControl, Switch,
    TextInput,
};

mod autonomy;
mod avatars;
mod db;
mod github;
mod judge;
mod model;
mod notify;
mod pace;
mod reorder;
mod sections;
mod stacks;

use db::Database;
use github::{SyncOutput, SyncStatus};
use model::{
    Blocker, CiStatus, InboxFilter, NotificationEvent, Ownership, PrAttention, PullRequest,
    RepoConfig, WatchRule,
};

/// How long a pressed Merge button waits for its confirming press.
const ARMED_SECS: f32 = 4.0;
/// How long a row stays washed in its button's colour after an approve or merge lands.
const INK_WASH_SECS: f32 = 1.4;
/// How long a merged PR's row celebrates before it leaves the list.
const CELEBRATION_SECS: f32 = 1.9;
/// A pull request that just arrived slides in over this long, and its row stays tinted for the longer one.
const ARRIVAL_SLIDE_SECS: f32 = 0.45;
const ARRIVAL_GLOW_SECS: f32 = 2.6;
/// Gap between rows arriving together, so a batch cascades.
const ARRIVAL_STAGGER_SECS: f32 = 0.07;

/// The first-launch intro: the button springs in, a finger presses it, the name appears, then the
/// setup screen fades in. A click or Escape skips it; Reduce Motion never plays it.
const INTRO_SECS: f32 = 5.0;
/// How long typing in Settings pauses before what was typed applies.
const TYPING_SETTLES: std::time::Duration = std::time::Duration::from_millis(1500);
/// When the finger reaches the button.
const INTRO_HIT: f32 = 1.45;

/// The section ignored pull requests move to, always last and folded until opened.
const IGNORED_GROUP: u8 = 255;
/// How long after the notifications feed announced a review request the sync keeps quiet about
/// the same request, which it sees as a second round.
const HEARD_SECS: u64 = 300;
/// How long a Copy prompt button says "Copied".
const COPIED_SECS: f32 = 1.6;
/// How long a row says it was quick (or bold), and the Pace panel's number for it pulses.
const PACE_FLASH_SECS: f32 = 3.2;

/// What a button can do to clear what blocks a pull request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unblock {
    /// Bring the branch up to date with its base.
    UpdateBranch,
    /// Run the failed GitHub Actions jobs again.
    Rerun,
}

fn current_epoch_hour() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64 / 3600
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Onboarding,
    Inbox,
}

pub struct App {
    avatars: avatars::Avatars,
    settings_open: bool,
    logo: Option<rune::Image>,
    /// The intro's button base, button face and finger.
    intro_art: Vec<rune::Image>,
    /// When the first-launch intro started; cleared once it ends or is skipped.
    pub intro: Option<std::time::Instant>,
    /// What `gh auth login` has printed so far, shown on the setup screen.
    auth_lines: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    auth_lines_shown: usize,
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
    /// Rows opened to show the description.
    pub expanded: std::collections::HashSet<String>,
    /// The PR whose Approve button was pressed once and waits for a second press.
    pub approve_armed: Option<(String, std::time::Instant)>,
    pub approving: Vec<String>,
    /// Approved from here, and when; the button stays done until a sync moves the row.
    pub approved: Vec<(String, std::time::Instant)>,
    pub approve_worker: Latest<(String, Result<(), String>)>,
    /// Updating a branch or running failed checks again: the pull request, which, and how it went.
    pub unblock_worker: Latest<(String, Unblock, Result<(), String>)>,
    pub unblocking: Vec<String>,
    /// Review requests the notifications feed announced, by URL, and when.
    heard: Vec<(String, std::time::Instant)>,
    /// The pull request whose agent prompt was just copied, and when: its button says so briefly.
    pub copied: Option<(String, std::time::Instant)>,
    /// PRs just merged, with the moment they were: the row celebrates, then leaves the list.
    pub celebrations: Vec<(String, std::time::Instant)>,
    /// Pull requests a sync just brought in, with when each starts to appear.
    pub arrivals: Vec<(String, std::time::Instant)>,
    pub merge_worker: Latest<(String, Result<(), String>)>,
    /// Turning auto-merge on or off: the pull request, which, and how it went.
    pub auto_merge_worker: Latest<(String, bool, Result<(), String>)>,
    pub auto_merging: Vec<String>,
    /// Pull requests with auto-merge on, and the ones GitHub merged by itself (shown faded).
    pub tracked: Vec<model::Tracked>,
    /// Asks GitHub what became of tracked pull requests that left the lists.
    pub resolve_worker: Latest<Vec<(String, Result<String, String>)>>,
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
    /// The saved data, opened on first use and kept open; see `App::db`.
    db: std::cell::OnceCell<Database>,
    /// The saved data could not be read at launch. Setup must not run over it then: finishing
    /// setup replaces the saved pull requests.
    pub boot_failed: bool,
    pub boot_worker: Latest<Result<db::AppSnapshot, rusqlite::Error>>,
    pub sync_worker: Latest<Result<SyncOutput, String>>,
    /// A sync was asked for while one ran; it starts when that one lands.
    pub sync_again: bool,
    pub judge: Option<judge::JudgeConfig>,
    /// Sort with the model's verdicts (true) or by rules alone (false); unset follows what exists.
    pub smart_sort: Option<bool>,
    /// The Inbox sections (built in).
    pub layout: sections::Layout,
    /// Hide outside contributors' pull requests from the Inbox tab.
    pub hide_external: bool,
    /// List requests made of a team you are in, not of you by name, in the Inbox too.
    pub show_team_reviews: bool,
    /// Whether the decision model may be asked at all. Off until chosen: turning it on sends the
    /// description and changed file paths of review requests, private repositories included.
    pub ai_enabled: bool,
    /// The experimental AI feature is visible at all (`MEAT_PROXY_AI=1`).
    pub ai_feature: bool,
    pub ai_url: TextEdit,
    pub ai_model: TextEdit,
    pub ai_key: TextEdit,
    /// The three AI fields as last applied, to notice an edit.
    ai_applied: (String, String, String),
    /// What the typed settings (the age limit and the AI fields) held when last seen, and since
    /// when: they apply once the typing stops, not on each keystroke, so "14" never applies "1"
    /// and a half-typed address never gets the key.
    typed: Option<(String, std::time::Instant)>,
    /// Pull requests moved to the Ignored section, and whether that section is open.
    pub ignored: std::collections::HashSet<String>,
    pub ignored_open: bool,
    pub judgments: std::collections::HashMap<String, judge::Judgment>,
    pub judge_worker: Latest<judge::Batch>,
    /// Spent on the model in this run of the app, against the config's cap.
    pub judge_spent: f64,
    /// An error that would repeat (bad key, no credit): no more asking until the app restarts.
    pub judge_off: bool,
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
    /// Settings › Autonomous: what meat proxy does on GitHub by itself, on the user's own PRs.
    pub autonomy_branches: bool,
    pub autonomy_reviewers: bool,
    /// Actions already taken (`autonomy::key`) with when, so each happens once per commit.
    pub autonomy_done: std::collections::HashMap<String, i64>,
    pub autonomy_worker: Latest<Vec<autonomy::Outcome>>,
    /// Pace: the score of quick approvals and merges, on unless switched off in Settings; its
    /// panel above the Inbox, and whether it is folded.
    pub pace_enabled: bool,
    pub pace_collapsed: bool,
    pub pace_events: Vec<pace::Event>,
    /// What was just counted, and when: the row says so, and the panel's number pulses.
    pub pace_flash: Vec<(pace::Event, std::time::Instant)>,
}

fn init() -> App {
    let headless = !cfg!(test) && rune::is_headless();
    let headless_or_silent = std::env::var("MEAT_PROXY_SILENT_OS_NOTIFY").is_ok() || headless;

    App {
        avatars: avatars::Avatars::default(),
        settings_open: false,
        logo: None,
        intro_art: Vec::new(),
        intro: None,
        auth_lines: Default::default(),
        auth_lines_shown: 0,
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
        expanded: std::collections::HashSet::new(),
        approve_armed: None,
        approving: Vec::new(),
        approved: Vec::new(),
        approve_worker: Latest::default(),
        celebrations: Vec::new(),
        arrivals: Vec::new(),
        merge_worker: Latest::default(),
        auto_merge_worker: Latest::default(),
        auto_merging: Vec::new(),
        tracked: Vec::new(),
        resolve_worker: Latest::default(),
        last_synced: None,
        sync_label_shown: String::new(),
        group_order: (0..sections::Layout::default().sections.len() as u8).collect(),
        collapsed_groups: Vec::new(),
        group_reorder: reorder::Reorder::new(),
        max_age_input: TextEdit::new(github::DEFAULT_MAX_AGE_DAYS.to_string()),
        filter: InboxFilter::Inbox,
        selected_repo: None,
        draft_stale_hours: 72,
        open_stale_hours: 48,
        onboarded: false,
        default_filter: InboxFilter::Inbox,
        native_notifications: false,
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
        db: std::cell::OnceCell::new(),
        boot_failed: false,
        boot_worker: Latest::default(),
        sync_worker: Latest::default(),
        sync_again: false,
        judge: None,
        smart_sort: None,
        layout: sections::Layout::default(),
        hide_external: false,
        show_team_reviews: false,
        ai_enabled: false,
        ai_feature: judge::feature_enabled(),
        ai_url: TextEdit::new(judge::DECISIONS_URL),
        ai_model: TextEdit::new(judge::DECISIONS_MODEL),
        ai_key: TextEdit::new(""),
        ai_applied: (
            judge::DECISIONS_URL.into(),
            judge::DECISIONS_MODEL.into(),
            String::new(),
        ),
        typed: None,
        ignored: std::collections::HashSet::new(),
        ignored_open: false,
        judgments: std::collections::HashMap::new(),
        judge_worker: Latest::default(),
        judge_spent: 0.0,
        judge_off: false,
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
        unblock_worker: Latest::default(),
        unblocking: Vec::new(),
        heard: Vec::new(),
        copied: None,
        autonomy_branches: false,
        autonomy_reviewers: false,
        autonomy_done: std::collections::HashMap::new(),
        autonomy_worker: Latest::default(),
        pace_enabled: true,
        pace_collapsed: false,
        pace_events: Vec::new(),
        pace_flash: Vec::new(),
    }
}

impl App {
    /// The saved data, for writing what the user changed. None in a preview, which never saves,
    /// and while the database cannot be opened (tried again on the next write).
    fn db(&self) -> Option<&Database> {
        if self.preview {
            return None;
        }
        if let Some(db) = self.db.get() {
            return Some(db);
        }
        match Database::open_default() {
            Ok(db) => Some(self.db.get_or_init(|| db)),
            Err(error) => {
                eprintln!("meat-proxy: could not open the saved data: {error}");
                None
            }
        }
    }

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
        // A saved order and fold state belong to the sections they were made for.
        let same_layout = snap.layout_key.as_deref() == Some(self.layout.fingerprint.as_str());
        self.group_order = if same_layout {
            snap.group_order
        } else {
            Vec::new()
        };
        self.collapsed_groups = if same_layout {
            snap.collapsed_groups
        } else {
            Vec::new()
        };
        if !same_layout
            && let Some(db) = self.db()
        {
            let _ = db.set_setting("inbox_layout", &self.layout.fingerprint);
        }
        self.normalize_group_order();
        self.max_age_input = TextEdit::new(self.sync_options.max_age_days.to_string());
        self.last_notif_modified = snap.last_notif_modified;
        self.organization = TextEdit::new(&snap.organization);
        self.judgments = snap.judgments;
        self.smart_sort = snap.smart_sort;
        self.hide_external = snap.hide_external;
        self.show_team_reviews = snap.show_team_reviews;
        self.ai_enabled = snap.ai_enabled;
        self.autonomy_branches = snap.autonomy_branches;
        self.autonomy_reviewers = snap.autonomy_reviewers;
        self.autonomy_done = snap.autonomy_done;
        self.pace_enabled = snap.pace_enabled;
        self.pace_collapsed = snap.pace_collapsed;
        self.pace_events = snap.pace_events;
        self.ignored = snap.ignored;
        self.tracked = snap.tracked;
        self.drop_removed_prs();
        // Pull requests saved before the limit was set, or under another one, do not slip through.
        let days = self.sync_options.max_age_days;
        self.prs.retain(|pr| pr.within_days(days));
        if self.onboarded {
            self.page = Page::Inbox;
        } else {
            // Notifications are something to opt in to on the setup screen.
            self.native_notifications = false;
        }
    }

    /// Remember which pull requests a sync brought that the list did not have, so they can slide
    /// in. Nothing arrives on the first fill of an empty list.
    fn note_arrivals(&mut self, incoming: &[PullRequest]) {
        if self.prs.is_empty() || self.preview {
            return;
        }
        let now = std::time::Instant::now();
        let fresh = incoming
            .iter()
            .filter(|pr| !self.prs.iter().any(|known| known.id == pr.id))
            .take(12);
        for (n, pr) in fresh.enumerate() {
            let at = now + std::time::Duration::from_secs_f32(n as f32 * ARRIVAL_STAGGER_SECS);
            self.arrivals.push((pr.id.clone(), at));
        }
    }

    /// Seconds since this pull request started to arrive, while it is still arriving.
    fn arrival_age(&self, pr_id: &str) -> Option<f32> {
        let (_, at) = self.arrivals.iter().find(|(id, _)| id == pr_id)?;
        let age = std::time::Instant::now()
            .saturating_duration_since(*at)
            .as_secs_f32();
        (age < ARRIVAL_GLOW_SECS).then_some(age)
    }

    /// What a judgment says about this pull request, if one was made.
    pub fn judgment(&self, pr: &PullRequest) -> Option<&judge::Judgment> {
        self.judgments.get(&pr.id)
    }

    /// Whether a model has anything to say here: a key is set, or verdicts were saved earlier.
    pub fn smart_available(&self) -> bool {
        self.ai_feature
            && self.ai_enabled
            && (self.judge.is_some() || !self.judgments.is_empty())
            && !self.layout.needed_questions().is_empty()
    }

    /// Sorting with the model's verdicts. The user's choice wins; without one, the model is used
    /// when it is there.
    pub fn smart_active(&self) -> bool {
        self.smart_sort.unwrap_or_else(|| self.smart_available()) && self.smart_available()
    }

    pub fn toggle_smart_sort(&mut self) {
        if !self.smart_available() {
            return;
        }
        let next = !self.smart_active();
        self.smart_sort = Some(next);
        self.save_flag("smart_sort", next);
    }

    /// What the sorting goes by: with AI, the model's answers or a rule's; with rules alone, only
    /// what a rule can say (bots, tiny changes), whether or not it was saved.
    fn answers_for(&self, pr: &PullRequest) -> std::collections::HashMap<String, f32> {
        match self.judgment(pr) {
            Some(j) if self.smart_active() || j.by == "rules" => j.answers.clone(),
            _ => judge::rule_judgment(pr)
                .map(|j| j.answers)
                .unwrap_or_default(),
        }
    }

    /// The default order of the sections on screen: the order they are listed in.
    fn default_group_order(&self) -> Vec<u8> {
        (0..self.layout.sections.len() as u8).collect()
    }

    /// Move a pull request to Ignored, or back.
    pub fn toggle_ignore(&mut self, pr_id: &str) {
        let now_ignored = !self.ignored.contains(pr_id);
        if now_ignored {
            self.ignored.insert(pr_id.to_owned());
        } else {
            self.ignored.remove(pr_id);
        }
        if let Some(db) = self.db() {
            let _ = db.set_ignored(pr_id, now_ignored);
        }
    }

    /// A notification about something the user ignored is not worth showing.
    fn is_ignored_url(&self, url: &str) -> bool {
        url.strip_prefix("https://github.com/")
            .and_then(|rest| rest.split_once("/pull/"))
            .is_some_and(|(repo, number)| {
                let number = number.split(['/', '#', '?']).next().unwrap_or_default();
                self.ignored.contains(&format!("{repo}#{number}"))
            })
    }

    pub fn toggle_show_team_reviews(&mut self) {
        self.show_team_reviews = !self.show_team_reviews;
        self.save_flag("show_team_reviews", self.show_team_reviews);
    }

    fn set_ai_fields(&mut self, url: &str, model: &str, key: &str) {
        self.ai_url = TextEdit::new(url);
        self.ai_model = TextEdit::new(model);
        self.ai_key = TextEdit::new(key);
        self.ai_applied = (url.into(), model.into(), key.into());
    }

    /// Applies the AI fields once they differ from what is in effect: saves `judge.json` and asks
    /// again for anything the model had not been able to judge.
    fn apply_ai_fields(&mut self) -> bool {
        let now = (
            self.ai_url.text.trim().to_owned(),
            self.ai_model.text.trim().to_owned(),
            self.ai_key.text.trim().to_owned(),
        );
        if now == self.ai_applied || now.0.is_empty() || now.1.is_empty() {
            return false;
        }
        let cfg = judge::JudgeConfig::from_fields(&now.0, &now.1, &now.2, self.judge.as_ref());
        if !self.preview && self.allow_os_side_effects {
            let _ = cfg.save();
        }
        self.ai_applied = now;
        self.judge = cfg.usable().then_some(cfg);
        self.judge_off = false;
        self.queue_judgments();
        true
    }

    pub fn toggle_ai(&mut self) {
        self.ai_enabled = !self.ai_enabled;
        self.save_flag("ai_enabled", self.ai_enabled);
        // Review requests become (or stop being) candidates: ask again on the next sync.
        self.queue_judgments();
    }

    pub fn toggle_hide_external(&mut self) {
        self.hide_external = !self.hide_external;
        self.save_flag("hide_external", self.hide_external);
    }

    /// Flip one Autonomous switch (`true` for branches, `false` for reviewers) and act right away.
    pub fn toggle_autonomy(&mut self, branches: bool) {
        let (flag, key) = if branches {
            (&mut self.autonomy_branches, "autonomy_branches")
        } else {
            (&mut self.autonomy_reviewers, "autonomy_reviewers")
        };
        *flag = !*flag;
        let value = *flag;
        self.save_flag(key, value);
        self.run_autonomy();
    }

    fn save_flag(&self, key: &str, value: bool) {
        if let Some(db) = self.db() {
            let _ = db.set_setting(key, if value { "1" } else { "0" });
        }
    }

    pub fn toggle_pace(&mut self) {
        self.pace_enabled = !self.pace_enabled;
        self.pace_flash.clear();
        self.save_flag("pace_enabled", self.pace_enabled);
    }

    pub fn toggle_pace_collapsed(&mut self) {
        self.pace_collapsed = !self.pace_collapsed;
        self.save_flag("pace_collapsed", self.pace_collapsed);
    }

    /// Count an approval or merge that just landed from here. Nothing is counted while Pace is off.
    pub fn record_pace(&mut self, kind: pace::Kind, pr_id: &str) {
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        if !self.pace_enabled {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let event = match kind {
            pace::Kind::Approve => pace::approval(pr, now),
            pace::Kind::Merge => pace::merge(pr, now),
        };
        if let Some(db) = self.db() {
            let _ = db.add_pace_event(&event);
        }
        self.pace_events.push(event.clone());
        self.pace_flash.push((event, std::time::Instant::now()));
    }

    /// What was just counted for this pull request, and how long ago.
    pub fn pace_flash_of(&self, pr_id: &str) -> Option<(&pace::Event, f32)> {
        self.pace_flash
            .iter()
            .rev()
            .find(|(e, _)| e.pr_id == pr_id)
            .map(|(e, at)| (e, at.elapsed().as_secs_f32()))
    }

    /// Start whatever the Autonomous switches call for on the pull requests as last synced.
    pub fn run_autonomy(&mut self) {
        if self.preview
            || !self.onboarded
            || !self.allow_os_side_effects
            || self.autonomy_worker.is_pending()
        {
            return;
        }
        let planned = autonomy::plan(
            &self.prs,
            self.autonomy_branches,
            self.autonomy_reviewers,
            &self.autonomy_done,
        );
        if planned.is_empty() {
            return;
        }
        let jobs: Vec<(model::PullRequest, autonomy::Action)> = planned
            .into_iter()
            .filter_map(|(id, action)| {
                Some((self.prs.iter().find(|p| p.id == id)?.clone(), action))
            })
            .collect();
        self.autonomy_worker.request((), move |()| {
            jobs.into_iter()
                .map(|(pr, action)| {
                    let result = autonomy::perform(&pr, &action);
                    (pr, action, result)
                })
                .collect()
        });
    }

    pub fn group_order_is_default(&self) -> bool {
        self.group_order == self.default_group_order()
    }

    /// Put the Inbox groups back in the order of the configuration.
    pub fn reset_group_order(&mut self) {
        self.group_order = self.default_group_order();
        self.persist_groups();
    }

    /// The section a pull request belongs to: the first whose conditions all hold. None when it
    /// belongs to none, which keeps it out of the Inbox.
    fn action_group_of(&self, pr: &PullRequest, attention: PrAttention) -> Option<(u8, &str)> {
        if self.ignored.contains(&pr.id) {
            return Some((IGNORED_GROUP, "Ignored"));
        }
        let answers = self.answers_for(pr);
        let at = self.layout.section_of(&sections::Subject {
            pr,
            attention,
            answers: &answers,
        })?;
        Some((at as u8, self.layout.sections[at].name.as_str()))
    }

    /// Whether the switches in Settings keep this one out of the Inbox tab: outside contributors,
    /// and requests made of a team rather than of you.
    fn hidden_from_inbox(&self, pr: &PullRequest, now: u64) -> bool {
        (self.hide_external && pr.external)
            || (!self.show_team_reviews
                && pr.attention(self.draft_stale_hours, self.open_stale_hours, now)
                    == PrAttention::IncomingViaTeam)
    }

    /// What the Inbox is about, whatever is typed in the search box.
    fn needs_you(&self, pr: &PullRequest, now: u64) -> bool {
        !self.ignored.contains(&pr.id)
            && InboxFilter::Inbox.matches(pr, self.draft_stale_hours, self.open_stale_hours, now)
            && !self.hidden_from_inbox(pr, now)
    }

    /// Whether the Inbox tab lists it: it asks something of you, and nothing hides it.
    fn in_inbox(&self, pr: &PullRequest, now: u64) -> bool {
        // An ignored pull request is listed, in its own section, while it would still be there.
        let listed = self.needs_you(pr, now)
            || (self.ignored.contains(&pr.id)
                && InboxFilter::Inbox.matches(
                    pr,
                    self.draft_stale_hours,
                    self.open_stale_hours,
                    now,
                ));
        listed && self.matches_search(pr)
    }

    /// After a sync: answer what cheap rules can, and send the model only what is left.
    pub fn queue_judgments(&mut self) {
        let Some(cfg) = self.judge.clone() else {
            return;
        };
        if !self.ai_feature
            || !self.ai_enabled
            || self.judge_off
            || self.preview
            || self.judge_worker.is_pending()
        {
            return;
        }
        let (draft, open, now) = (
            self.draft_stale_hours,
            self.open_stale_hours,
            current_epoch_hour(),
        );
        // Only the questions the sections use are asked; with none, no model is called at all.
        let questions = self.layout.needed_questions();
        let plan = judge::plan(
            &self.prs,
            |pr| pr.attention(draft, open, now),
            &self.judgments,
            &cfg,
            &questions,
        );
        let settled = plan.settled;
        if !settled.is_empty() {
            self.store_judgments(settled);
        }
        if plan.to_ask.is_empty() || self.judge_spent >= cfg.max_spend_usd {
            return;
        }
        let spent = self.judge_spent;
        self.judge_worker.request(plan.to_ask, move |prs| {
            judge::run_batch(&cfg, &questions, prs, spent)
        });
    }

    fn store_judgments(&mut self, judged: Vec<(String, judge::Judgment)>) {
        if let Some(db) = self.db() {
            let live: Vec<String> = self.prs.iter().map(|p| p.id.clone()).collect();
            let _ = db.save_judgments(&judged, &live);
        }
        self.judgments.extend(judged);
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
        let default = self.default_group_order();
        for g in self.group_order.iter().chain(default.iter()) {
            if default.contains(g) && !order.contains(g) {
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
        if let Some(db) = self.db() {
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
        let (id, url, method, head) =
            (pr.id.clone(), pr.url.clone(), pr.merge_method, pr.head_oid.clone());
        self.merge_worker
            .request((), move |()| (id, github::merge_pr(&url, method, &head)));
    }

    pub fn toggle_expanded(&mut self, pr_id: &str) {
        if !self.expanded.remove(pr_id) {
            self.expanded.insert(pr_id.to_owned());
        }
    }

    /// First press arms the button, the second approves. An approval can't be taken back.
    pub fn press_approve(&mut self, pr_id: &str) {
        if !self.approving.is_empty() || self.approved.iter().any(|(id, _)| id == pr_id) {
            return;
        }
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        let armed = self
            .approve_armed
            .as_ref()
            .is_some_and(|(id, _)| id == pr_id);
        if !armed {
            self.approve_armed = Some((pr_id.to_owned(), std::time::Instant::now()));
            return;
        }
        self.approve_armed = None;
        if self.preview {
            return;
        }
        self.approving.push(pr_id.to_owned());
        let (id, repo, number, head) =
            (pr.id.clone(), pr.repo.clone(), pr.number, pr.head_oid.clone());
        self.approve_worker
            .request((), move |()| (id, github::approve_pr(&repo, number, &head)));
    }

    /// Clear what blocks a pull request. One press is enough, unlike Merge and Approve: both
    /// actions only start the checks over, and either can be done again.
    pub fn press_unblock(&mut self, pr_id: &str, what: Unblock) {
        if self.preview || !self.unblocking.is_empty() {
            return;
        }
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        let id = pr.id.clone();
        match what {
            Unblock::UpdateBranch if pr.can_update_branch => {
                let (repo, number, head) = (pr.repo.clone(), pr.number, pr.head_oid.clone());
                self.unblock_worker.request((), move |()| {
                    (id, what, github::update_branch(&repo, number, &head))
                });
            }
            Unblock::Rerun if !pr.rerunnable().is_empty() => {
                let runs = pr.rerunnable();
                self.unblock_worker
                    .request((), move |()| (id, what, github::rerun_failed(&runs)));
            }
            _ => return,
        }
        self.unblocking.push(pr_id.to_owned());
    }

    /// What pressing the auto-merge button does: Some(true) turns it on, Some(false) turns it off,
    /// None when it is not on offer (the pull request can be merged now, or GitHub does not allow it).
    pub fn auto_merge_target(&self, pr_id: &str) -> Option<bool> {
        let pr = self.prs.iter().find(|p| p.id == pr_id)?;
        match pr.auto_merge {
            model::AutoMerge::On => Some(false),
            model::AutoMerge::Available if !pr.merge_readiness.is_ready() => Some(true),
            _ => None,
        }
    }

    pub fn press_auto_merge(&mut self, pr_id: &str) {
        let Some(enable) = self.auto_merge_target(pr_id) else {
            return;
        };
        if self.preview || !self.auto_merging.is_empty() {
            return;
        }
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        self.auto_merging.push(pr_id.to_owned());
        let (id, url, method, head) =
            (pr.id.clone(), pr.url.clone(), pr.merge_method, pr.head_oid.clone());
        self.auto_merge_worker.request((), move |()| {
            (id, enable, github::set_auto_merge(&url, method, enable, &head))
        });
    }

    fn save_tracked_row(&self, t: &model::Tracked) {
        if let Some(db) = self.db() {
            let _ = db.save_tracked(t);
        }
    }

    /// Remove the faded row of a pull request that auto-merge finished.
    pub fn clear_tracked(&mut self, pr_id: &str) {
        self.tracked.retain(|t| t.pr.id != pr_id);
        if let Some(db) = self.db() {
            let _ = db.delete_tracked(pr_id);
        }
    }

    /// After a sync: remember pull requests that have auto-merge on, forget the ones it was turned
    /// off for, and ask GitHub what became of the ones that left the lists.
    pub fn after_sync_track(&mut self) {
        let (draft, open, now) = (
            self.draft_stale_hours,
            self.open_stale_hours,
            current_epoch_hour(),
        );
        let live: Vec<model::PullRequest> = self.prs.clone();
        for pr in &live {
            let on = pr.ownership == Ownership::Mine && pr.auto_merge == model::AutoMerge::On;
            let known = self.tracked.iter().position(|t| t.pr.id == pr.id);
            match (on, known) {
                (true, Some(i)) => {
                    self.tracked[i].pr = pr.clone();
                    self.tracked[i].state = model::TrackedState::Watching;
                }
                (true, None) => {
                    let section = self
                        .action_group_of(pr, pr.attention(draft, open, now))
                        .map_or_else(|| "Merge these".to_owned(), |(_, name)| name.to_owned());
                    self.tracked.push(model::Tracked {
                        pr: pr.clone(),
                        section,
                        state: model::TrackedState::Watching,
                    });
                }
                (false, Some(i)) if self.tracked[i].state == model::TrackedState::Watching => {
                    // Auto-merge was turned off: nothing to follow any more.
                    let id = self.tracked.remove(i).pr.id;
                    if let Some(db) = self.db() {
                        let _ = db.delete_tracked(&id);
                    }
                    continue;
                }
                _ => continue,
            }
            if let Some(t) = self.tracked.iter().find(|t| t.pr.id == pr.id) {
                self.save_tracked_row(t);
            }
        }
        if self.resolve_worker.is_pending() || self.preview {
            return;
        }
        let gone: Vec<(String, String)> = self
            .tracked
            .iter()
            .filter(|t| {
                t.state == model::TrackedState::Watching
                    && !self.prs.iter().any(|p| p.id == t.pr.id)
            })
            .map(|t| (t.pr.id.clone(), t.pr.url.clone()))
            .collect();
        if !gone.is_empty() {
            self.resolve_worker.request(gone, |list| {
                list.into_iter()
                    .map(|(id, url)| {
                        let state = github::pr_state(&url);
                        (id, state)
                    })
                    .collect()
            });
        }
    }

    pub fn set_max_age_days(&mut self, days: u32) {
        self.sync_options.max_age_days = days;
        if days > 0 {
            self.prs.retain(|pr| pr.within_days(days));
        }
        if let Some(db) = self.db() {
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
            .filter(|pr| self.in_inbox(pr, now))
            .filter_map(|pr| {
                self.action_group_of(
                    pr,
                    pr.attention(self.draft_stale_hours, self.open_stale_hours, now),
                )
                .map(|(id, _)| id)
            })
            .collect();
        groups.sort_unstable();
        groups.dedup();
        // Ignored folds by its own rule and does not count toward "everything is folded".
        groups.retain(|g| *g != IGNORED_GROUP);
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

    /// A drag ended: `order` is the displayed repositories, pinned first, and the dragged one
    /// moved from `from` to `to`. Dropping among the pinned ones pins it there; dragging a pinned
    /// one below them unpins it. The automatic section can't be reordered by hand.
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

    /// Starts a sync, or, while one runs, one more after it: a new request would drop the running
    /// one's answer, and a busy minute of approvals would never refresh the list.
    pub fn trigger_sync(&mut self) {
        if self.loading || self.preview {
            return;
        }
        if self.sync_worker.is_pending() {
            self.sync_again = true;
            return;
        }
        self.sync_again = false;
        let repos = self.repos.clone();
        let rules = self.rules.clone();
        let options = self.sync_options;
        self.sync_worker
            .request((), move |()| github::fetch_live(&repos, &rules, options));
    }

    pub fn persist_repos(&self) {
        if self.onboarded
            && self.page == Page::Inbox
            && let Some(db) = self.db()
        {
            let _ = db.save_repos(&self.repos);
        }
    }

    pub fn persist_thresholds(&self) {
        if self.onboarded
            && self.settings_open
            && let Some(db) = self.db()
        {
            let _ = db.set_setting("draft_stale_hours", &self.draft_stale_hours.to_string());
            let _ = db.set_setting("open_stale_hours", &self.open_stale_hours.to_string());
        }
    }

    pub fn complete_onboarding(&mut self) {
        if self.boot_failed {
            return;
        }
        if !matches!(self.sync_status, SyncStatus::Connected { .. }) {
            self.page = Page::Onboarding;
            return;
        }
        if !self.preview {
            let saved = self.db().ok_or("the saved data could not be opened".to_owned()).and_then(|db| {
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
                .map_err(|e| e.to_string())
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
        if let Some(db) = self.db() {
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
        if let Some(db) = self.db() {
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
        // One past the highest number in use: after a rule is removed, the count is taken.
        let next = self
            .rules
            .iter()
            .filter_map(|r| r.id.strip_prefix("rule-custom-")?.parse::<usize>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let id = format!("rule-custom-{next}");
        let rule = WatchRule {
            id,
            name: name.clone(),
            query,
            exclude_drafts: self.new_rule_exclude_drafts,
            exclude_mine: self.new_rule_exclude_mine,
            enabled: true,
        };
        if let Some(db) = self.db() {
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
            let r = r.clone();
            if let Some(db) = self.db() {
                let _ = db.upsert_rule(&r);
            }
        }
    }

    pub fn remove_rule(&mut self, rule_id: &str) {
        self.rules.retain(|r| r.id != rule_id);
        if let Some(db) = self.db() {
            let _ = db.delete_rule(rule_id);
        }
    }
}

fn setup(s: &mut App, waker: rune::Waker) {
    let decoder = std::sync::Arc::new(rune::ImageCache::new(waker.clone()));
    let logo = decoder.load(&include_bytes!("../assets/logo.png")[..]);
    s.intro_art = vec![
        decoder.load(&include_bytes!("../assets/onboarding/button-base.png")[..]),
        decoder.load(&include_bytes!("../assets/onboarding/button-top.png")[..]),
        decoder.load(&include_bytes!("../assets/onboarding/finger.png")[..]),
    ];
    s.avatars.setup(decoder, waker.clone());
    if s.preview {
        let _ = logo.wait();
        for art in &s.intro_art {
            let _ = art.wait();
        }
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
    s.judge_worker.set_waker(waker.clone());
    if s.ai_feature && !s.preview && s.allow_os_side_effects {
        let raw = judge::JudgeConfig::load_raw();
        s.judge = raw.clone().filter(judge::JudgeConfig::usable);
        let shown = raw.unwrap_or_else(judge::JudgeConfig::starting_point);
        s.set_ai_fields(shown.address(), &shown.model, &shown.api_key);
    }
    s.auth_worker.set_waker(waker.clone());
    s.fast_notif_worker.set_waker(waker.clone());
    s.discovery_worker.set_waker(waker.clone());
    s.verify_worker.set_waker(waker.clone());
    s.auto_merge_worker.set_waker(waker.clone());
    s.resolve_worker.set_waker(waker.clone());
    s.approve_worker.set_waker(waker.clone());
    s.unblock_worker.set_waker(waker.clone());
    s.autonomy_worker.set_waker(waker.clone());
    s.merge_worker.set_waker(waker);
    s.boot_worker.request((), |()| {
        Database::open_default().and_then(|db| db.load_snapshot())
    });
}

/// Records the events that are new and, unless `quiet`, shows each as a notification. Returns the
/// pull requests the new ones are about.
fn deliver_events(s: &mut App, events: Vec<NotificationEvent>, quiet: bool) -> Vec<String> {
    if !s.onboarded || !s.native_notifications || !s.allow_os_side_effects || events.is_empty() {
        return Vec::new();
    }
    let Some(db) = s.db() else {
        return Vec::new();
    };
    // Saving an event first is what makes it new: one already saved was shown before.
    let new: Vec<NotificationEvent> = events
        .into_iter()
        .filter(|event| db.insert_notification(event).unwrap_or(false))
        .collect();
    let mut delivered = Vec::new();
    for event in new {
        if !quiet {
            notify::dispatch_system_notification(&event, true);
        }
        delivered.push(event.pr_url.clone());
        s.notifications.insert(0, event);
    }
    delivered
}

fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    if s.preview {
        return false;
    }
    let mut changed = false;

    let typed = [&s.max_age_input, &s.ai_url, &s.ai_model, &s.ai_key]
        .map(|field| field.text.as_str())
        .join("\u{1f}");
    if s.typed.as_ref().is_none_or(|(seen, _)| *seen != typed) {
        s.typed = Some((typed, std::time::Instant::now()));
        tx.wake_after(TYPING_SETTLES);
    } else if s.typed.as_ref().is_some_and(|(_, at)| at.elapsed() >= TYPING_SETTLES) {
        // The age limit applies once it parses as a number.
        if let Ok(days) = s.max_age_input.text.trim().parse::<u32>()
            && days != s.sync_options.max_age_days
        {
            s.set_max_age_days(days);
            changed = true;
        }
        changed |= s.apply_ai_fields();
    }

    if let Some(snapshot) = s.boot_worker.take() {
        s.loading = false;
        match snapshot {
            Ok(snap) => s.apply_snapshot(snap),
            Err(error) => {
                s.boot_failed = true;
                s.status_banner = Some(format!(
                    "Could not read saved settings, so nothing will be saved over them. Quit and open meat proxy again. {error}"
                ));
            }
        }
        if !s.onboarded {
            s.intro = Some(std::time::Instant::now());
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
        match result {
            Ok(()) => {
                s.status_banner = None;
                s.trigger_sync();
            }
            Err(error) => s.status_banner = Some(format!("Could not sign in. {error}")),
        }
        changed = true;
    } else if s.auth_worker.is_pending() {
        // Show what gh prints while it waits for the browser.
        tx.wake_after(std::time::Duration::from_millis(150));
        let printed = s.auth_lines.lock().map_or(0, |lines| lines.len());
        changed |= printed != std::mem::replace(&mut s.auth_lines_shown, printed);
    }

    if let Some(result) = s.sync_worker.take() {
        match result {
            Ok(mut out) => {
                if !out.failed_rules.is_empty() {
                    s.status_banner =
                        Some(format!("Some rules did not sync: {}", out.failed_rules.join(" · ")));
                }
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
                // The list now says what GitHub thinks of each approval, so an approval made here
                // stops holding its button once its wash is over: a push that dismissed it can
                // be approved again.
                s.approved
                    .retain(|(_, at)| at.elapsed().as_secs_f32() < INK_WASH_SECS);
                s.repos = github::infer_repos_from_prs(&out.prs, &s.repos);
                s.note_arrivals(&out.prs);
                s.prs = out.prs;
                if s.rank_refresh_pending {
                    s.refresh_ranks();
                }
                s.queue_judgments();
                s.after_sync_track();
                s.run_autonomy();
                let events: Vec<_> = events
                    .into_iter()
                    .filter(|e| !s.is_ignored_url(&e.pr_url))
                    .collect();
                if s.onboarded {
                    // A request the notifications feed announced a moment ago looks like a second
                    // round to the sync when the user had reviewed before: it is the same one.
                    s.heard
                        .retain(|(_, at)| at.elapsed().as_secs() < HEARD_SECS);
                    let (heard, events): (Vec<_>, Vec<_>) = events.into_iter().partition(|e| {
                        e.id.starts_with("asked-again-")
                            && s.heard.iter().any(|(url, _)| *url == e.pr_url)
                    });
                    deliver_events(s, heard, true);
                    deliver_events(s, events, false);
                    if let Some(db) = s.db() {
                        let _ = db.save_prs(&s.prs);
                        let _ = db.save_repos(&s.repos);
                    }
                }
            }
            Err(reason) => s.sync_status = SyncStatus::NeedsAuth { reason },
        }
        if s.sync_again {
            s.trigger_sync();
        }
        changed = true;
    }

    if let Some(at) = s.intro {
        if at.elapsed().as_secs_f32() >= INTRO_SECS {
            s.intro = None;
        } else {
            tx.wake_after(std::time::Duration::from_millis(16));
        }
        changed = true;
    }

    if !s.arrivals.is_empty() {
        let now = std::time::Instant::now();
        s.arrivals.retain(|(_, at)| {
            now.saturating_duration_since(*at).as_secs_f32() < ARRIVAL_GLOW_SECS || *at > now
        });
        // Redraw every frame while rows slide in and their tint fades.
        tx.wake_after(std::time::Duration::from_millis(16));
        changed = true;
    }

    if let Some(batch) = s.judge_worker.take() {
        s.judge_spent += batch.usage.cost_usd;
        s.judge_off |= batch.fatal;
        for error in batch.errors.iter().take(3) {
            eprintln!("meat-proxy judge: {error}");
        }
        s.store_judgments(batch.judged);
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
            .filter(|e| !s.is_ignored_url(&e.pr_url))
            .collect();
        let announced = deliver_events(s, events, false);
        if !announced.is_empty() && !s.sync_worker.is_pending() {
            s.trigger_sync();
        }
        let now = std::time::Instant::now();
        s.heard.extend(announced.into_iter().map(|url| (url, now)));
        if let Some(modified) = notifications.modified
            && s.last_notif_modified.as_ref() != Some(&modified)
        {
            if let Some(db) = s.db() {
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
    if let Some(results) = s.autonomy_worker.take() {
        let mut errors = Vec::new();
        let now = chrono::Utc::now().timestamp();
        // Each is keyed by the commit it ran on, which a sync that landed meanwhile may have moved.
        for (pr, action, result) in results {
            let pr = &pr;
            // Done or failed, once per commit: a failure would fail the same way every minute.
            s.autonomy_done.insert(autonomy::key(pr, &action), now);
            match result {
                Ok(()) => {
                    let body = action.announce(pr);
                    notify::post(
                        &format!("autonomy-{}", autonomy::key(pr, &action)),
                        "meat proxy",
                        &body,
                        Some(&pr.url),
                        s.native_notifications && s.allow_os_side_effects,
                    );
                }
                Err(error) => errors.push(format!("{}#{}: {error}", pr.repo, pr.number)),
            }
        }
        s.autonomy_done
            .retain(|_, at| now - *at < autonomy::DONE_KEPT_SECS);
        if let Some(db) = s.db() {
            let mut done: Vec<String> =
                s.autonomy_done.iter().map(|(k, at)| format!("{k}\t{at}")).collect();
            done.sort_unstable();
            let _ = db.set_setting("autonomy_done", &done.join("\n"));
        }
        if !errors.is_empty() {
            s.status_banner = Some(format!("Autonomous: {}", errors.join(" · ")));
        }
        s.trigger_sync();
        changed = true;
    }
    if let Some((id, enable, result)) = s.auto_merge_worker.take() {
        s.auto_merging.retain(|x| *x != id);
        match result {
            Ok(()) => {
                if let Some(pr) = s.prs.iter_mut().find(|p| p.id == id) {
                    pr.auto_merge = if enable {
                        model::AutoMerge::On
                    } else {
                        model::AutoMerge::Available
                    };
                }
                s.after_sync_track();
                s.trigger_sync();
            }
            Err(error) => {
                s.status_banner = Some(format!(
                    "Could not turn auto-merge {}. {error}",
                    if enable { "on" } else { "off" }
                ));
            }
        }
        changed = true;
    }

    if let Some(results) = s.resolve_worker.take() {
        for (id, state) in results {
            let Ok(state) = state else { continue };
            let finished = match state.as_str() {
                "MERGED" => model::TrackedState::Merged,
                "CLOSED" => model::TrackedState::Closed,
                _ => continue,
            };
            if let Some(t) = s.tracked.iter_mut().find(|t| t.pr.id == id) {
                t.state = finished;
                let t = t.clone();
                s.save_tracked_row(&t);
                if finished == model::TrackedState::Merged {
                    notify::post(
                        &format!("auto-merged-{id}"),
                        "Auto-merged",
                        &format!("{}#{}: {}", t.pr.repo, t.pr.number, t.pr.title),
                        Some(&t.pr.url),
                        s.allow_os_side_effects,
                    );
                }
            }
        }
        changed = true;
    }

    if let Some((id, result)) = s.merge_worker.take() {
        s.merging.retain(|m| *m != id);
        match result {
            Ok(()) => {
                notify::play_sound("Hero", s.allow_os_side_effects);
                s.record_pace(pace::Kind::Merge, &id);
                s.celebrations.push((id, std::time::Instant::now()));
            }
            Err(error) => s.status_banner = Some(format!("Could not merge. {error}")),
        }
        changed = true;
    }
    if let Some((id, what, result)) = s.unblock_worker.take() {
        s.unblocking.retain(|u| *u != id);
        match result {
            Ok(()) => {
                // GitHub starts the checks over; until the next sync says how they went, the
                // pull request is no longer blocked by what the button cleared.
                if let Some(pr) = s.prs.iter_mut().find(|p| p.id == id) {
                    pr.ci_status = CiStatus::Pending;
                    pr.failing_checks.clear();
                    if what == Unblock::UpdateBranch {
                        pr.can_update_branch = false;
                        if pr.merge_readiness == model::MergeReadiness::Behind {
                            pr.merge_readiness = model::MergeReadiness::Unknown;
                        }
                    }
                }
            }
            Err(error) => {
                s.status_banner = Some(format!(
                    "Could not {}. {error}",
                    match what {
                        Unblock::UpdateBranch => "update the branch",
                        Unblock::Rerun => "run the checks again",
                    }
                ));
            }
        }
        changed = true;
    }
    if let Some((id, result)) = s.approve_worker.take() {
        s.approving.retain(|a| *a != id);
        match result {
            Ok(()) => {
                s.record_pace(pace::Kind::Approve, &id);
                s.approved.push((id, std::time::Instant::now()));
                s.trigger_sync();
            }
            Err(error) => s.status_banner = Some(format!("Could not approve. {error}")),
        }
        changed = true;
    }
    // A job that panicked settles with nothing to land; its button must not stay busy for good.
    for (pending, busy) in [
        (s.merge_worker.is_pending(), &mut s.merging),
        (s.approve_worker.is_pending(), &mut s.approving),
        (s.unblock_worker.is_pending(), &mut s.unblocking),
        (s.auto_merge_worker.is_pending(), &mut s.auto_merging),
    ] {
        if !pending && !busy.is_empty() {
            busy.clear();
            changed = true;
        }
    }
    if s.approve_armed
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed().as_secs_f32() > ARMED_SECS)
    {
        s.approve_armed = None;
        changed = true;
    }
    if s.merge_armed
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed().as_secs_f32() > ARMED_SECS)
    {
        s.merge_armed = None;
        changed = true;
    }
    if s
        .copied
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed().as_secs_f32() > COPIED_SECS)
    {
        s.copied = None;
        changed = true;
    }
    let flashes = s.pace_flash.len();
    s.pace_flash
        .retain(|(_, at)| at.elapsed().as_secs_f32() < PACE_FLASH_SECS);
    changed |= flashes != s.pace_flash.len();
    // Redraw every frame while an armed button's bar drains or a fresh approval's row washes.
    if s.approve_armed.is_some()
        || !s.pace_flash.is_empty()
        || s.merge_armed.is_some()
        || s.copied.is_some()
        || s.approved.iter().any(|(_, at)| at.elapsed().as_secs_f32() < INK_WASH_SECS)
    {
        tx.wake_after(std::time::Duration::from_millis(16));
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
            if let Some(db) = s.db() {
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

/// The logo and name. The Inbox shows them above its title; the setup page, in its bar.
fn brand(s: &App, k: &Kit) -> El {
    let keycap = if let Some(image) = &s.logo {
        el().size(22.0, 22.0).pixels(image.pixels())
    } else {
        el().size(22.0, 22.0)
    };
    row()
        .key("brand")
        .center_y()
        .gap(9.0)
        .cursor(rune::Cursor::Pointer)
        .on_press(|s: &mut App| {
            if s.page == Page::Inbox {
                s.selected_repo = None;
            }
        })
        .child(keycap)
        .child(
            text("meat proxy")
                .font(TextStyle::new(14.0, 650.0))
                .color(k.c.text_base),
        )
}

/// `hide_brand` while the intro flies its own logo and name into place.
fn top_bar(s: &App, k: &Kit, hide_brand: bool) -> El {
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
        Page::Onboarding => "Setup".to_owned(),
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
        row()
            .center_y()
            .child(text("Setup").font(k.ty.mono).color(c.text_muted))
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
                .child(if s.page == Page::Inbox {
                    el()
                } else {
                    brand(s, k)
                        .opacity(if hide_brand { 0.0 } else { 1.0 })
                        .opacity_motion(rune::Anim::INSTANT)
                })
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

/// Says how the Inbox is sorted, and lets you change it: AI (a model reads the review requests) or
/// rules alone. Next to it, a button puts the groups back in the default order.
fn sort_control(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let ai = s.smart_active();
    let available = s.smart_available();
    let tip = if !available {
        "Sorted by rules. Turn on AI sorting in Settings to sort with a model."
    } else if ai {
        "Sorted by alien intelligence: a model reads each review request. Click to sort by rules alone, with nothing sent to a model."
    } else {
        "Sorted by rules alone. Nothing is sent to a model. Click to sort with AI."
    };
    let mut label = row().center_y().gap(5.0);
    if ai {
        label = label.child(rune::icon(&BRAIN, 13.0).color(model_ink(k)));
    }
    let label = label.child(
        text(if ai {
            "Sorted by AI"
        } else {
            "Sorted by rules"
        })
        .font(k.ty.small)
        .color(c.text_muted),
    );
    let mut toggle = row()
        .key("sort-mode")
        .role("button")
        .label(if ai {
            "Sorted by AI"
        } else {
            "Sorted by rules"
        })
        .px(8.0)
        .py(4.0)
        .radius(6.0)
        .center_y()
        .help(tip)
        .child(label);
    if available {
        toggle = toggle
            .cursor(rune::Cursor::Pointer)
            .hover(c.hover)
            .on_press(|s: &mut App| s.toggle_smart_sort());
    }
    let reset = icon_action(&RESET, "Reset to the default order")
        .disabled(s.group_order_is_default())
        .on_press(|s: &mut App| s.reset_group_order())
        .build(k)
        .key("reset-group-order");
    row()
        .center_y()
        .gap(2.0)
        .children(s.ai_feature.then_some(toggle))
        .child(reset)
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

fn phase(t: f32, from: f32, to: f32) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p).powi(3)
}

/// Drawn exactly where the intro's clock puts it this frame: no spring follows the change.
fn frame_exact(e: El) -> El {
    let now = rune::Anim::INSTANT;
    e.scale_motion(now)
        .offset_motion(now)
        .opacity_motion(now)
        .rotate_motion(now)
        .blur_motion(now)
}

/// The first-launch intro at `t` seconds, over the whole `w`×`h` window: the approve button
/// springs in, a finger falls onto it, approvals fly off, the name appears, and then button and
/// name fly into the header, where the logo and name take over. Every value is computed from
/// `t`, so nothing springs. `name_w` and `name_h` are the full-size name's last laid-out size.
fn intro_view(s: &App, k: &Kit, t: f32, (w, h): (f32, f32), (name_w, name_h): (f32, f32)) -> El {
    let c = &k.c;
    let green = oklch(0.57, 0.15, 150.0);
    let art = |image: Option<&rune::Image>, size: f32| match image {
        Some(image) => el().size(size, size).pixels(image.pixels()),
        None => el().size(size, size),
    };
    let since_hit = t - INTRO_HIT;
    // The press: the face sinks, the button squashes, then both spring back.
    let press = if since_hit >= 0.0 {
        (-7.0 * since_hit).exp() * (16.0 * since_hit).cos()
    } else {
        0.0
    };
    let sink = press.clamp(0.0, 1.0) * 10.0;
    let arrive = phase(t, 0.1, 0.9);
    let spring = 1.0 - (-5.5 * arrive).exp() * (10.0 * arrive).cos();
    let shown = phase(t, 0.1, 0.5);
    // Into the header: the header's logo is centred at (95, 25), its name starts at x 115.
    let p = phase(t, 4.0, 4.85);
    let dock = if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) / 2.0
    };
    let lerp = |a: f32, b: f32| a + (b - a) * dock;
    let (cx, cy) = (w / 2.0, h / 2.0 - 55.0);

    let mut layer = col()
        .key("intro")
        .w_full()
        .h_full()
        .instant_changes()
        .cursor(rune::Cursor::Pointer)
        .on_press(|s: &mut App| s.intro = None)
        .child(frame_exact(
            el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .fill(c.bg_base)
                .opacity(1.0 - phase(t, 4.3, INTRO_SECS)),
        ))
        .child(frame_exact(
            el().absolute(cx - 210.0, cy - 210.0)
                .size(420.0, 420.0)
                .radial_gradient(
                    [0.5, 0.5],
                    210.0,
                    [
                        (
                            0.0,
                            green.alpha(
                                if k.dark { 0.42 } else { 0.24 } * (0.6 + 0.4 * press.max(0.0)),
                            ),
                        ),
                        (1.0, green.alpha(0.0)),
                    ],
                )
                .opacity(shown * (1.0 - dock)),
        ));
    for i in 0..3 {
        let from = INTRO_HIT + i as f32 * 0.11;
        let p = ease_out(phase(t, from, from + 1.1));
        layer = layer.child(frame_exact(
            el().absolute(cx - 100.0, cy - 100.0)
                .size(200.0, 200.0)
                .radius(100.0)
                .border(3.0 - 2.0 * p, mint(k))
                .scale(1.0 + p * (1.9 + i as f32 * 0.5))
                .opacity(if t < from { 0.0 } else { 0.7 * (1.0 - p) }),
        ));
    }
    // The button turns into the logo as it lands in the header.
    let logo = phase(dock, 0.5, 1.0);
    layer = layer.child(frame_exact(
        col()
            .absolute(cx - 100.0, cy - 100.0)
            .size(200.0, 212.0)
            .scale(lerp((0.55 + 0.45 * spring) * (1.0 - 0.07 * press), 0.11))
            .offset(lerp(0.0, 95.0 - cx), lerp(0.0, 25.7 - (cy + 6.0)))
            .opacity(shown)
            .blur((1.0 - shown) * 12.0)
            .child(
                art(s.intro_art.first(), 200.0)
                    .absolute(0.0, 12.0)
                    .opacity(1.0 - logo),
            )
            .child(
                art(s.intro_art.get(1), 200.0)
                    .absolute(0.0, 0.0)
                    .offset(0.0, sink)
                    .opacity(1.0 - logo),
            )
            .child(art(s.logo.as_ref(), 200.0).absolute(0.0, 0.0).opacity(logo)),
    ));
    let approvals = [
        ("LGTM", -150.0, 200.0),
        ("+1", -115.0, 230.0),
        ("ship it", -65.0, 210.0),
        ("approved", -30.0, 250.0),
        ("👍", 20.0, 200.0),
        ("didn't read, looks good", 160.0, 215.0),
    ];
    for (i, (label, degrees, distance)) in approvals.into_iter().enumerate() {
        let from = INTRO_HIT + 0.04 + i as f32 * 0.035;
        let p = 1.0 - (1.0 - phase(t, from, from + 1.25)).powi(5);
        let angle = (degrees as f32).to_radians();
        let width = label.chars().count() as f32 * 7.5 + 22.0;
        layer = layer.child(frame_exact(
            row()
                .absolute(cx - width / 2.0, cy - 13.0)
                .px(11.0)
                .py(5.0)
                .radius(999.0)
                .fill(mint(k).alpha(0.12))
                .border(1.0, mint(k).alpha(0.3))
                .offset(
                    angle.cos() * distance * p,
                    angle.sin() * distance * p + 40.0 * p * p,
                )
                .opacity(if t < from {
                    0.0
                } else {
                    (p * 6.0).min(1.0) * (1.0 - phase(t, from + 0.7, from + 1.3))
                })
                .child(text(label).font(TextStyle::new(14.0, 600.0)).color(mint(k))),
        ));
    }
    let fall = phase(t, 0.8, INTRO_HIT).powi(3);
    let lift = phase(t, 1.9, 2.6).powi(2);
    let drop = if t >= INTRO_HIT {
        sink
    } else {
        -360.0 * (1.0 - fall)
    };
    layer = layer.child(frame_exact(
        art(s.intro_art.get(2), 110.0)
            .size(110.0, 275.0)
            .absolute(cx - 55.0, cy - 295.0)
            .offset(0.0, drop - lift * 360.0)
            .rotate(((1.0 - fall) * -4.0 + lift * 3.0).to_radians())
            .opacity(ease_out(phase(t, 0.8, 1.15)) * (1.0 - phase(t, 2.0, 2.6))),
    ));

    let name_top = cy + 110.0;
    let mut name = row()
        .key("intro-name")
        .absolute(cx - name_w / 2.0, name_top)
        .origin(0.0, 0.5)
        .scale(lerp(1.0, 14.0 / 64.0))
        .offset(
            lerp(0.0, 115.0 - (cx - name_w / 2.0)),
            lerp(0.0, 25.0 - (name_top + name_h / 2.0)),
        );
    for (i, ch) in "meat proxy".chars().enumerate() {
        let from = 2.4 + i as f32 * 0.055;
        let p = ease_out(phase(t, from, from + 0.7));
        name = name.child(frame_exact(if ch == ' ' {
            el().size(18.0, 10.0)
        } else {
            text(ch.to_string())
                .font(TextStyle::new(64.0, 720.0))
                .color(c.text_base)
                .opacity(p)
                .offset(0.0, (1.0 - p) * 22.0)
                .blur((1.0 - p) * 8.0)
        }));
    }
    let tagline = ease_out(phase(t, 3.0, 3.8)) * (1.0 - phase(t, 3.9, 4.2));
    layer.child(frame_exact(name)).child(frame_exact(
        row()
            .absolute(0.0, name_top + name_h + 4.0)
            .w_full()
            .justify_center()
            .opacity(tagline)
            .offset(0.0, (1.0 - tagline) * 12.0)
            .child(
                text("Keep up with the pace of LLM-generated code.")
                    .font(TextStyle::new(19.0, 450.0))
                    .color(c.text_muted),
            ),
    ))
}

fn onboarding_page_view(s: &App, k: &Kit, intro: Option<f32>) -> El {
    let c = &k.c;
    let connected = match &s.sync_status {
        SyncStatus::Connected { username } => Some(username),
        _ => None,
    };
    let mut doc = col()
        .key("setup-github")
        .w_full()
        .max_w(560.0)
        .py(48.0)
        .gap(14.0)
        .child(
            text("Connect GitHub.")
                .font(TextStyle::new(40.0, 720.0))
                .color(c.text_base),
        )
        .child(
            text("meat proxy puts the pull requests that need your thumb first. It reads GitHub through the GitHub CLI, signed in as you. No tokens to paste.")
                .wrap()
                .font(k.ty.body)
                .color(c.text_muted),
        );

    if let Some(message) = &s.status_banner {
        doc = doc.child(
            text(message.clone())
                .wrap()
                .font(k.ty.small)
                .color(c.text_muted),
        );
    }

    let (glyph, status) = if let Some(username) = connected {
        ("✓", format!("Signed in as @{username} through GitHub CLI."))
    } else if s.auth_worker.is_pending() {
        (
            "❯_",
            "Waiting for you to finish in the browser…".to_string(),
        )
    } else if s.loading
        || s.sync_worker.is_pending()
        || matches!(s.sync_status, SyncStatus::Checking)
    {
        ("❯_", "Checking GitHub CLI…".to_string())
    } else {
        ("❯_", "GitHub CLI is not signed in.".to_string())
    };
    let mut card = col()
        .w_full()
        .radius(10.0)
        .fill(surface(k))
        .border(1.0, c.border_weak)
        .child(
            row()
                .w_full()
                .px(16.0)
                .py(14.0)
                .center_y()
                .gap(12.0)
                .child(text(glyph).font(k.ty.mono).color(mint(k)))
                .child(text(status).font(k.ty.body).color(c.text_base)),
        );
    // What gh prints while it signs in, the one-time code among it.
    let lines: Vec<String> = s
        .auth_lines
        .lock()
        .map(|lines| lines.iter().rev().take(4).rev().cloned().collect())
        .unwrap_or_default();
    if connected.is_none() && (s.auth_worker.is_pending() || !lines.is_empty()) {
        let mut term = col()
            .key("onboard-terminal")
            .w_full()
            .px(16.0)
            .py(12.0)
            .gap(4.0)
            .child(
                text("❯ gh auth login --web")
                    .font(k.ty.mono)
                    .color(c.text_base),
            );
        for line in lines {
            term = term.child(text(line).wrap().font(k.ty.mono).color(c.text_muted));
        }
        card = card
            .child(el().w_full().h(1.0).fill(c.border_weak))
            .child(term);
    }
    doc = doc.child(card).child(
        row()
            .w_full()
            .px(16.0)
            .py(14.0)
            .radius(10.0)
            .fill(surface(k))
            .border(1.0, c.border_weak)
            .center_y()
            .gap(16.0)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(text("Tap me on the shoulder").font(TextStyle::new(15.0, 650.0)).color(c.text_base))
                    .child(
                        text("A macOS notification when a review is requested of you or your PR sits unreviewed.")
                            .wrap()
                            .font(k.ty.small)
                            .color(c.text_muted),
                    ),
            )
            .child(
                Switch::new(s.native_notifications)
                    .label("Tap me on the shoulder")
                    .hide_label()
                    .on_toggle(|s: &mut App| {
                        s.native_notifications = !s.native_notifications;
                        if s.native_notifications {
                            s.send_welcome_notification();
                        }
                    })
                    .build(k)
                    .key("onboard-notifications"),
            ),
    );

    if connected.is_some() {
        doc = doc.child(
            row().pt(6.0).child(
                Button::new("Open inbox")
                    .variant(ButtonVariant::Contrast)
                    .on_press(|s: &mut App| {
                        s.complete_onboarding();
                        s.trigger_discovery();
                    })
                    .build(k)
                    .key("onboard-open-inbox"),
            ),
        );
    } else {
        doc = doc
            .child(
                row()
                    .pt(6.0)
                    .gap(8.0)
                    .child(
                        Button::new("Authenticate with GitHub CLI")
                            .variant(ButtonVariant::Contrast)
                            .disabled(s.auth_worker.is_pending())
                            .on_press(|s: &mut App| {
                                if s.allow_os_side_effects {
                                    s.status_banner = None;
                                    s.auth_lines.lock().unwrap().clear();
                                    let lines = s.auth_lines.clone();
                                    s.auth_worker.request((), move |()| github::login_in_browser(lines));
                                }
                            })
                            .build(k)
                            .key("onboard-run-gh-auth"),
                    )
                    .child(
                        Button::new("Check connection")
                            .variant(ButtonVariant::Outline)
                            .disabled(s.sync_worker.is_pending())
                            .on_press(|s: &mut App| {
                                s.status_banner = None;
                                s.trigger_sync();
                            })
                            .build(k)
                            .key("onboard-recheck-gh"),
                    ),
            )
            .child(
                text("Or run gh auth login --hostname github.com --web in your terminal, then check the connection.")
                    .wrap()
                    .font(k.ty.small)
                    .color(c.text_muted),
            );
        if let SyncStatus::NeedsAuth { reason } = &s.sync_status
            && !s.auth_worker.is_pending()
        {
            doc = doc.child(
                text(reason.clone())
                    .wrap()
                    .font(k.ty.small)
                    .color(c.text_faint),
            );
        }
    }

    // The scroll sits on a column: a row clips its children to its own height and never overflows.
    let setup = col().w_full().h_full().min_h(0.0).child(
        col()
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .grow(1.0)
            .min_h(0.0)
            .child(row().w_full().px(28.0).justify_center().child(doc)),
    );
    match intro {
        // The setup screen fades in under the intro as it ends.
        Some(t) => setup
            .instant_changes()
            .opacity(phase(t, INTRO_SECS - 0.6, INTRO_SECS)),
        None => setup,
    }
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
                && let Some(db) = s.db()
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

/// Ignore a pull request: an eye with a slash through it. Stop ignoring: the eye.
const EYE_OFF: rune::Icon = rune::Icon {
    name: "ignore",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M1.6 8s2.3-4.2 6.4-4.2S14.4 8 14.4 8s-2.3 4.2-6.4 4.2S1.6 8 1.6 8Z"/><circle fill="none" stroke="currentColor" stroke-width="1.3" cx="8" cy="8" r="1.9"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" d="M2.8 13.2L13.2 2.8"/>"#,
};
const EYE: rune::Icon = rune::Icon {
    name: "stop-ignoring",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M1.6 8s2.3-4.2 6.4-4.2S14.4 8 14.4 8s-2.3 4.2-6.4 4.2S1.6 8 1.6 8Z"/><circle fill="none" stroke="currentColor" stroke-width="1.3" cx="8" cy="8" r="1.9"/>"#,
};

/// Merge: two branches joining.
const MERGE: rune::Icon = rune::Icon {
    name: "merge",
    view: 16.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="4.5" cy="3.5" r="1.6"/><circle cx="4.5" cy="12.5" r="1.6"/><circle cx="11.5" cy="9" r="1.6"/><path d="M4.5 5.1v5.8M4.5 5.1c0 2.4 2.2 3.9 5.4 3.9"/></g>"#,
};

/// Auto-merge: a bolt.
const BOLT: rune::Icon = rune::Icon {
    name: "auto-merge",
    view: 16.0,
    body: r#"<path fill="currentColor" d="M9.2 1.5 3.8 9h3.5l-.7 5.5L12.2 7H8.7l.5-5.5Z"/>"#,
};

/// A new best of the week: a cup.
const TROPHY: rune::Icon = rune::Icon {
    name: "trophy",
    view: 16.0,
    body: r#"<path fill="currentColor" fill-rule="evenodd" d="M4.5 2h7v1h2v1.5a2.5 2.5 0 0 1-2.2 2.5 3.5 3.5 0 0 1-2.55 2.4v2.1h2.25V13h-6v-1.5h2.25V9.4A3.5 3.5 0 0 1 4.7 7 2.5 2.5 0 0 1 2.5 4.5V3h2Zm7 2.4v1.4a1 1 0 0 0 .5-.9v-.5Zm-7 0H4v.5a1 1 0 0 0 .5.9Z"/>"#,
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

/// Marks what a model decided, as opposed to a rule: a brain, two lobes with a fold in each.
const BRAIN: rune::Icon = rune::Icon {
    name: "model-brain",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M8 3.4C8 2.3 7.1 1.7 6.2 2c-1-.4-2.2.3-2.1 1.4-1.1.2-1.8 1.4-1.2 2.4C2 6.5 2 7.8 2.9 8.4c-.2 1.1.5 2.1 1.6 2.2.2 1.1 1.3 1.8 2.3 1.5.4.5.8.6 1.2.6"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M8 3.4C8 2.3 8.9 1.7 9.8 2c1-.4 2.2.3 2.1 1.4 1.1.2 1.8 1.4 1.2 2.4.9.7.9 2 0 2.6.2 1.1-.5 2.1-1.6 2.2-.2 1.1-1.3 1.8-2.3 1.5-.4.5-.8.6-1.2.6"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M8 3.4v9.1"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M4.2 5.2c1 .2 1.7.9 1.8 1.8M11.8 5.2c-1 .2-1.7.9-1.8 1.8M4.5 10.6c.9-.2 1.5-.8 1.6-1.6M11.5 10.6c-.9-.2-1.5-.8-1.6-1.6"/>"#,
};

/// Back to the default: an arrow turning counter-clockwise.
const RESET: rune::Icon = rune::Icon {
    name: "reset-order",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M3.2 8a4.8 4.8 0 1 0 1.5-3.5M3 2.6v2.6h2.6"/>"#,
};

/// Update branch: two arrows chasing each other.
const UPDATE: rune::Icon = rune::Icon {
    name: "update-branch",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M12.9 7a5 5 0 0 0-8.7-2.3M3.1 9a5 5 0 0 0 8.7 2.3M4 2.3v2.6h2.6M12 13.7v-2.6H9.4"/>"#,
};

/// Copy: one sheet over another.
const COPY: rune::Icon = rune::Icon {
    name: "copy-prompt",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M7.2 5.6h4.2a1.6 1.6 0 0 1 1.6 1.6v4.2a1.6 1.6 0 0 1-1.6 1.6H7.2a1.6 1.6 0 0 1-1.6-1.6V7.2a1.6 1.6 0 0 1 1.6-1.6ZM10.4 3.9v-.3a1.2 1.2 0 0 0-1.2-1.2H4A1.6 1.6 0 0 0 2.4 4v5.2a1.2 1.2 0 0 0 1.2 1.2h.3"/>"#,
};

/// The colour of the buttons that clear a blocker: a calm blue, apart from Approve's mint and
/// Merge's violet.
fn unblock_tone(k: &Kit) -> Color {
    oklch(if k.dark { 0.74 } else { 0.55 }, 0.13, 245.0)
}

/// The color of anything a model decided, so it reads as one thing apart from the rules.
fn model_ink(k: &Kit) -> Color {
    // Quiet on purpose: a hint of violet, close to the muted text, so it never competes with a status.
    oklch(if k.dark { 0.66 } else { 0.64 }, 0.045, 295.0)
}

const GRIP: rune::Icon = rune::Icon {
    name: "drag-handle",
    view: 16.0,
    body: r#"<circle cx="6" cy="4" r="1.1"/><circle cx="10" cy="4" r="1.1"/><circle cx="6" cy="8" r="1.1"/><circle cx="10" cy="8" r="1.1"/><circle cx="6" cy="12" r="1.1"/><circle cx="10" cy="12" r="1.1"/>"#,
};

const SECTION_GAP: f32 = 16.0;
/// Header plus its 1px rule and the section border.
const HEADER_HEIGHT: f32 = 40.0;
const SECTION_HEIGHT: f32 = HEADER_HEIGHT + 3.0;

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

    // The status, its colour, and what a tooltip adds to it.
    let (status, tone, why) = match att {
        PrAttention::IncomingAssigned => match &pr.asked {
            // A day or more: say how long it has waited, and warn once it is as stale as the
            // user lets their own pull requests get.
            Some(ask) if ask.hours_ago >= 24 => (
                format!("Waiting on you {}", model::span(ask.hours_ago)),
                if ask.hours_ago >= s.open_stale_hours {
                    c.warning.fg
                } else {
                    mint(k)
                },
                if ask.again {
                    "Asked again after your last review"
                } else {
                    "Since your review was requested"
                },
            ),
            Some(ask) if ask.again => (
                "Review requested again".to_owned(),
                mint(k),
                "You reviewed this before; the author asked for another look",
            ),
            _ => ("Review requested".to_owned(), mint(k), ""),
        },
        PrAttention::IncomingViaTeam => ("Team review requested".to_owned(), c.text_muted, ""),
        PrAttention::MyOpenNeedsNudge if pr.reviewers.is_empty() => {
            ("Needs reviewer".to_owned(), c.warning.fg, "")
        }
        PrAttention::MyOpenNeedsNudge => (
            format!("Unreviewed {}d", (pr.unreviewed_hours / 24).max(1)),
            c.warning.fg,
            "",
        ),
        PrAttention::MyDraftNeedsNudge => ("Stale draft".to_owned(), c.warning.fg, ""),
        PrAttention::MyOpenChangesRequested
            if pr.review_state != model::ReviewState::ChangesRequested =>
        {
            (
                comments_to_answer(pr.unanswered()),
                c.warning.fg,
                "Review threads still open, where the last word is not yours",
            )
        }
        PrAttention::MyOpenChangesRequested => ("Changes requested".to_owned(), c.danger.fg, ""),
        PrAttention::MyOpenBlocked => match pr.blockers().first() {
            Some(Blocker::Behind) => (
                Blocker::Behind.label().to_owned(),
                c.warning.fg,
                "The repository merges only branches that are up to date with their base",
            ),
            Some(Blocker::ChecksFailing) => (pr.failing_label(), c.danger.fg, ""),
            _ => (
                Blocker::Conflicts.label().to_owned(),
                c.danger.fg,
                "It conflicts with its base branch",
            ),
        },
        PrAttention::MyOpenApproved => (pr.approval_label(), mint(k), ""),
        PrAttention::MyOpenWaiting => match pr.reviewers.as_slice() {
            [] => ("Waiting for review".to_owned(), c.text_muted, ""),
            [one] => (format!("Waiting on {one}"), c.text_muted, ""),
            [one, rest @ ..] => (
                format!("Waiting on {one} +{}", rest.len()),
                c.text_muted,
                "",
            ),
        },
        PrAttention::MyDraftActive => ("Draft".to_owned(), c.text_muted, ""),
        PrAttention::IncomingWatched => ("Watching".to_owned(), c.text_muted, ""),
    };
    let pr_url_open = pr.url.clone();
    let id_toggle = pr.id.clone();

    let mut right_side = row().center_y().gap(2.0);
    if s.filter == InboxFilter::Inbox && s.page == Page::Inbox {
        let ignored = s.ignored.contains(&pr.id);
        let id = pr.id.clone();
        let button = header_action(
            k,
            if ignored { &EYE } else { &EYE_OFF },
            if ignored {
                "Stop ignoring"
            } else {
                "Ignore: move it to Ignored"
            },
            format!("ignore-{}", pr.id),
            270.0,
            ignored,
        )
        .on_press(move |s: &mut App| s.toggle_ignore(&id));
        // It shows while the row is hovered, and always in the Ignored section.
        right_side = right_side.child(if ignored {
            button
        } else {
            button.group_opacity(0.0, 1.0)
        });
    }
    if let Some(pill) = pace_pill(s, k, pr) {
        right_side = right_side.child(pill);
    }
    if matches!(
        att,
        PrAttention::IncomingAssigned | PrAttention::IncomingViaTeam
    ) && !pr.is_draft
        && s.page == Page::Inbox
    {
        right_side = right_side.child(approve_control(s, k, pr));
    }
    if pr.ownership == Ownership::Mine && !pr.is_draft && s.page == Page::Inbox {
        let unblock = unblock_control(s, k, pr, att);
        // In Unblock the status says why it cannot merge; a Merge button that can do nothing
        // only says it again.
        let merge_is_dead =
            !pr.merge_readiness.is_ready() && s.auto_merge_target(&pr.id).is_none();
        let shows_merge = !(merge_is_dead && att == PrAttention::MyOpenBlocked);
        if let Some((button, _)) = unblock {
            right_side = right_side.child(button);
        }
        if shows_merge {
            right_side = right_side.child(merge_control(s, k, pr));
        }
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
        .child(
            text(pr.age_label())
                .font(k.ty.small)
                .color(c.text_muted)
                .help(pr.age_detail()),
        )
        .child(el().size(1.0, 10.0).fill(c.border_base))
        .child({
            let label = text(status.clone()).font(k.ty.small_medium).color(tone);
            if why.is_empty() { label } else { label.help(why) }
        });
    // A brain marks what the model judged; a rule's verdict needs none. In the Inbox the section
    // already says what it means, so the questions that came out yes are named only elsewhere.
    if s.smart_active()
        && let Some(judgment) = s.judgment(pr).filter(|j| j.by != "rules")
    {
        let fired = s.layout.fired(&judgment.answers);
        let in_model_section = s
            .action_group_of(pr, att)
            .and_then(|(id, _)| s.layout.sections.get(usize::from(id)))
            .is_some_and(sections::Section::uses_model);
        if !fired.is_empty() && (in_model_section || s.filter != InboxFilter::Inbox) {
            let mut mark = row()
                .center_y()
                .gap(4.0)
                .help(judgment.summary())
                .child(rune::icon(&BRAIN, 12.0).color(model_ink(k)));
            if s.filter != InboxFilter::Inbox {
                mark = mark.child(
                    text(fired.join(", ").replace(['-', '_'], " "))
                        .font(k.ty.small)
                        .color(model_ink(k)),
                );
            }
            metadata = metadata.child(mark);
        }
    }
    if let Some(parent) = item.parent {
        metadata = metadata.child(
            text(format!("↳ #{parent}"))
                .font(k.ty.small)
                .color(c.text_muted)
                .help(format!("Stacked on #{parent}")),
        );
    }
    for (label, tone, why) in blocker_chips(pr, &status, k) {
        let chip = text(label).font(k.ty.small_medium).color(tone);
        metadata = metadata.child(if why.is_empty() { chip } else { chip.help(why) });
    }
    let mut line = row()
        .key(format!("pr-row-{}", pr.id))
        .w_full()
        .pr(14.0)
        .pl(14.0 + item.depth.min(5) as f32 * 18.0)
        .py(10.0)
        .center_y()
        .gap(12.0)
        .hover_group()
        .hover(c.hover)
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut App| s.toggle_expanded(&id_toggle))
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
                    // Only the title goes to GitHub; the rest of the row opens the description.
                    text(pr.title.clone())
                        .font(TextStyle::new(14.0, 530.0))
                        .color(c.text_base)
                        .truncate()
                        .link(
                            rune::Link::new(0..pr.title.len(), "open")
                                .hover(rune::LinkPaint::new().underline(c.text_base))
                                .help("Open on GitHub"),
                        )
                        .on_link(move |s: &mut App, _: &str, _: &mut Tx| {
                            notify::open_url_in_browser(&pr_url_open, s.allow_os_side_effects);
                        }),
                )
                .child(metadata),
        )
        .child(right_side);
    let approved_at = s
        .approved
        .iter()
        .find(|(id, _)| *id == pr.id)
        .map(|(_, at)| (mint(k), at.elapsed().as_secs_f32()));
    if let Some((tone, t)) = celebrating.map(|t| (merge_tone(k), t)).or(approved_at) {
        // The button's colour washes over the row once and ebbs away.
        line = line.fill(tone.alpha((0.16 * (1.0 - t / INK_WASH_SECS)).max(0.0)));
    }
    if let Some(age) = s.arrival_age(&pr.id) {
        // New here: it fades in and settles down a few pixels, then a mint tint ebbs away.
        let slide = (age / ARRIVAL_SLIDE_SECS).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - slide).powi(3);
        let glow = (1.0 - age / ARRIVAL_GLOW_SECS).clamp(0.0, 1.0);
        line = line
            .opacity(ease)
            .offset(0.0, (1.0 - ease) * -10.0)
            .fill(mint(k).alpha(0.16 * glow * glow));
    }
    if s.expanded.contains(&pr.id) {
        let indent = 14.0 + 28.0 + item.depth.min(5) as f32 * 18.0;
        return col()
            .w_full()
            .child(line)
            .child(merge_path(s, k, pr, indent))
            .child(description(s, k, pr, indent));
    }
    line
}

fn comments_to_answer(n: u32) -> String {
    if n == 1 {
        "1 comment to answer".to_owned()
    } else {
        format!("{n} comments to answer")
    }
}

/// What the row says after its status about the way to the merge: each thing in it, its colour
/// and a tooltip. On someone else's pull request these are the author's to clear: they are said
/// quietly, and the tooltip says whose move it is.
fn blocker_chips(pr: &PullRequest, status: &str, k: &Kit) -> Vec<(String, Color, String)> {
    let c = &k.c;
    let mine = pr.ownership == Ownership::Mine;
    let mut chips = Vec::new();
    for blocker in pr.blockers() {
        let (tone, why) = match blocker {
            Blocker::Conflicts if mine => (c.danger.fg, "It conflicts with its base branch"),
            Blocker::Conflicts => (c.text_muted, "The author has conflicts to resolve first"),
            Blocker::Behind if mine => (
                c.warning.fg,
                "The repository merges only branches that are up to date with their base",
            ),
            Blocker::Behind => (c.text_muted, "The author has to update the branch first"),
            // Named below, whether or not GitHub holds the merge for it.
            Blocker::ChecksFailing => continue,
        };
        chips.push((blocker.label().to_owned(), tone, why.to_owned()));
    }
    if pr.ci_status == CiStatus::Failing {
        let names: Vec<&str> = pr.failing_checks.iter().map(|c| c.name.as_str()).collect();
        let why = match (names.is_empty(), mine) {
            (true, true) => String::new(),
            (true, false) => "The author has failing checks to fix first".to_owned(),
            (false, _) => format!("Failed: {}", names.join(", ")),
        };
        let tone = if mine { c.danger.fg } else { c.text_muted };
        chips.push(("CI failing".to_owned(), tone, why));
    }
    if mine && pr.unanswered() > 0 {
        chips.push((
            comments_to_answer(pr.unanswered()),
            c.warning.fg,
            "Review threads still open, where the last word is not yours".to_owned(),
        ));
    }
    // The status already says the first of these.
    chips.retain(|(label, _, _)| !status.starts_with(label.as_str()));
    chips
}

/// The button for what holds up one of the user's pull requests, and whether it shows at rest.
/// Conflicts and failed checks take code to clear, so their button copies a prompt for a coding
/// agent; a branch that is only behind is updated from here. It shows at rest when it answers the
/// row's status, and waits for the pointer anywhere else.
fn unblock_control(s: &App, k: &Kit, pr: &PullRequest, att: PrAttention) -> Option<(El, bool)> {
    let blockers = pr.blockers();
    let first = (att == PrAttention::MyOpenBlocked)
        .then(|| blockers.first().copied())
        .flatten();
    let busy = s.unblocking.contains(&pr.id);
    let key = format!("unblock-{}", pr.id);
    let tone = unblock_tone(k);
    let id = pr.id.clone();
    // While GitHub holds the merge for a branch that is behind, updating it comes first: the
    // checks run again on the result.
    let update_first = blockers.contains(&Blocker::Behind) && pr.can_update_branch;
    let prompt = if update_first { None } else { pr.agent_prompt() };
    let (button, why, at_rest) = if let Some(prompt) = prompt {
        let copied = s
            .copied
            .as_ref()
            .filter(|(copied, _)| *copied == pr.id)
            .map(|(_, at)| at.elapsed().as_secs_f32());
        let button = match copied {
            Some(age) => ink_button(
                k,
                tone,
                &rune_kit::glyphs::CHECK,
                "Copied",
                Ink::Done(age),
                key,
            ),
            None => ink_button(k, tone, &COPY, "Copy prompt", Ink::Rest, key).on_press_tx(
                move |s: &mut App, tx: &mut Tx| {
                    tx.copy(prompt.clone());
                    s.copied = Some((id.clone(), std::time::Instant::now()));
                },
            ),
        };
        let conflicts = blockers.contains(&Blocker::Conflicts);
        let why = match (conflicts, pr.ci_status == CiStatus::Failing) {
            (true, true) => {
                "Copy a prompt for your coding agent: resolve the conflicts and fix the failed checks"
            }
            (true, false) => "Copy a prompt for your coding agent: resolve the conflicts",
            _ => "Copy a prompt for your coding agent: fix the failed checks",
        };
        (
            button,
            why,
            first.is_some_and(|blocker| blocker != Blocker::Behind) || copied.is_some(),
        )
    } else if pr.can_update_branch {
        let (label, ink) = if busy {
            ("Updating…", Ink::Busy)
        } else {
            ("Update branch", Ink::Rest)
        };
        (
            ink_button(k, tone, &UPDATE, label, ink, key)
                .on_press(move |s: &mut App| s.press_unblock(&id, Unblock::UpdateBranch)),
            "Merge the base branch into this one, as Update branch does on GitHub",
            first == Some(Blocker::Behind),
        )
    } else {
        return None;
    };
    let wrap = row().center_y().hover_group().help(why).child(button);
    Some(if at_rest || busy {
        (wrap, true)
    } else {
        (wrap.group_opacity(0.0, 1.0), false)
    })
}

/// Under an opened row, above the description: where the pull request stands on its way to the
/// merge. Checks that failed link to their logs, and the user's own can be run again from here.
fn merge_path(s: &App, k: &Kit, pr: &PullRequest, indent: f32) -> El {
    let c = &k.c;
    let fact = |label: &str, value: El| {
        row()
            .w_full()
            .gap(10.0)
            .child(text(label.to_owned()).font(k.ty.small).color(c.text_faint).w(52.0))
            .child(value.grow(1.0).min_w(0.0))
    };
    let plain = |value: String, tone: Color| text(value).font(k.ty.small).color(tone).wrap();
    let mut facts: Vec<El> = Vec::new();

    if !pr.failing_checks.is_empty() {
        let mut line = String::new();
        let mut links = Vec::new();
        for check in &pr.failing_checks {
            if !line.is_empty() {
                line.push_str("  ·  ");
            }
            let start = line.len();
            line.push_str(&check.name);
            if !check.url.is_empty() {
                links.push(
                    rune::Link::new(start..line.len(), check.url.clone())
                        .hover(rune::LinkPaint::new().underline(c.danger.fg))
                        .help("Open this check"),
                );
            }
        }
        let mut names = plain(line, c.danger.fg);
        for link in links {
            names = names.link(link);
        }
        let names = names.on_link(|s: &mut App, url: &str, _: &mut Tx| {
            notify::open_url_in_browser(url, s.allow_os_side_effects)
        });
        // Running the failed jobs again only helps when the failure was a fluke, so it is offered
        // here and not as the row's button.
        let can_rerun = pr.ownership == Ownership::Mine
            && !pr.rerunnable().is_empty()
            && !s.unblocking.contains(&pr.id);
        let failed = if can_rerun {
            let label = "Run the failed jobs again";
            let id = pr.id.clone();
            col().gap(3.0).child(names).child(
                text(label)
                    .key(format!("rerun-{}", pr.id))
                    .font(k.ty.small)
                    .color(unblock_tone(k))
                    .link(
                        rune::Link::new(0..label.len(), "rerun")
                            .hover(rune::LinkPaint::new().underline(unblock_tone(k)))
                            .help("Runs the failed GitHub Actions jobs again, which helps when the failure was a fluke"),
                    )
                    .on_link(move |s: &mut App, _: &str, _: &mut Tx| {
                        s.press_unblock(&id, Unblock::Rerun)
                    }),
            )
        } else {
            names
        };
        facts.push(fact("Failed", failed));
    } else {
        match pr.ci_status {
            CiStatus::Failing => facts.push(fact("Checks", plain("Failing".into(), c.danger.fg))),
            CiStatus::Pending => facts.push(fact("Checks", plain("Running".into(), c.text_muted))),
            CiStatus::Passing => facts.push(fact("Checks", plain("Passing".into(), c.text_muted))),
            CiStatus::None => {}
        }
    }

    match pr.merge_readiness {
        model::MergeReadiness::Conflicts => {
            facts.push(fact("Branch", plain("Conflicts with its base".into(), c.danger.fg)))
        }
        model::MergeReadiness::Behind => facts.push(fact(
            "Branch",
            plain("Behind its base, which has to be merged in first".into(), c.warning.fg),
        )),
        _ if pr.can_update_branch => {
            facts.push(fact("Branch", plain("Behind its base".into(), c.text_muted)))
        }
        _ => {}
    }

    let mut reviews: Vec<String> = Vec::new();
    if !pr.approved_by.is_empty() {
        reviews.push(pr.approval_label());
    }
    let pending: Vec<&str> = pr
        .reviewers
        .iter()
        .filter(|r| !pr.approved_by.contains(r))
        .map(String::as_str)
        .collect();
    if !pending.is_empty() {
        reviews.push(format!("waiting on {}", pending.join(", ")));
    }
    if pr.unanswered() > 0 {
        reviews.push(comments_to_answer(pr.unanswered()));
    }
    if let Some(ask) = &pr.asked {
        reviews.push(format!(
            "you were asked {}{} ago",
            if ask.again { "again " } else { "" },
            model::span(ask.hours_ago)
        ));
    }
    if !reviews.is_empty() {
        facts.push(fact("Reviews", plain(reviews.join("  ·  "), c.text_muted)));
    }

    if !pr.files.is_empty() {
        const SHOWN: usize = 6;
        let mut list = col().gap(2.0).child(plain(
            format!("+{} −{}", pr.additions, pr.deletions),
            c.text_muted,
        ));
        for path in pr.files.iter().take(SHOWN) {
            list = list.child(
                text(path.clone())
                    .font(k.ty.mono)
                    .color(c.text_muted)
                    .truncate_head(),
            );
        }
        if pr.files.len() >= github::FILES_FETCHED {
            list = list.child(plain("and more".into(), c.text_faint));
        } else if pr.files.len() > SHOWN {
            list = list.child(plain(
                format!("and {} more", pr.files.len() - SHOWN),
                c.text_faint,
            ));
        }
        facts.push(fact("Files", list));
    }

    let mut block = col()
        .key(format!("pr-path-{}", pr.id))
        .w_full()
        .pl(indent)
        .pr(14.0)
        .gap(5.0);
    if !facts.is_empty() {
        block = block.pb(10.0);
    }
    for fact in facts {
        block = block.child(fact);
    }
    block
}

/// The PR description as Markdown, under its row.
fn description(s: &App, k: &Kit, pr: &PullRequest, indent: f32) -> El {
    let c = &k.c;
    let source = clean_description(&pr.description);
    let body = if source.is_empty() {
        text("No description.").font(k.ty.small).color(c.text_faint)
    } else {
        let _ = s;
        let style = MarkdownStyle::from_kit(k)
            .body_font(TextStyle::new(12.5, 400.0))
            .heading_font(1, TextStyle::new(14.0, 650.0))
            .heading_font(2, TextStyle::new(13.5, 650.0))
            .heading_font(3, TextStyle::new(13.0, 650.0))
            .code_block_font(TextStyle::new(12.0, 400.0))
            .list(16.0, 3.0);
        Markdown::new(source)
            .style(style)
            .on_link(|s: &mut App, url: &str| {
                notify::open_url_in_browser(url, s.allow_os_side_effects)
            })
            .build(k)
    };
    col()
        .key(format!("pr-description-{}", pr.id))
        .w_full()
        .pl(indent)
        .pr(14.0)
        .pb(12.0)
        .child(
            col()
                .w_full()
                .fill(c.layer_02)
                .border(1.0, c.border_base)
                .radius(8.0)
                .px(12.0)
                .py(10.0)
                .child(
                    col()
                        .w_full()
                        .max_h(300.0)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(body),
                ),
        )
}

/// Drops the HTML comments PR templates leave in, and keeps a very long description short.
fn clean_description(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    let trimmed = out.trim();
    match trimmed.char_indices().nth(8000) {
        Some((at, _)) => format!("{}…", &trimmed[..at]),
        None => trimmed.to_owned(),
    }
}

/// What a Quiet ink button shows: at rest it has no fill, only its icon in its colour.
#[derive(Clone, Copy, PartialEq)]
enum Ink {
    Rest,
    /// Waiting for the confirming press, with the share of the wait left.
    Armed(f32),
    /// Switched on (auto-merge): it keeps its tint.
    On,
    Busy,
    Disabled,
    /// Landed `age` seconds ago: it pops once.
    Done(f32),
}

/// Approve and Merge: no fill at rest, a tint under the pointer that deepens as the button sinks,
/// and the colour itself once it is armed or done. Every reward stays inside the button's box.
fn ink_button(
    k: &Kit,
    tone: Color,
    glyph: &'static rune::Icon,
    label: &str,
    ink: Ink,
    key: String,
) -> El {
    let c = &k.c;
    let live = matches!(ink, Ink::Rest | Ink::Armed(_) | Ink::On);
    let (text_ink, icon_ink) = match ink {
        Ink::Rest => (c.text_base, tone),
        Ink::Busy => (c.text_muted, c.text_muted),
        Ink::Disabled => (c.text_faint, c.text_faint),
        Ink::Armed(_) | Ink::On | Ink::Done(_) => (tone, tone),
    };
    let icon = if ink == Ink::Busy {
        rune_kit::Spinner::new().size(14.0).color(c.text_muted).build(k)
    } else {
        rune::icon(glyph, 14.0).color(icon_ink)
    };
    let mut button = row()
        .key(key)
        .role("button")
        .label(label.to_owned())
        .focusable(live)
        .disabled(!live)
        .h(26.0)
        .min_w(92.0)
        .px(10.0)
        .radius(6.0)
        .center()
        .gap(6.0)
        .cursor(if live {
            rune::Cursor::Pointer
        } else {
            rune::Cursor::Default
        })
        .child(icon.a11y_hidden())
        .child(text(label.to_owned()).font(TextStyle::new(12.5, 600.0)).color(text_ink));
    if live {
        button = button
            .hover(tone.alpha(0.12))
            .press(tone.alpha(0.12))
            .press_scale(0.94);
    }
    match ink {
        Ink::Armed(left) => {
            // The wait for the second press drains along the bottom edge.
            button = button.fill(tone.alpha(0.14)).child(
                row()
                    .pin(Some(6.0), None, Some(6.0), Some(2.0))
                    .h(2.0)
                    .instant_changes()
                    .child(el().h_full().w_frac(left.clamp(0.0, 1.0)).radius(1.0).fill(tone)),
            );
        }
        Ink::On => button = button.fill(tone.alpha(0.14)),
        Ink::Done(age) => {
            // A small overshoot from 0.88, once.
            let p = (age / 0.32).clamp(0.0, 1.0) - 1.0;
            let back = 1.0 + 2.7 * p * p * p + 1.7 * p * p;
            button = button
                .scale(0.88 + 0.12 * back)
                .scale_motion(rune::Anim::INSTANT);
        }
        _ => {}
    }
    button
}

/// Merge's violet, lighter on dark so it reads as text.
fn merge_tone(k: &Kit) -> Color {
    oklch(if k.dark { 0.72 } else { 0.56 }, if k.dark { 0.17 } else { 0.2 }, 295.0)
}

fn armed_left(armed: &Option<(String, std::time::Instant)>, id: &str) -> Option<f32> {
    armed
        .as_ref()
        .filter(|(armed, _)| armed == id)
        .map(|(_, at)| 1.0 - at.elapsed().as_secs_f32() / ARMED_SECS)
}

fn approve_control(s: &App, k: &Kit, pr: &PullRequest) -> El {
    let id = pr.id.clone();
    let done = s
        .approved
        .iter()
        .find(|(a, _)| *a == pr.id)
        .map(|(_, at)| at.elapsed().as_secs_f32());
    let busy = s.approving.contains(&pr.id);
    let armed = armed_left(&s.approve_armed, &pr.id);
    let (label, ink) = if let Some(age) = done {
        ("Approved", Ink::Done(age))
    } else if busy {
        ("Approving…", Ink::Busy)
    } else if let Some(left) = armed {
        ("Press again", Ink::Armed(left))
    } else {
        ("Approve", Ink::Rest)
    };
    let mut button = ink_button(
        k,
        mint(k),
        &rune_kit::glyphs::CHECK,
        label,
        ink,
        format!("approve-{}", pr.id),
    );
    if done.is_none() && !busy {
        button = button.on_press(move |s: &mut App| s.press_approve(&id));
    }
    let wrap = row()
        .center_y()
        .hover_group()
        .help(if armed.is_some() {
            format!("Press again to approve {}#{}", pr.repo, pr.number)
        } else {
            "Approve without a comment".to_owned()
        })
        .child(button);
    // It shows while the row is hovered, and stays once it is armed, busy or done.
    if armed.is_some() || busy || done.is_some() {
        wrap
    } else {
        wrap.group_opacity(0.0, 1.0)
    }
}

/// A pull request GitHub merged by itself, kept faded at the bottom of its section until cleared.
fn finished_row(k: &Kit, t: &model::Tracked) -> El {
    let c = &k.c;
    let pr = &t.pr;
    let (label, color) = match t.state {
        model::TrackedState::Merged => ("Auto-merged ✓", mint(k)),
        _ => ("Closed without merging", c.text_muted),
    };
    let id = pr.id.clone();
    row()
        .key(format!("finished-{}", pr.id))
        .w_full()
        .pl(14.0)
        .pr(14.0)
        .py(10.0)
        .center_y()
        .gap(12.0)
        .opacity(0.62)
        .child(rune::icon(&PR_ICON, 16.0).color(c.text_faint))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    text(pr.title.clone())
                        .font(TextStyle::new(14.0, 530.0))
                        .color(c.text_muted)
                        .truncate(),
                )
                .child(
                    row()
                        .center_y()
                        .gap(9.0)
                        .child(text(pr.repo.clone()).font(k.ty.small).color(c.text_muted))
                        .child(
                            text(format!("#{}", pr.number))
                                .font(k.ty.mono)
                                .color(c.text_muted),
                        )
                        .child(el().size(1.0, 10.0).fill(c.border_base))
                        .child(text(label).font(k.ty.small_medium).color(color)),
                ),
        )
        .child(
            icon_action(k.glyph.close, "Clear")
                .on_press(move |s: &mut App| s.clear_tracked(&id))
                .build(k)
                .key(format!("clear-{}", pr.id)),
        )
}

/// The Merge button on one of the user's PRs: off until GitHub says it can merge, armed by the
/// first press, and done once the merge lands, while the row leaves.
fn merge_control(s: &App, k: &Kit, pr: &PullRequest) -> El {
    let tone = merge_tone(k);
    let key = format!("merge-{}", pr.id);
    if let Some((_, at)) = s.celebrations.iter().find(|(id, _)| *id == pr.id) {
        let age = at.elapsed().as_secs_f32();
        return ink_button(k, tone, &rune_kit::glyphs::CHECK, "Merged", Ink::Done(age), key);
    }
    let id = pr.id.clone();
    let ready = pr.merge_readiness.is_ready();
    let merging = s.merging.contains(&pr.id);
    let armed = armed_left(&s.merge_armed, &pr.id);
    let busy = !s.merging.is_empty();
    // Not mergeable yet: GitHub can merge it by itself once it is, which is its own state.
    let auto_on = pr.auto_merge == model::AutoMerge::On;
    let auto_offer = !ready && s.auto_merge_target(&pr.id) == Some(true);
    let auto = auto_on || auto_offer;
    let auto_busy = s.auto_merging.contains(&pr.id);

    let (glyph, label, ink) = if merging {
        (&MERGE, "Merging…", Ink::Busy)
    } else if let Some(left) = armed {
        (&MERGE, "Press again", Ink::Armed(left))
    } else if auto_on {
        (&BOLT, "Auto-merge on", if auto_busy { Ink::Busy } else { Ink::On })
    } else if auto_offer {
        (&BOLT, "Auto-merge", if auto_busy { Ink::Busy } else { Ink::Rest })
    } else if !ready || busy {
        (&MERGE, "Merge", Ink::Disabled)
    } else {
        (&MERGE, "Merge", Ink::Rest)
    };
    let mut button = ink_button(k, tone, glyph, label, ink, key);
    if matches!(ink, Ink::Rest | Ink::Armed(_) | Ink::On) {
        button = button.on_press(move |s: &mut App| {
            if auto {
                s.press_auto_merge(&id)
            } else {
                s.press_merge(&id)
            }
        });
    }

    row()
        .center_y()
        .hover_group()
        .help(if auto_on {
            "Merges by itself as soon as GitHub allows it. Click to turn off".to_owned()
        } else if auto_offer {
            format!(
                "Merge by itself once GitHub allows it ({}). Click to turn on",
                pr.merge_readiness.blocked_reason().to_lowercase()
            )
        } else if armed.is_some() {
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
    let now = current_epoch_hour();
    let attention = s.prs.iter().filter(|p| s.needs_you(p, now)).count();
    doc = doc
        .child(brand(s, k))
        .child(
            row()
                .w_full()
                .justify_between()
                .items_end()
                .child(col().gap(5.0).child(heading(k, "Pull requests")).child(
                    if s.prs.is_empty() {
                        el()
                    } else {
                        text(format!(
                            "{} open  ·  {} need your thumb",
                            s.prs.len(),
                            attention
                        ))
                        .font(k.ty.body)
                        .color(c.text_muted)
                    },
                ))
                .child(if s.filter == InboxFilter::Inbox {
                    sort_control(s, k)
                } else {
                    el()
                }),
        )
        .children((s.pace_enabled && s.page == Page::Inbox).then(|| pace_panel(s, k)))
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
    let mut items: Vec<&PullRequest> = s.prs.iter().filter(|pr| s.in_inbox(pr, now)).collect();
    let group = |pr: &PullRequest| {
        s.action_group_of(
            pr,
            pr.attention(s.draft_stale_hours, s.open_stale_hours, now),
        )
    };
    // A pull request no section claims is not listed.
    items.retain(|pr| group(pr).is_some());
    // Ones auto-merge finished stay at the bottom of their section, faded, until cleared.
    let finished: Vec<&model::Tracked> = s
        .tracked
        .iter()
        .filter(|t| t.state != model::TrackedState::Watching && s.matches_search(&t.pr))
        .collect();
    items.sort_by(|a, b| {
        let rank = |pr: &PullRequest| group(pr).map_or(usize::MAX, |(id, _)| s.group_rank(id));
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.external.cmp(&b.external))
            .then_with(|| a.idle_hours.cmp(&b.idle_hours))
            .then_with(|| a.id.cmp(&b.id))
    });

    if items.is_empty() && finished.is_empty() {
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
    let mut displayed: Vec<(u8, &str)> = Vec::new();
    for pr in shown_items {
        if let Some(g) = group(pr)
            && !displayed.iter().any(|(p, _)| *p == g.0)
        {
            displayed.push(g);
        }
    }
    for t in &finished {
        if let Some(at) = s
            .layout
            .sections
            .iter()
            .position(|sec| sec.name == t.section)
            && !displayed.iter().any(|(p, _)| usize::from(*p) == at)
        {
            displayed.push((at as u8, s.layout.sections[at].name.as_str()));
        }
    }
    displayed.sort_by_key(|(p, _)| s.group_rank(*p));
    // Ignored sits at the bottom and is not dragged, so it is left out of the drag order.
    let order: Vec<u8> = displayed
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| *p != IGNORED_GROUP)
        .collect();
    let group_count = order.len();
    let dragging = s.group_reorder.dragging().is_some();
    for (index, (position, heading)) in displayed.iter().copied().enumerate() {
        let in_group = items
            .iter()
            .filter(|pr| group(pr).is_some_and(|(id, _)| id == position))
            .count();
        let lifted = s.group_reorder.dragging() == Some(index);
        let is_ignored = position == IGNORED_GROUP;
        let folded = if is_ignored {
            !s.ignored_open
        } else {
            dragging || s.collapsed_groups.contains(&position)
        };
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
                        if position == IGNORED_GROUP {
                            s.ignored_open = !s.ignored_open;
                        } else {
                            s.toggle_collapse_group(position);
                        }
                    })
                    .child(
                        row()
                            .center_y()
                            .gap(8.0)
                            .child(if is_ignored { el().size(20.0, 20.0) } else { grip })
                            .child(
                                text(heading)
                                    .font(TextStyle::new(13.0, 650.0))
                                    .color(c.text_muted),
                            )
                            .child(if in_group > 0 {
                                text(in_group.to_string())
                                    .font(k.ty.small)
                                    .color(c.text_faint)
                            } else {
                                el()
                            })
                            .child(if s.smart_active()
                                && s.layout.sections.get(usize::from(position)).is_some_and(sections::Section::uses_model)
                            {
                                rune::icon(&BRAIN, 13.0)
                                    .color(model_ink(k))
                                    .help("Sorted by alien intelligence: a model read each title and description")
                            } else {
                                el()
                            }),
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
                .filter(|pr| group(pr).is_some_and(|(id, _)| id == position))
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
            let mut first = !items
                .iter()
                .any(|pr| group(pr).is_some_and(|(id, _)| id == position));
            for t in finished.iter().filter(|t| t.section == heading) {
                if !first {
                    list = list.child(el().w_full().h(1.0).fill(c.border_weak));
                }
                first = false;
                list = list.child(finished_row(k, t));
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
                s.save_flag(key, value);
                s.trigger_sync();
            })
            .build(k)
            .key(format!("option-{key}")),
    )
}

/// Settings › Autonomous: two switches, by who an action reaches. What each does stays out of
/// sight; the notification after each action says it.
fn autonomy_section(s: &App, k: &Kit) -> El {
    let switch = |label: &'static str, on: bool, branches: bool, key: &'static str| {
        setup_row(
            k,
            label,
            Switch::new(on)
                .label(label)
                .hide_label()
                .on_toggle(move |s: &mut App| s.toggle_autonomy(branches))
                .build(k)
                .key(key),
        )
    };
    col()
        .w_full()
        .gap(10.0)
        .child(text("Autonomous").font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            text("Your own PRs only. You get a notification each time.")
                .wrap()
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(switch(
            "Update branches and auto-merge",
            s.autonomy_branches,
            true,
            "option-autonomy-branches",
        ))
        .child(switch(
            "Re-request reviews after I push",
            s.autonomy_reviewers,
            false,
            "option-autonomy-reviewers",
        ))
}

// --- Pace ---

const PACE_CELL: f32 = 12.0;
const PACE_GAP: f32 = 3.0;
/// The streak, the score beside it, their labels, and the grid's chrome: tabular figures, so a
/// number that rolls keeps its width.
const PACE_HERO: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(52.0, 700.0).lh(56.0).tracking(-1.5)
};
const PACE_NUMBER: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(26.0, 650.0).lh(30.0)
};
const PACE_UNIT: TextStyle = TextStyle::new(16.0, 550.0).lh(30.0);
const PACE_LABEL: TextStyle = TextStyle::new(11.0, 500.0);
const PACE_SUB: TextStyle = TextStyle::new(11.0, 400.0);
const PACE_CHROME: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(10.0, 500.0)
};

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The grid's ink for a level: a quiet mint ramp that only reaches the button's mint at the top.
fn pace_ink(k: &Kit, level: u8) -> Color {
    let ramp = if k.dark {
        [(0.34, 0.035), (0.47, 0.06), (0.63, 0.085), (0.83, 0.10)]
    } else {
        [(0.90, 0.04), (0.78, 0.075), (0.62, 0.10), (0.44, 0.10)]
    };
    match level {
        0 => k.c.text_base.alpha(if k.dark { 0.07 } else { 0.06 }),
        n => {
            let (l, chroma) = ramp[usize::from(n.min(4)) - 1];
            oklch(l, chroma, 161.0)
        }
    }
}

/// One day of the grid, with what was cleared in its tooltip. A day of only bold approvals is
/// drawn hollow; today is ringed, dashed while it still waits for its first one.
fn pace_cell(k: &Kit, stats: &pace::Stats, day: &pace::Day, size: f32, today: Option<chrono::NaiveDate>) -> El {
    let c = &k.c;
    let Some(date) = day.date else {
        return el().size(size, size);
    };
    let mut done = Vec::new();
    if day.approvals > 0 {
        done.push(plural(day.approvals, "approval", "approvals"));
    }
    if day.merges > 0 {
        done.push(plural(day.merges, "merge", "merges"));
    }
    let mut what = if done.is_empty() {
        "nothing cleared".to_owned()
    } else {
        done.join(", ")
    };
    if day.quick > 0 {
        what.push_str(&format!(", {} quick", day.quick));
    }
    if day.bold > 0 {
        what.push_str(&format!(", {} bold", day.bold));
    }
    let mut cell = el()
        .size(size, size)
        .radius(size / 4.0)
        .help(format!("{} · {what}", date.format("%a %-d %b")));
    cell = if day.only_bold() {
        cell.border(1.0, c.text_muted.alpha(0.6)).border_dash(2.0, 1.5)
    } else {
        cell.fill(pace_ink(k, stats.level(day)))
    };
    if Some(date) == today {
        cell = if day.approvals + day.merges == 0 {
            cell.border(1.0, c.text_muted).border_dash(2.0, 2.0)
        } else {
            let lit = cell.border(1.5, c.text_base.alpha(0.9));
            if k.dark {
                lit.shadow(0.0, 8.0, mint(k).alpha(0.45))
            } else {
                lit
            }
        };
    }
    cell
}

/// The count that is landing right now, how long ago it landed, and the score as it stood before.
fn pace_landing(s: &App, now: chrono::DateTime<chrono::FixedOffset>) -> Option<(f32, &pace::Event, pace::Stats)> {
    let (event, at) = s.pace_flash.iter().max_by_key(|(_, at)| *at)?;
    let age = at.elapsed().as_secs_f32();
    if age > 1.0 {
        return None;
    }
    let before: Vec<pace::Event> = s
        .pace_events
        .iter()
        .filter(|e| !(e.at == event.at && e.pr_id == event.pr_id && e.kind == event.kind))
        .cloned()
        .collect();
    Some((age, event, pace::Stats::new(&before, now)))
}

/// A number that rolls up from what it was, like an odometer, while a count lands.
fn rolling(before: Option<String>, after: String, style: TextStyle, tone: Color, age: f32) -> El {
    let p = ease_out(phase(age, 0.2, 0.48));
    match before.filter(|b| *b != after && p < 1.0) {
        None => text(after).font(style).color(tone),
        Some(before) => col().h(style.line_height).clip_rounded(0.0).child(
            col()
                .offset(0.0, -style.line_height * p)
                .offset_motion(rune::Anim::INSTANT)
                .child(text(before).font(style).color(tone))
                .child(text(after).font(style).color(tone)),
        ),
    }
}

/// "53m" as a number and its unit.
fn split_unit(value: &str) -> (String, String) {
    let at = value
        .find(|ch: char| ch.is_alphabetic())
        .unwrap_or(value.len());
    (value[..at].to_owned(), value[at..].to_owned())
}

/// One column of the score: the number, what it counts, and over when.
fn pace_column(k: &Kit, value: El, label: &str, sub: El) -> El {
    col()
        .gap(3.0)
        .child(value)
        .child(text(label.to_owned()).font(PACE_LABEL).color(k.c.text_muted))
        .child(sub)
}

/// The newest count of this kind that beat the week's fastest, if one is still being celebrated:
/// how long ago it landed, and its wait.
fn pace_record(s: &App, kind: pace::Kind) -> Option<(f32, i64)> {
    s.pace_flash
        .iter()
        .filter(|(e, _)| e.kind == kind && pace::new_best(&s.pace_events, e))
        .map(|(e, at)| (at.elapsed().as_secs_f32(), e.wait_minutes.unwrap_or_default()))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Above the Inbox: the streak, this week's quick approvals and merges, and a grid of the last
/// weeks, GitHub-style. Click the header to fold it to one line; the fold is remembered.
fn pace_panel(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let now = chrono::Local::now().fixed_offset();
    let today = now.date_naive();
    let stats = pace::Stats::new(&s.pace_events, now);
    let landing = pace_landing(s, now);
    let age = landing.as_ref().map_or(f32::MAX, |(age, _, _)| *age);
    let before = |f: &dyn Fn(&pace::Stats) -> String| landing.as_ref().map(|(_, _, b)| f(b));
    let collapsed = s.pace_collapsed;
    let days: Vec<&pace::Day> = stats.weeks.iter().flatten().filter(|d| d.date.is_some()).collect();
    let cleared_today = days.last().is_some_and(|d| d.approvals + d.merges > 0);

    let caption = if collapsed {
        let quick = stats.quick_approvals + stats.quick_merges;
        let mut summary = Vec::new();
        if stats.streak > 0 {
            summary.push(format!("{}-day streak", stats.streak));
        }
        if quick > 0 {
            summary.push(format!("{quick} quick this week"));
        }
        if summary.is_empty() {
            summary.push("Approve or merge from here to start a streak".to_owned());
        }
        text(summary.join("  ·  ")).font(k.ty.small).color(c.text_muted)
    } else if stats.bold > 0 {
        row()
            .center_y()
            .gap(5.0)
            .child(rune::icon(&EYE, 12.0).color(c.warning.fg).a11y_hidden())
            .child(
                text(format!(
                    "{} this week. Faster than they read; no medal.",
                    plural(stats.bold, "bold approval", "bold approvals")
                ))
                .font(k.ty.small)
                .color(c.text_faint),
            )
    } else {
        text("Quick: within the hour of being asked, or of the approval for a merge")
            .font(k.ty.small)
            .color(c.text_faint)
    };
    // Folded, the last week as a strip, with the streak's days joined into one bar.
    let week = &days[days.len().saturating_sub(7)..];
    let active = |i: usize| week.get(i).is_some_and(|d| d.approvals + d.merges > 0);
    let mut strip = row().center_y();
    for (i, day) in week.iter().enumerate() {
        let (joined_left, joined_right) = (
            i > 0 && active(i) && active(i - 1),
            active(i) && active(i + 1),
        );
        if i > 0 && !joined_left {
            strip = strip.child(el().w(2.0));
        }
        let r = 2.5;
        strip = strip.child(pace_cell(k, &stats, day, 9.0, None).corners([
            if joined_left { 0.0 } else { r },
            if joined_right { 0.0 } else { r },
            if joined_right { 0.0 } else { r },
            if joined_left { 0.0 } else { r },
        ]));
    }

    let header = row()
        .key("pace-header")
        .w_full()
        .center_y()
        .justify_between()
        .hover_group()
        .role("button")
        .label(if collapsed { "Show Pace" } else { "Hide Pace" })
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            tx.motion_curve(rune::Curve::tween(0.16, rune::Easing::cubic(0.2, 0.0, 0.0, 1.0)));
            s.toggle_pace_collapsed();
        })
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(rune::icon(&BOLT, 14.0).color(mint(k)).a11y_hidden())
                .child(text("Pace").font(k.ty.ui_medium).color(c.text_base))
                .child(el().size(1.0, 10.0).fill(c.border_base))
                .child(caption),
        )
        .child(
            row()
                .center_y()
                .gap(12.0)
                .child(if collapsed { strip } else { el() })
                .child(
                    rune::icon(k.glyph.chevron_down, 14.0)
                        .color(c.text_muted)
                        .rotate(if collapsed { -std::f32::consts::FRAC_PI_2 } else { 0.0 })
                        .group_opacity(0.5, 1.0)
                        .a11y_hidden(),
                ),
        );
    let card = col()
        .key("pace-panel")
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(6.0)
        .radius(9.0)
        .border(1.0, c.border_weak)
        .fill(surface(k))
        .child(header);
    if collapsed {
        return card;
    }

    // The score: four columns on one baseline. The streak waits in grey until today has one.
    let streak_tone = match (stats.streak, cleared_today) {
        (0, _) => c.text_faint,
        (_, false) => c.text_muted,
        _ => c.text_base,
    };
    let hero = pace_column(
        k,
        rolling(
            before(&|b| b.streak.to_string()),
            stats.streak.to_string(),
            PACE_HERO,
            streak_tone,
            age,
        ),
        "day streak",
        text(if stats.best_streak > 0 {
            format!("best {}", stats.best_streak)
        } else {
            "clear one to start".to_owned()
        })
        .font(PACE_SUB)
        .color(c.text_faint),
    );
    let number = |value: String, was: Option<String>, tone: Color| {
        let (n, unit) = split_unit(&value);
        let was = was.map(|w| split_unit(&w).0);
        row()
            .items_end()
            .gap(1.0)
            // The hero's baseline sits this much higher in its taller line.
            .pb(5.0)
            .child(rolling(was, n, PACE_NUMBER, tone, age))
            .child(if unit.is_empty() {
                el()
            } else {
                text(unit).font(PACE_UNIT).color(c.text_muted)
            })
    };
    let tile = |value: El, label: &str, sub: El| {
        pace_column(k, value, label, sub).grow(1.0).min_w(92.0)
    };
    // Under each count, the week's fastest; while a new one is celebrated, it says so in its ink.
    let fastest = |best: Option<i64>, kind: pace::Kind, tone: Color| match (pace_record(s, kind), best) {
        (Some((_, wait)), _) => row()
            .center_y()
            .gap(4.0)
            .child(rune::icon(&TROPHY, 11.0).color(tone).a11y_hidden())
            .child(
                text(format!("new best · {}", pace::span(wait)))
                    .font(TextStyle::new(11.0, 600.0))
                    .color(tone),
            ),
        (None, Some(wait)) => text(format!("fastest {} · 7 days", pace::span(wait)))
            .font(PACE_SUB)
            .color(c.text_faint),
        (None, None) => text("last 7 days").font(PACE_SUB).color(c.text_faint),
    };
    let divider = || el().size(1.0, 40.0).fill(c.border_weak).self_center();
    let median = |st: &pace::Stats| st.median_merge_minutes.map_or("–".to_owned(), pace::span);
    let score = row()
        .grow(1.0)
        .min_w(0.0)
        .items_end()
        .gap(16.0)
        .pt(18.0)
        .pb(16.0)
        .child(hero)
        .child(divider())
        .child(tile(
            number(
                stats.quick_approvals.to_string(),
                before(&|b| b.quick_approvals.to_string()),
                if stats.quick_approvals > 0 { mint(k) } else { c.text_faint },
            ),
            "quick approvals",
            fastest(stats.fastest_approval, pace::Kind::Approve, mint(k)),
        ))
        .child(divider())
        .child(tile(
            number(
                stats.quick_merges.to_string(),
                before(&|b| b.quick_merges.to_string()),
                if stats.quick_merges > 0 { merge_tone(k) } else { c.text_faint },
            ),
            "quick merges",
            fastest(stats.fastest_merge, pace::Kind::Merge, merge_tone(k)),
        ))
        .child(divider())
        .child(tile(
            number(median(&stats), before(&median), c.text_base),
            "approval to merge",
            text("median, 30 days").font(PACE_SUB).color(c.text_faint),
        ));

    // The grid: weeks left to right, Monday on top, months named where they start.
    let step = PACE_CELL + PACE_GAP;
    let grid_w = pace::WEEKS as f32 * step - PACE_GAP;
    let mut months = el().w(grid_w).h(13.0);
    let mut last_month = None;
    for (w, week) in stats.weeks.iter().enumerate() {
        let Some(date) = week[0].date else { continue };
        let month = chrono::Datelike::month(&date);
        // Named on the first full week of a month, and not squeezed against the right edge.
        if last_month != Some(month)
            && w + 2 < pace::WEEKS
            && (last_month.is_some() || chrono::Datelike::day(&date) <= 7)
        {
            months = months.child(
                text(date.format("%b").to_string())
                    .font(PACE_CHROME)
                    .color(c.text_faint)
                    .absolute(w as f32 * step, 0.0),
            );
        }
        last_month = Some(month);
    }
    let weekdays = (0..7).fold(col().gap(PACE_GAP).w(22.0), |col, d| {
        col.child(
            row().h(PACE_CELL).center_y().child(
                text(match d {
                    0 => "Mon",
                    2 => "Wed",
                    4 => "Fri",
                    _ => "",
                })
                .font(PACE_CHROME)
                .color(c.text_faint),
            ),
        )
    });
    // What just landed: today's cell pops and sends out one ring, for a quick one only.
    let ripple = landing
        .as_ref()
        .filter(|(age, event, _)| *age < 0.95 && event.verdict == pace::Verdict::Quick)
        .map(|(age, event, _)| {
            let tone = if event.kind == pace::Kind::Merge { merge_tone(k) } else { mint(k) };
            (*age, tone)
        });
    let mut cells = row().gap(PACE_GAP);
    for (w, week) in stats.weeks.iter().enumerate() {
        let mut column = col().gap(PACE_GAP);
        for (d, day) in week.iter().enumerate() {
            let mut cell = pace_cell(k, &stats, day, PACE_CELL, Some(today))
                .key(format!("pace-day-{w}-{d}"))
                // Unfolding, the grid washes in from the oldest week.
                .transition(rune::Transition::scale(0.6))
                .enter_delay(w as f32 * 0.012 + d as f32 * 0.008);
            if let (Some((age, tone)), Some(true)) = (ripple, day.date.map(|date| date == today)) {
                let pop = phase(age, 0.35, 0.8);
                let ring = ease_out(phase(age, 0.35, 0.95));
                cell = el()
                    .size(PACE_CELL, PACE_CELL)
                    .raise()
                    .child(
                        cell.absolute(0.0, 0.0)
                            .scale(1.0 + 0.3 * (pop * std::f32::consts::PI).sin())
                            .scale_motion(rune::Anim::INSTANT),
                    )
                    .child(
                        el().size(PACE_CELL, PACE_CELL)
                            .radius(PACE_CELL / 4.0)
                            .border(1.5, tone)
                            .absolute(0.0, 0.0)
                            .scale(1.0 + 1.8 * ring)
                            .scale_motion(rune::Anim::INSTANT)
                            .opacity(if age < 0.35 { 0.0 } else { 0.6 * (1.0 - ring) })
                            .opacity_motion(rune::Anim::INSTANT),
                    );
            }
            column = column.child(cell);
        }
        cells = cells.child(column);
    }
    let legend = (0..5u8)
        .fold(
            row()
                .gap(PACE_GAP)
                .center_y()
                .group_opacity(0.0, 1.0)
                .child(text("Less").font(PACE_CHROME).color(c.text_faint).pr(3.0)),
            |legend, n| legend.child(el().size(10.0, 10.0).radius(2.5).fill(pace_ink(k, n))),
        )
        .child(text("More").font(PACE_CHROME).color(c.text_faint).pl(3.0));
    let grid = col()
        .gap(4.0)
        .hover_group()
        .child(row().gap(6.0).child(el().w(22.0)).child(months))
        .child(row().gap(6.0).child(weekdays).child(cells))
        .child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .pl(28.0)
                .child(
                    text(format!("{} cleared in {} weeks", stats.total, pace::WEEKS))
                        .font(PACE_CHROME)
                        .color(c.text_faint),
                )
                .child(legend),
        );
    card.child(
        row()
            .key("pace-body")
            .w_full()
            .gap(28.0)
            .center_y()
            .transition(rune::Transition::blur(4.0))
            .child(score)
            .child(grid),
    )
}

/// On a row whose approval or merge was just counted: how quick it was, or an outline and a
/// raised eyebrow for a big one approved faster than it reads. It rises in sharp, a quick one
/// catches the light once, and it drifts off.
fn pace_pill(s: &App, k: &Kit, pr: &PullRequest) -> Option<El> {
    let c = &k.c;
    let (event, age) = s.pace_flash_of(&pr.id)?;
    let record = pace::new_best(&s.pace_events, event);
    let wait = event.wait_minutes.map(pace::span).unwrap_or_default();
    let rise = ease_out(phase(age, 0.0, 0.35));
    let leave = phase(age, PACE_FLASH_SECS - 0.5, PACE_FLASH_SECS);
    let pill = row()
        .key(format!("pace-pill-{}", pr.id))
        .h(22.0)
        .px(8.0)
        .radius(11.0)
        .clip_rounded(11.0)
        .gap(4.0)
        .center_y()
        .opacity(rise * (1.0 - leave))
        .opacity_motion(rune::Anim::INSTANT)
        .offset(0.0, (1.0 - rise) * 8.0 - 4.0 * leave)
        .offset_motion(rune::Anim::INSTANT)
        .scale(0.92 + 0.08 * rise)
        .scale_motion(rune::Anim::INSTANT)
        .blur(6.0 * (1.0 - rise) + 4.0 * leave)
        .blur_motion(rune::Anim::INSTANT);
    let label = |s: String, tone: Color| text(s).font(TextStyle::new(11.5, 600.0)).color(tone);
    Some(match event.verdict {
        // A new best is celebrated even when it was not within the hour.
        pace::Verdict::Quick | pace::Verdict::Steady
            if event.verdict == pace::Verdict::Quick || record =>
        {
            let tone = if event.kind == pace::Kind::Merge { merge_tone(k) } else { mint(k) };
            let mut pill = pill
                .fill(tone.alpha(0.14))
                .border_gradient(1.0, tone.alpha(0.35), tone.alpha(0.10))
                .help(if record {
                    "Faster than any other this week"
                } else if event.kind == pace::Kind::Merge {
                    "Merged within the hour of it being ready"
                } else {
                    "Approved within the hour of being asked"
                })
                .child(rune::icon(if record { &TROPHY } else { &BOLT }, 12.0).color(tone).a11y_hidden())
                .child(label(
                    format!("{} · {wait}", if record { "New best" } else { "Quick" }),
                    tone,
                ));
            let sweep = phase(age, 0.30, 0.85);
            if sweep > 0.0 && sweep < 1.0 {
                let shine = c.white.alpha(if k.dark { 0.22 } else { 0.55 });
                pill = pill.child(
                    el().size(36.0, 22.0)
                        .absolute(0.0, 0.0)
                        .offset(-36.0 + 216.0 * ease_out(sweep), 0.0)
                        .offset_motion(rune::Anim::INSTANT)
                        .linear_gradient(
                            std::f32::consts::FRAC_PI_2,
                            [(0.0, shine.alpha(0.0)), (0.5, shine), (1.0, shine.alpha(0.0))],
                        ),
                );
            }
            pill
        }
        pace::Verdict::Bold => {
            let lines = if event.lines >= 1000 {
                format!("{:.1}k", f64::from(event.lines) / 1000.0)
            } else {
                event.lines.to_string()
            };
            let blink = 1.0 - 0.8 * (phase(age, 0.5, 0.7) * std::f32::consts::PI).sin();
            pill.fill(c.text_base.alpha(0.06))
                .border(1.0, c.text_muted.alpha(0.5))
                .border_dash(3.0, 2.0)
                .help("Faster than anyone reads that much. Counted, no medal")
                .child(
                    rune::icon(&EYE, 12.0)
                        .color(c.warning.fg)
                        .opacity(blink)
                        .opacity_motion(rune::Anim::INSTANT)
                        .a11y_hidden(),
                )
                .child(label(format!("{lines} lines in {wait}"), c.text_muted))
        }
        pace::Verdict::Quick | pace::Verdict::Steady => return None,
    })
}

/// Settings › Pace: one switch for the whole thing.
fn pace_section(s: &App, k: &Kit) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(text("Pace").font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            text("Keeps score of approvals and merges you make from here, and celebrates the quick ones. A big pull request approved faster than it reads earns no medal.")
                .wrap()
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(setup_row(
            k,
            "Show my pace",
            Switch::new(s.pace_enabled)
                .label("Show my pace")
                .hide_label()
                .on_toggle(|s: &mut App| s.toggle_pace())
                .build(k)
                .key("option-pace"),
        ))
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

    let notifications_row = setup_row(
        k,
        "Show notifications",
        row().center_y().gap(10.0).child(
            Switch::new(s.native_notifications)
                .label("Show notifications")
                .hide_label()
                .on_toggle(|s: &mut App| {
                    s.native_notifications = !s.native_notifications;
                    if s.native_notifications {
                        s.send_welcome_notification();
                    }
                    s.save_flag("native_notifications", s.native_notifications);
                    s.next_fast_poll_at = 0.0;
                })
                .build(k),
        ),
    );

    let thresholds_section = col()
        .w_full()
        .gap(10.0)
        .child(
            text("Notifications")
                .font(k.ty.ui_medium)
                .color(c.text_base),
        )
        .child(notifications_row)
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
    let ai_field = |label: &'static str, input: El| setup_row(k, label, input);
    let ai_section = col()
        .w_full()
        .gap(10.0)
        .child(text("AI").font(k.ty.ui_medium).color(c.text_base))
        .child(
            text("When on, the description and changed file paths of every PR that asks for your review, private repositories included, are sent to the model at this URL. Off, only rules sort.")
                .wrap()
                .font(k.ty.small)
                .color(c.text_faint),
        )
        .child(setup_row(
            k,
            "Use AI sorting",
            Switch::new(s.ai_enabled)
                .label("Use AI sorting")
                .hide_label()
                .on_toggle(|s: &mut App| s.toggle_ai())
                .build(k)
                .key("option-ai-enabled"),
        ))
        .child(ai_field(
            "API URL",
            TextInput::new(&s.ai_url)
                .label("AI API URL")
                .disabled(!s.ai_enabled)
                .placeholder("https://openrouter.ai/api/alpha/decisions")
                .width(Some(360.0))
                .on_edit(|s: &mut App| &mut s.ai_url)
                .build(k)
                .key("option-ai-url"),
        ))
        .child(ai_field(
            "Model",
            TextInput::new(&s.ai_model)
                .label("AI model")
                .disabled(!s.ai_enabled)
                .placeholder("typesafe/jev-1.13")
                .width(Some(360.0))
                .on_edit(|s: &mut App| &mut s.ai_model)
                .build(k)
                .key("option-ai-model"),
        ))
        .child(ai_field(
            "API key",
            TextInput::new(&s.ai_key)
                .label("AI API key")
                .disabled(!s.ai_enabled)
                .placeholder("sk-…")
                .secure()
                .width(Some(360.0))
                .on_edit(|s: &mut App| &mut s.ai_key)
                .build(k)
                .key("option-ai-key"),
        ));

    let doc = col()
        .key("settings-column")
        .w_full()
        .max_w(740.0)
        .pb(16.0)
        .gap(12.0)
        .child(panel(k).child(default_filter_row(s, k)))
        .child(
            panel(k)
                .child(
                    text("Pull requests")
                        .font(k.ty.ui_medium)
                        .color(c.text_base),
                )
                .child(setup_row(
                    k,
                    "Show team review requests in the Inbox",
                    Switch::new(s.show_team_reviews)
                        .label("Show team review requests in the Inbox")
                        .hide_label()
                        .on_toggle(|s: &mut App| s.toggle_show_team_reviews())
                        .build(k)
                        .key("option-show-team-reviews"),
                ))
                .child(setup_row(
                    k,
                    "Show outside contributors in the Inbox",
                    Switch::new(!s.hide_external)
                        .label("Show outside contributors in the Inbox")
                        .hide_label()
                        .on_toggle(|s: &mut App| s.toggle_hide_external())
                        .build(k)
                        .key("option-hide-external"),
                ))
                .child(sync_option_row(
                    k,
                    "Show archived repositories",
                    !s.sync_options.hide_archived,
                    "hide_archived",
                    |o| &mut o.hide_archived,
                ))
                .child(setup_row(
                    k,
                    "Show PRs opened in the last",
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
                                .label("Show PRs opened in the last (days)")
                                .width(Some(64.0))
                                .on_edit(|s: &mut App| &mut s.max_age_input)
                                .build(k)
                                .key("option-max-age-days"),
                        ),
                )),
        )
        .child(panel(k).child(autonomy_section(s, k)))
        .child(panel(k).child(pace_section(s, k)))
        .children(s.ai_feature.then(|| panel(k).child(ai_section)))
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

/// The release this app was built as. A packaged app carries it in its Info.plist (the release tag,
/// which can be ahead of Cargo.toml); a `cargo run` build says so.
fn app_version() -> String {
    let from_bundle = std::env::current_exe().ok().and_then(|exe| {
        let plist = std::fs::read_to_string(exe.parent()?.parent()?.join("Info.plist")).ok()?;
        let after = plist
            .split("<key>CFBundleShortVersionString</key>")
            .nth(1)?;
        let value = after.split("<string>").nth(1)?.split("</string>").next()?;
        Some(value.trim().to_owned())
    });
    from_bundle.unwrap_or_else(|| format!("{} (dev build)", env!("CARGO_PKG_VERSION")))
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
                    row()
                        .center_y()
                        .gap(10.0)
                        .child(
                            text("Settings")
                                .font(TextStyle::new(22.0, 650.0))
                                .color(k.c.text_base),
                        )
                        .child(
                            text(format!("v{}", app_version()))
                                .font(TextStyle::new(11.0, 450.0))
                                .color(k.c.text_faint)
                                .key("app-version"),
                        ),
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

    let intro = s
        .intro
        .filter(|_| s.page == Page::Onboarding && !cx.is_reduced_motion())
        .map(|at| at.elapsed().as_secs_f32());
    let body = match s.page {
        Page::Onboarding => onboarding_page_view(s, &k, intro),
        Page::Inbox => document_inbox_view(s, &k),
    };

    let root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(top_bar(s, &k, intro.is_some()))
        .child(body.grow(1.0).min_h(0.0));
    let root = match intro {
        Some(t) => {
            let window = cx
                .layout_size("root")
                .map_or((920.0, 680.0), |[w, h]| (w, h));
            let name = cx
                .layout_size("intro-name")
                .map_or((340.0, 77.0), |[w, h]| (w, h));
            root.child(intro_view(s, &k, t, window, name).pin(
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
            ))
        }
        None => root,
    };
    if s.settings_open && s.page == Page::Inbox {
        root.child(settings_modal(s, &k))
    } else if s.add_repo_open && s.page == Page::Inbox {
        root.child(repositories_modal(s, &k))
    } else {
        root
    }
}

fn on_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key == rune::Key::Escape && s.intro.is_some() {
        s.intro = None;
        return true;
    }
    if ev.key == rune::Key::Escape && s.approve_armed.is_some() {
        s.approve_armed = None;
        return true;
    }
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

/// Eighteen weeks of made-up approvals and merges for the preview scenes: busy on weekdays,
/// quiet at weekends, and a streak running into today.
fn demo_pace_events() -> Vec<pace::Event> {
    let now = chrono::Utc::now().timestamp();
    let mut out = Vec::new();
    for back in (0..pace::WEEKS as i64 * 7).rev() {
        let h = (back as u64).wrapping_mul(2_654_435_761) % 97;
        let date = chrono::Local::now().date_naive() - chrono::Days::new(back as u64);
        let weekend = chrono::Datelike::weekday(&date).number_from_monday() >= 6;
        let count = match back {
            0..=3 => 2 + h % 2,
            4 => 0,
            _ if weekend => u64::from(h.is_multiple_of(9)),
            _ => h % 6,
        };
        for i in 0..count {
            // The last week is the one the panel counts: mostly quick, one bold.
            let (merge, quick) = if back < 7 {
                (i == 1, i == 1 || (back as u64 + i) % 3 != 2)
            } else {
                ((h + i).is_multiple_of(3), !(h + i).is_multiple_of(4))
            };
            let wait = if quick { 4 + (h * 7 + i * 13) as i64 % 50 } else { 90 + h as i64 * 11 };
            out.push(pace::Event {
                at: now - back * 86_400 - i as i64 * 1800,
                kind: if merge { pace::Kind::Merge } else { pace::Kind::Approve },
                pr_id: format!("acme/demo#{back}{i}"),
                wait_minutes: Some(wait),
                lines: 40 + (h * 3) as u32,
                verdict: if !merge && back == 2 && i == 0 {
                    pace::Verdict::Bold
                } else if quick {
                    pace::Verdict::Quick
                } else {
                    pace::Verdict::Steady
                },
            });
        }
    }
    out
}

fn seed_preview(s: &mut App) {
    s.loading = false;
    s.pace_events = demo_pace_events();
    s.ai_feature = true;
    s.ai_enabled = true;
    s.sync_status = SyncStatus::Connected {
        username: "octocat".to_string(),
    };
    s.prs = github::demo_prs();
    if let Some(pr) = s.prs.iter_mut().find(|p| p.id == "acme/engine#418") {
        pr.description = "## What\n\nBinding the offscreen `Rgba16Float` layer once per pinch frame instead of once per draw. **Cuts about 40%** of encoder time on a 3-finger pinch.\n\n<!-- template: link the issue -->\n\n- coalesce binds in `metal/layer.rs`\n- keep the bind cache across passes\n- add a pinch trace to the bench\n\nFixes [#402](https://github.com/acme/engine/issues/402)".into();
    }
    s.repos = github::infer_repos_from_prs(&s.prs, &[]);
    s.last_synced = Some(std::time::Instant::now());
    // What a model would have said about some of the demo review requests, and a rule about one.
    let model = "gpt-4o-mini";
    for (id, reading, urgent, by) in [
        ("acme/engine#418", 0.5, 0.83, model),
        ("acme/cli#904", 0.9, 0.04, model),
        ("acme/cli#910", 0.03, 0.03, "rules"),
        ("acme/engine#920", 0.45, 0.05, model),
        ("acme/cli#931", 0.05, 0.02, model),
    ] {
        s.judgments.insert(
            id.to_owned(),
            judge::Judgment {
                answers: [("reading", reading), ("urgent", urgent)]
                    .into_iter()
                    .map(|(q, p)| (q.to_owned(), p))
                    .collect(),
                by: by.into(),
                fingerprint: String::new(),
            },
        );
    }
}

/// `meat-proxy --judge-eval`: fetch the real pull requests (read only), judge them the way the app
/// does, and print what each would get. Point MEAT_PROXY_DB at a copy of the database.
fn judge_eval() {
    // It sends what the app would: only with the feature shown and AI sorting switched on.
    if !judge::feature_enabled() {
        return eprintln!("The AI feature is hidden: run with MEAT_PROXY_AI=1.");
    }
    if !Database::open_default()
        .and_then(|db| db.load_snapshot())
        .is_ok_and(|snap| snap.ai_enabled)
    {
        return eprintln!("AI sorting is off: switch it on in Settings › AI first.");
    }
    let Some(mut cfg) = judge::JudgeConfig::load() else {
        eprintln!("No judge config: put an api_key in ~/.config/meat-proxy/judge.json");
        return;
    };
    cfg.max_per_sync = 100;
    let layout = sections::Layout::default();
    let questions = layout.needed_questions();
    println!(
        "{} sections; questions asked: {}",
        layout.sections.len(),
        questions
            .iter()
            .map(|q| q.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let snap = match Database::open_default().and_then(|db| db.load_snapshot()) {
        Ok(snap) => snap,
        Err(error) => return eprintln!("Could not read the saved data: {error}"),
    };
    let started = std::time::Instant::now();
    let out = match github::fetch_live(&snap.repos, &snap.rules, snap.sync_options) {
        Ok(out) => out,
        Err(error) => return eprintln!("Sync failed: {error}"),
    };
    println!(
        "synced {} pull requests in {:.1}s; model {} via {}",
        out.prs.len(),
        started.elapsed().as_secs_f32(),
        cfg.model,
        cfg.base_url
    );
    let now = current_epoch_hour();
    let attention =
        |pr: &PullRequest| pr.attention(snap.draft_stale_hours, snap.open_stale_hours, now);
    let mut cache = std::collections::HashMap::new();
    let plan = judge::plan(&out.prs, attention, &cache, &cfg, &questions);
    let candidates = plan.cached + plan.settled.len() + plan.to_ask.len() + plan.deferred;
    println!(
        "{} need a judgment: {} answered by rules, {} for the model ({} deferred by the cap)",
        candidates,
        plan.settled.len(),
        plan.to_ask.len(),
        plan.deferred
    );
    cache.extend(plan.settled);
    let asked = plan.to_ask.len();
    let started = std::time::Instant::now();
    let batch = judge::run_batch(&cfg, &questions, plan.to_ask, 0.0);
    println!(
        "{} calls in {:.1}s ({} answered): {} prompt + {} completion tokens, ${:.5}{}",
        asked,
        started.elapsed().as_secs_f32(),
        batch.judged.len(),
        batch.usage.prompt_tokens,
        batch.usage.completion_tokens,
        batch.usage.cost_usd,
        if batch.fatal { "  STOPPED EARLY" } else { "" }
    );
    for error in batch.errors.iter().take(5) {
        println!("  error: {error}");
    }
    cache.extend(batch.judged);

    let again = judge::plan(&out.prs, attention, &cache, &cfg, &questions);
    println!(
        "second pass: {} calls, {} answered from the cache\n",
        again.to_ask.len(),
        again.cached
    );

    let mut rows: Vec<_> = out
        .prs
        .iter()
        .filter_map(|pr| cache.get(&pr.id).map(|j| (pr, j)))
        .map(|(pr, j)| {
            let at = layout.section_of(&sections::Subject {
                pr,
                attention: attention(pr),
                answers: &j.answers,
            });
            (at, pr, j)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    println!(
        "{:<18} {:<28} {:<4} {:<14} title",
        "section", "answers", "ext", "author"
    );
    for (at, pr, j) in rows {
        let section = at.map_or("(none)", |i| layout.sections[i].name.as_str());
        let mut answers: Vec<_> = j.answers.iter().collect();
        answers.sort_by(|a, b| a.0.cmp(b.0));
        let odds: Vec<String> = answers
            .iter()
            .map(|(id, p)| format!("{}{:.0}", id.chars().next().unwrap_or('?'), **p * 100.0))
            .collect();
        let title: String = pr.title.chars().take(64).collect();
        println!(
            "{:<18} {:<28} {:<4} {:<14} {}   [{}]",
            section,
            odds.join(" "),
            if pr.external { "ext" } else { "" },
            pr.author.chars().take(14).collect::<String>(),
            title,
            j.by
        );
    }
}

/// AppKit shows the tooltips and waits about a second first. Ask for a quicker one, unless the
/// user set `NSInitialToolTipDelay` for this app themselves (`defaults write`).
#[cfg(target_os = "macos")]
fn show_tooltips_sooner() {
    use objc2_foundation::{NSString, NSUserDefaults};
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("NSInitialToolTipDelay");
    if defaults.objectForKey(&key).is_none() {
        defaults.setInteger_forKey(TOOLTIP_DELAY_MS, &key);
    }
}

#[cfg(not(target_os = "macos"))]
fn show_tooltips_sooner() {}

/// How long the pointer rests on something before its tooltip appears.
const TOOLTIP_DELAY_MS: isize = 200;

fn main() {
    if std::env::args().any(|a| a == "--judge-eval") {
        return judge_eval();
    }
    show_tooltips_sooner();
    rune::prewarm_gpu();
    let chrome = Chrome {
        full_size_content: true,
        traffic_lights: Some((16.0, 18.0)),
        drag_regions: true,
    };
    type Scene = (&'static str, fn(&mut App));
    let scenes: &[Scene] = &[
        ("onboarding", |s| {
            s.loading = false;
            s.sync_status = SyncStatus::NeedsAuth {
                reason: "GitHub CLI is signed out.".to_string(),
            };
        }),
        ("signed-in", seed_preview),
        // The intro as the finger lands, and once the name is up.
        ("intro-press", |s| {
            s.loading = false;
            s.intro = Some(
                std::time::Instant::now() - std::time::Duration::from_secs_f32(INTRO_HIT + 0.12),
            );
        }),
        ("intro-dock", |s| {
            s.loading = false;
            s.intro = Some(std::time::Instant::now() - std::time::Duration::from_secs_f32(4.45));
        }),
        ("intro-name", |s| {
            s.loading = false;
            s.intro = Some(std::time::Instant::now() - std::time::Duration::from_secs_f32(3.9));
        }),
        ("inbox", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
        }),
        ("inbox-open", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.expanded.insert("acme/engine#418".into());
            if let Some(pr) = s.prs.iter_mut().find(|p| p.id == "acme/cli#908") {
                pr.merge_readiness = model::MergeReadiness::Ready;
            }
            s.approve_armed = Some(("acme/cli#904".into(), std::time::Instant::now()));
        }),
        ("settings-ai", |s| {
            seed_preview(s);
            s.ai_key = TextEdit::new("sk-or-v1-0123456789abcdef");
            s.ai_enabled = false;
            s.page = Page::Inbox;
            s.settings_open = true;
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
            s.autonomy_branches = true;
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
        ("sorted-by-rules", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.smart_sort = Some(false);
        }),
        // The groups dragged away from the default, so the reset button is on offer.
        ("reordered", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.group_order = vec![0, 2, 8, 9, 10, 1, 3, 7, 4, 5, 6];
        }),
        ("ignored", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.ignored.insert("acme/engine#920".into());
            s.ignored.insert("acme/engine#925".into());
        }),
        ("ignored-open", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.ignored.insert("acme/engine#920".into());
            s.ignored.insert("acme/cli#910".into());
            s.ignored_open = true;
        }),
        ("auto-merge", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            for pr in &mut s.prs {
                match pr.number {
                    412 => {
                        pr.auto_merge = model::AutoMerge::Available;
                        pr.merge_readiness = model::MergeReadiness::NeedsReview;
                    }
                    889 => {
                        pr.auto_merge = model::AutoMerge::On;
                        pr.merge_readiness = model::MergeReadiness::NeedsReview;
                    }
                    _ => {}
                }
            }
            // One GitHub merged by itself while the app was closed.
            if let Some(done) = s.prs.iter().find(|p| p.number == 908).cloned() {
                let mut pr = done;
                pr.id = "acme/cli#901".into();
                pr.number = 901;
                pr.title = "cli: print the version next to the command name".into();
                pr.approved_by = vec!["lkostrowski".into()];
                s.tracked.push(model::Tracked {
                    pr,
                    section: "Merge these".into(),
                    state: model::TrackedState::Merged,
                });
            }
        }),
        // A moment into two pull requests arriving: the tint is still fading.
        ("arriving", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            let past = std::time::Instant::now() - std::time::Duration::from_millis(150);
            s.arrivals = vec![
                ("acme/engine#920".into(), past),
                ("acme/engine#925".into(), past),
            ];
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
        // Approve and Merge mid-flight: one of each armed, one approval just landed.
        ("buttons", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            let ago = |secs: f32| std::time::Instant::now() - std::time::Duration::from_secs_f32(secs);
            s.approve_armed = Some(("acme/engine#418".into(), ago(1.5)));
            s.approved = vec![("acme/cli#904".into(), ago(0.6))];
            s.merge_armed = Some(("acme/cli#908".into(), ago(1.0)));
        }),
        // Pull requests held up on their way to the merge: yours, with the button that clears each,
        // and other people's, where it is the author's to clear.
        ("unblock", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            let check = |name: &str, run: u64| model::Check {
                name: name.into(),
                url: format!("https://github.com/acme/cli/actions/runs/{run}/job/1"),
            };
            let asked = |hours_ago, again| {
                Some(model::Ask {
                    at: "2026-11-18T09:00:00Z".into(),
                    hours_ago,
                    again,
                })
            };
            for pr in &mut s.prs {
                match pr.id.as_str() {
                    "acme/cli#908" => {
                        pr.merge_readiness = model::MergeReadiness::Behind;
                        pr.can_update_branch = true;
                    }
                    "acme/cli#889" => {
                        pr.ci_status = CiStatus::Failing;
                        pr.merge_readiness = model::MergeReadiness::NeedsReview;
                        pr.failing_checks = vec![check("lint", 71), check("e2e (chromium)", 72)];
                        pr.files = ["src/cli/args.rs", "src/cli/complete.rs", "tests/complete.rs"]
                            .map(String::from)
                            .to_vec();
                    }
                    "acme/engine#412" => pr.merge_readiness = model::MergeReadiness::Conflicts,
                    "acme/inbox#14" => pr.unanswered_threads = Some(2),
                    "acme/engine#418" => pr.asked = asked(74, false),
                    "acme/cli#904" => {
                        pr.ci_status = CiStatus::Failing;
                        pr.merge_readiness = model::MergeReadiness::Conflicts;
                        pr.asked = asked(5, true);
                    }
                    _ => {}
                }
            }
        }),
        ("unblock-open", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            if let Some(pr) = s.prs.iter_mut().find(|p| p.id == "acme/cli#889") {
                pr.ci_status = CiStatus::Failing;
                pr.merge_readiness = model::MergeReadiness::NeedsReview;
                pr.can_update_branch = true;
                pr.unanswered_threads = Some(0);
                pr.failing_checks = ["lint", "e2e (chromium)", "e2e (webkit)"]
                    .iter()
                    .zip(71..)
                    .map(|(name, run)| model::Check {
                        name: (*name).into(),
                        url: format!("https://github.com/acme/cli/actions/runs/{run}/job/1"),
                    })
                    .collect();
                pr.files = [
                    "src/cli/args.rs",
                    "src/cli/complete.rs",
                    "src/cli/complete/zsh.rs",
                    "tests/complete.rs",
                ]
                .map(String::from)
                .to_vec();
                pr.description = "Completes subcommands and flags in zsh.".into();
            }
            s.expanded.insert("acme/cli#889".into());
        }),
        // Pace right after an approval landed quick, and a big one approved bold.
        ("pace", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            let ago = |secs: f32| std::time::Instant::now() - std::time::Duration::from_secs_f32(secs);
            let flash = |id: &str, kind, wait, lines, verdict| pace::Event {
                at: chrono::Utc::now().timestamp(),
                kind,
                pr_id: id.into(),
                wait_minutes: Some(wait),
                lines,
                verdict,
            };
            s.approved = vec![("acme/cli#904".into(), ago(1.6)), ("acme/engine#418".into(), ago(1.6))];
            s.pace_flash = vec![
                (flash("acme/cli#904", pace::Kind::Approve, 2, 180, pace::Verdict::Quick), ago(1.2)),
                (flash("acme/engine#418", pace::Kind::Approve, 3, 1240, pace::Verdict::Bold), ago(1.6)),
            ];
            s.celebrations.push(("acme/cli#908".into(), ago(0.9)));
            s.pace_flash.push((flash("acme/cli#908", pace::Kind::Merge, 1, 90, pace::Verdict::Quick), ago(0.9)));
        }),
        // A quick approval a moment after it landed: the number rolls, today's cell rings.
        ("pace-landing", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            let age: f32 = std::env::var("MEAT_PROXY_SCENE_AGE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.45);
            let event = pace::Event {
                at: chrono::Utc::now().timestamp(),
                kind: pace::Kind::Approve,
                pr_id: "acme/cli#904".into(),
                wait_minutes: Some(2),
                lines: 180,
                verdict: pace::Verdict::Quick,
            };
            let at = std::time::Instant::now() - std::time::Duration::from_secs_f32(age);
            s.pace_events.push(event.clone());
            s.pace_flash = vec![(event, at)];
            s.approved = vec![("acme/cli#904".into(), at)];
        }),
        ("pace-collapsed", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
            s.pace_collapsed = true;
        }),
        ("pace-empty", |s| {
            seed_preview(s);
            s.pace_events.clear();
            s.page = Page::Inbox;
            s.filter = InboxFilter::Inbox;
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
        assert_eq!(snap.prs.len(), 14);
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
    fn starts_on_onboarding_and_opens_the_inbox_once_connected() {
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

        // Connected: setup waits for Open inbox. Notifications start off and are switched on here.
        seed_preview(&mut rt.state);
        rt.update(|_, _| {});
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.page, Page::Onboarding);
        assert!(!rt.state.native_notifications);
        let (x, y) = rt.find("onboard-notifications").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0 / 60.0);
        let (x, y) = rt.find("onboard-open-inbox").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(3.0 / 60.0);
        assert_eq!(rt.state.page, Page::Inbox);
        assert_eq!(rt.state.filter, InboxFilter::Inbox);
        assert_eq!(
            (rt.state.open_stale_hours, rt.state.draft_stale_hours),
            (48, 72)
        );
        assert!(rt.state.native_notifications);
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
        // The listed order is the order on screen: ready to merge leads.
        assert_eq!(state.layout.sections[0].name, "Merge these");
        assert_eq!(state.group_rank(0), 0, "ready to merge leads");
        assert_eq!(state.group_rank(1), 1, "then the next section in the file");
        // Groups 0, 3 and 6 are on screen; drag the last to the top.
        state.drop_group(&[0, 3, 6], 2, 0);
        assert_eq!(state.group_order, [6, 0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12]);
        // Drag the first to the bottom of what is shown: it lands after group 3.
        state.drop_group(&[6, 0, 3], 0, 2);
        assert_eq!(state.group_order, [0, 1, 2, 3, 6, 4, 5, 7, 8, 9, 10, 11, 12]);
        // Reordering is undone by the reset, which is only on offer while the order differs.
        assert!(!state.group_order_is_default());
        state.reset_group_order();
        assert!(state.group_order_is_default());

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

    fn said(pairs: &[(&str, f32)], by: &str) -> judge::Judgment {
        judge::Judgment {
            answers: pairs.iter().map(|(q, p)| ((*q).to_owned(), *p)).collect(),
            by: by.into(),
            fingerprint: String::new(),
        }
    }

    fn section_name(state: &App, pr: &PullRequest) -> Option<String> {
        let att = pr.attention(72, 48, current_epoch_hour());
        state
            .action_group_of(pr, att)
            .map(|(_, name)| name.to_owned())
    }

    #[test]
    fn review_requests_go_to_the_first_section_that_matches() {
        let mut state = init();
        state.preview = true;
        state.prs = github::demo_prs();
        let mut pr = state.prs.iter().find(|p| p.number == 904).unwrap().clone();
        pr.external = false;
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Review these"));
        // Size is a plain rule and needs no model.
        let mut huge = pr.clone();
        huge.additions = 600;
        assert_eq!(section_name(&state, &huge).as_deref(), Some("Big reviews"));
        // The model's answers apply, in the order of the sections.
        state.ai_feature = true;
        state.ai_enabled = true;
        state.judgments.insert(
            pr.id.clone(),
            said(&[("reading", 0.05), ("urgent", 0.02)], "m"),
        );
        assert_eq!(
            section_name(&state, &pr).as_deref(),
            Some("Approve at will")
        );
        state
            .judgments
            .insert(huge.id.clone(), said(&[("reading", 0.02)], "m"));
        assert_eq!(
            section_name(&state, &huge).as_deref(),
            Some("Big reviews"),
            "big is never approved on sight"
        );
        state.judgments.insert(
            pr.id.clone(),
            said(&[("reading", 0.05), ("urgent", 0.7)], "m"),
        );
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Do first"));
        state
            .judgments
            .insert(pr.id.clone(), said(&[("reading", 0.9)], "m"));
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Read carefully"));
        // Mine are never moved by a question: they keep their own sections.
        let mine = state
            .prs
            .iter()
            .find(|p| p.ownership == Ownership::Mine && p.number == 908)
            .unwrap()
            .clone();
        state
            .judgments
            .insert(mine.id.clone(), said(&[("reading", 0.01)], "m"));
        assert_eq!(section_name(&state, &mine).as_deref(), Some("Merge these"));
    }

    #[test]
    fn rules_alone_ignore_the_models_answers_but_keep_the_rules_own() {
        let mut state = init();
        state.preview = true;
        state.ai_feature = true;
        state.ai_enabled = true;
        let mut review = github::demo_prs()
            .into_iter()
            .find(|p| p.number == 904)
            .unwrap();
        review.external = false;
        review.additions = 300;
        state
            .judgments
            .insert(review.id.clone(), said(&[("urgent", 0.9)], "gpt"));
        // A model's answer counts while sorting by AI, and not once the user picks rules.
        assert!(state.smart_active());
        assert_eq!(section_name(&state, &review).as_deref(), Some("Do first"));
        state.toggle_smart_sort();
        assert!(!state.smart_active());
        assert_eq!(
            section_name(&state, &review).as_deref(),
            Some("Review these")
        );
        // A rule needs no model: a bot's bump is approved at will either way, saved or not.
        let mut bump = review.clone();
        bump.id = "acme/cli#950".into();
        bump.author = "dependabot".into();
        bump.title = "Bump serde from 1.0.1 to 1.0.2".into();
        assert_eq!(
            section_name(&state, &bump).as_deref(),
            Some("Approve at will")
        );
        // Behind the flag: with it off nothing is asked and the answers saved earlier are ignored.
        state.ai_feature = false;
        assert!(!state.smart_available() && !state.smart_active());
        // With no key and no saved answers there is nothing to sort by except rules.
        let mut bare = init();
        bare.preview = true;
        assert!(!bare.smart_available() && !bare.smart_active());
        bare.toggle_smart_sort();
        assert_eq!(bare.smart_sort, None);
    }

    #[test]
    fn outside_contributors_can_be_hidden_with_the_flag() {
        let mut state = init();
        state.preview = true;
        let mut pr = github::demo_prs()
            .into_iter()
            .find(|p| p.number == 904)
            .unwrap();
        let now = current_epoch_hour();
        pr.external = true;
        state.prs = vec![pr.clone()];
        assert!(state.in_inbox(&pr, now));
        state.hide_external = true;
        assert!(
            !state.in_inbox(&pr, now),
            "the flag hides them from the Inbox"
        );
        assert_eq!(state.visible_groups(), Vec::<u8>::new());
    }

    #[test]
    fn a_new_layout_drops_the_screen_order_made_for_the_old_one() {
        let mut state = init();
        state.preview = true;
        state.group_order = vec![3, 1, 0, 2, 4, 5, 6, 7, 8, 9];
        let before = state.layout.fingerprint.clone();
        state.layout = sections::Layout::parse(
            r#"{ "sections": [{ "name": "All of it", "when": ["any"] }] }"#,
        )
        .unwrap();
        assert_ne!(state.layout.fingerprint, before);
        // A snapshot saved under the old layout does not bring its order back.
        let mut snap = db::AppSnapshot {
            group_order: vec![3, 1, 0],
            layout_key: Some(before),
            ..Database::open_in_memory().unwrap().load_snapshot().unwrap()
        };
        snap.collapsed_groups = vec![1];
        state.apply_snapshot(snap);
        assert_eq!(state.group_order, [0]);
        assert!(state.collapsed_groups.is_empty());
    }

    #[test]
    fn an_ignored_pull_request_moves_to_its_own_section_and_stops_counting() {
        let mut state = init();
        state.preview = true;
        state.prs = github::demo_prs();
        let now = current_epoch_hour();
        let pr = state.prs.iter().find(|p| p.number == 920).unwrap().clone();
        let counted = |s: &App| s.prs.iter().filter(|p| s.needs_you(p, now)).count();
        let before = counted(&state);
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Review these"));
        state.toggle_ignore(&pr.id);
        // It is still listed, but under Ignored, last, and no longer counts as needing you.
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Ignored"));
        assert!(state.in_inbox(&pr, now) && !state.needs_you(&pr, now));
        assert_eq!(counted(&state), before - 1);
        assert!(!state.visible_groups().contains(&IGNORED_GROUP));
        assert!(state.group_rank(IGNORED_GROUP) > state.group_rank(11));
        // A notification about it is not worth showing; about another it is.
        assert!(state.is_ignored_url("https://github.com/acme/engine/pull/920"));
        assert!(!state.is_ignored_url("https://github.com/acme/engine/pull/921"));
        // Stop ignoring puts it back, and the choice survives a restart.
        state.toggle_ignore(&pr.id);
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Review these"));
        let db = Database::open_in_memory().unwrap();
        db.set_ignored("a/b#1", true).unwrap();
        db.set_ignored("a/b#2", true).unwrap();
        db.set_ignored("a/b#1", false).unwrap();
        assert_eq!(db.load_snapshot().unwrap().ignored.len(), 1);
    }

    #[test]
    fn the_age_limit_looks_at_when_it_was_opened_as_well_as_when_it_last_moved() {
        let mut pr = github::demo_prs().remove(0);
        pr.opened_hours = 24 * 190; // opened in May
        pr.idle_hours = 24 * 14; // an edit two weeks ago
        assert!(!pr.within_days(30), "an edit does not make it young");
        assert!(pr.within_days(0), "0 means no limit");
        assert!(pr.within_days(365));
        // Opened recently but untouched for a while: the older clock counts too.
        pr.opened_hours = 24 * 5;
        pr.idle_hours = 24 * 40;
        assert!(!pr.within_days(30));
        pr.idle_hours = 24 * 3;
        assert!(pr.within_days(30));
        // Saved ones are held to it when they load.
        let mut state = init();
        state.preview = true;
        let mut old = pr.clone();
        old.id = "a/b#1".into();
        old.opened_hours = 24 * 190;
        old.idle_hours = 24 * 14;
        let mut snap = Database::open_in_memory().unwrap().load_snapshot().unwrap();
        snap.prs = vec![pr.clone(), old];
        state.apply_snapshot(snap);
        assert_eq!(state.prs.len(), 1);
        assert_eq!(state.prs[0].id, pr.id);
    }

    #[test]
    fn auto_merge_is_a_state_of_the_merge_button() {
        let mut state = init();
        state.preview = true;
        state.prs = github::demo_prs();
        let id = "acme/engine#412".to_owned();
        let set = |s: &mut App, auto: model::AutoMerge, ready: model::MergeReadiness| {
            let pr = s.prs.iter_mut().find(|p| p.id == id).unwrap();
            pr.auto_merge = auto;
            pr.merge_readiness = ready;
        };
        use model::{AutoMerge, MergeReadiness};
        // Not mergeable yet but GitHub allows it: auto-merge is on offer.
        set(
            &mut state,
            AutoMerge::Available,
            MergeReadiness::NeedsReview,
        );
        assert_eq!(state.auto_merge_target(&id), Some(true));
        // Mergeable now: plain Merge, no auto-merge.
        set(&mut state, AutoMerge::Available, MergeReadiness::Ready);
        assert_eq!(state.auto_merge_target(&id), None);
        // Not allowed: nothing to offer.
        set(
            &mut state,
            AutoMerge::Unavailable,
            MergeReadiness::NeedsReview,
        );
        assert_eq!(state.auto_merge_target(&id), None);
        // Already on: pressing turns it off.
        set(&mut state, AutoMerge::On, MergeReadiness::NeedsReview);
        assert_eq!(state.auto_merge_target(&id), Some(false));
    }

    #[test]
    fn a_pull_request_auto_merge_finished_stays_faded_until_cleared() {
        let mut state = init();
        state.preview = true;
        state.page = Page::Inbox;
        state.filter = InboxFilter::Inbox;
        state.prs = github::demo_prs();
        let mut merged = state.prs.iter().find(|p| p.number == 908).unwrap().clone();
        merged.id = "acme/cli#999".into();
        merged.number = 999;
        merged.auto_merge = model::AutoMerge::On;
        // It is followed while auto-merge is on, in the section it was in.
        state.prs.push(merged.clone());
        state.after_sync_track();
        let tracked = state.tracked.iter().find(|t| t.pr.id == merged.id).unwrap();
        assert_eq!(tracked.state, model::TrackedState::Watching);
        assert_eq!(tracked.section, "Merge these");
        // GitHub merges it, and it leaves the lists.
        state.prs.retain(|p| p.id != merged.id);
        state.tracked[0].state = model::TrackedState::Merged;
        let mut rt = Runtime::new(state, view, (920.0, 1500.0), Env::new(true, 2.0));
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);
        assert!(
            rt.find("finished-acme/cli#999").is_some(),
            "it stays on screen"
        );
        let (x, y) = rt.find("clear-acme/cli#999").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0 / 60.0);
        assert!(
            rt.find("finished-acme/cli#999").is_none(),
            "clearing removes it"
        );
        // It survives a restart until it is cleared.
        let db = Database::open_in_memory().unwrap();
        let mut t = rt.state.tracked.first().cloned().unwrap_or(model::Tracked {
            pr: merged.clone(),
            section: "Merge these".into(),
            state: model::TrackedState::Merged,
        });
        t.state = model::TrackedState::Merged;
        db.save_tracked(&t).unwrap();
        let loaded = db.load_snapshot().unwrap().tracked;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].state, model::TrackedState::Merged);
        db.delete_tracked(&t.pr.id).unwrap();
        assert!(db.load_snapshot().unwrap().tracked.is_empty());
    }

    #[test]
    fn who_approved_is_named() {
        let mut pr = github::demo_prs().remove(0);
        assert_eq!(pr.approval_label(), "Approved");
        pr.approved_by = vec!["lkostrowski".into()];
        assert_eq!(pr.approval_label(), "lkostrowski approved");
        pr.approved_by.push("mirekm".into());
        assert_eq!(pr.approval_label(), "lkostrowski and mirekm approved");
        pr.approved_by.push("kai".into());
        pr.approved_by.push("juno".into());
        assert_eq!(pr.approval_label(), "lkostrowski +3 approved");
    }

    #[test]
    fn only_pull_requests_new_to_a_filled_list_arrive() {
        let mut state = init();
        let all = github::demo_prs();
        // The first fill of an empty list animates nothing.
        state.note_arrivals(&all);
        assert!(state.arrivals.is_empty());
        state.prs = all[..10].to_vec();
        state.note_arrivals(&all);
        let ids: Vec<&str> = state.arrivals.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids.len(), all.len() - 10);
        assert!(ids.iter().all(|id| !state.prs.iter().any(|p| p.id == *id)));
        // Each later arrival starts a little after the one before it, so a batch cascades.
        let times: Vec<_> = state.arrivals.iter().map(|(_, at)| *at).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
        // It ends: a long time on, nothing is arriving.
        assert!(state.arrival_age(&state.arrivals[0].0.clone()).is_some());
        state.arrivals[0].1 -= std::time::Duration::from_secs(10);
        assert!(state.arrival_age(&state.arrivals[0].0.clone()).is_none());
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
    fn a_row_opens_on_press_and_approve_needs_two() {
        let mut state = init();
        state.preview = true;
        state.prs = github::demo_prs();
        let id = state.prs[0].id.clone();
        state.toggle_expanded(&id);
        assert!(state.expanded.contains(&id));
        state.toggle_expanded(&id);
        assert!(!state.expanded.contains(&id));

        state.press_approve(&id);
        assert_eq!(
            state.approve_armed.as_ref().map(|(a, _)| a.as_str()),
            Some(id.as_str())
        );
        state.press_approve(&id);
        assert!(state.approve_armed.is_none());
        assert!(state.approving.is_empty(), "a preview never calls GitHub");
    }

    #[test]
    fn approve_shows_only_while_its_row_is_hovered() {
        let mut state = init();
        state.preview = true;
        seed_preview(&mut state);
        state.page = Page::Inbox;
        let mut rt = Runtime::new(state, view, (1000.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        let key = "approve-acme/engine#418";
        let opacity = |rt: &Runtime<App>| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.opacity)
        };
        assert_eq!(opacity(&rt), Some(0.0));
        let (x, y) = rt.find("pr-row-acme/engine#418").unwrap();
        rt.pointer_move(x + 20.0, y);
        rt.frame(0.5);
        rt.frame(1.0);
        assert_eq!(opacity(&rt), Some(1.0));
    }

    #[test]
    fn a_blocked_pull_request_goes_to_unblock_with_the_button_that_clears_it() {
        let mut state = init();
        state.preview = true;
        seed_preview(&mut state);
        state.page = Page::Inbox;
        let approved = "acme/cli#908";
        let set = |state: &mut App, readiness, can_update| {
            let pr = state.prs.iter_mut().find(|p| p.id == approved).unwrap();
            pr.merge_readiness = readiness;
            pr.can_update_branch = can_update;
        };
        let section = |state: &App| {
            section_name(state, state.prs.iter().find(|p| p.id == approved).unwrap())
        };
        let opacity = |rt: &Runtime<App>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.opacity)
        };
        let (unblock, merge) = (format!("unblock-{approved}"), format!("merge-{approved}"));

        // Mergeable: no button but Merge.
        set(&mut state, model::MergeReadiness::Ready, false);
        assert_eq!(section(&state).as_deref(), Some("Merge these"));
        let mut rt = Runtime::new(state, view, (1000.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        assert_eq!(opacity(&rt, &unblock), None);
        assert_eq!(opacity(&rt, &merge), Some(1.0));

        // Behind its base without that holding the merge: Update branch waits for the pointer.
        set(&mut rt.state, model::MergeReadiness::Ready, true);
        rt.update(|_, _| {});
        rt.frame(1.0);
        rt.frame(2.0);
        assert_eq!(section(&rt.state).as_deref(), Some("Merge these"));
        assert_eq!(opacity(&rt, &unblock), Some(0.0));
        assert_eq!(opacity(&rt, &merge), Some(1.0));

        // GitHub holds the merge until the branch is up to date: the row moves to Unblock, and
        // Update branch takes the place of a Merge button that could do nothing.
        set(&mut rt.state, model::MergeReadiness::Behind, true);
        rt.update(|_, _| {});
        rt.frame(3.0);
        rt.frame(4.0);
        assert_eq!(section(&rt.state).as_deref(), Some("Unblock"));
        assert_eq!(opacity(&rt, &unblock), Some(1.0));
        assert_eq!(opacity(&rt, &merge), None);
    }

    #[test]
    fn conflicts_copy_a_prompt_for_an_agent() {
        let mut state = init();
        state.preview = true;
        seed_preview(&mut state);
        state.page = Page::Inbox;
        let conflicted = "acme/engine#412";
        let pr = state.prs.iter_mut().find(|p| p.id == conflicted).unwrap();
        pr.merge_readiness = model::MergeReadiness::Conflicts;
        let url = pr.url.clone();
        let clipboard = rune::clipboard::memory();
        let mut rt = Runtime::new(state, view, (1000.0, 1600.0), Env::new(true, 1.0));
        rt.use_clipboard(&clipboard);
        rt.frame(0.0);
        let (x, y) = rt.find(&format!("unblock-{conflicted}")).unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.1);
        let copied = clipboard.text().expect("a prompt on the clipboard");
        assert!(copied.contains(&format!("gh pr checkout {url}")), "{}", *copied);
        assert!(copied.contains("resolve the conflicts"));
        // The button says so, and the press did not open the row.
        assert_eq!(rt.state.copied.as_ref().map(|(id, _)| id.as_str()), Some(conflicted));
        assert!(!rt.state.expanded.contains(conflicted));
    }

    #[test]
    fn template_comments_are_dropped_from_a_description() {
        assert_eq!(clean_description("a <!-- note --> b\n<!-- open"), "a  b");
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
    fn a_new_rule_never_takes_the_id_of_one_still_there() {
        let mut state = init();
        state.preview = true;
        for name in ["one", "two"] {
            state.new_rule_name = TextEdit::new(name);
            state.new_rule_query = TextEdit::new("is:pr label:x");
            state.add_custom_rule();
        }
        state.rules.retain(|r| r.name != "one");
        state.new_rule_name = TextEdit::new("three");
        state.new_rule_query = TextEdit::new("is:pr label:y");
        state.add_custom_rule();
        let mut ids: Vec<&str> = state.rules.iter().map(|r| r.id.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "{ids:?}");
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
        // Nothing acts on GitHub by itself until the user says so.
        assert!(!snapshot.autonomy_branches && !snapshot.autonomy_reviewers);

        db.set_setting("autonomy_reviewers", "1").unwrap();
        db.set_setting("autonomy_done", "PR_1:update-branch:abc\nPR_2:rerequest:def")
            .unwrap();
        let snapshot = db.load_snapshot().unwrap();
        assert!(!snapshot.autonomy_branches && snapshot.autonomy_reviewers);
        assert!(snapshot.autonomy_done.contains_key("PR_2:rerequest:def"));
        assert_eq!(snapshot.autonomy_done.len(), 2);

        // Pace is on and unfolded until the user says otherwise, and keeps what it counted.
        assert!(snapshot.pace_enabled && !snapshot.pace_collapsed);
        let event = pace::Event {
            at: chrono::Utc::now().timestamp(),
            kind: pace::Kind::Merge,
            pr_id: "acme/cli#908".into(),
            wait_minutes: None,
            lines: 90,
            verdict: pace::Verdict::Steady,
        };
        db.add_pace_event(&event).unwrap();
        db.set_setting("pace_enabled", "0").unwrap();
        db.set_setting("pace_collapsed", "1").unwrap();
        let snapshot = db.load_snapshot().unwrap();
        assert!(!snapshot.pace_enabled && snapshot.pace_collapsed);
        assert_eq!(snapshot.pace_events, vec![event]);
    }

    #[test]
    fn a_quick_approval_shows_on_its_row_and_in_the_pace_panel_which_folds_and_turns_off() {
        let mut state = init();
        state.preview = true;
        state.loading = false;
        state.page = Page::Inbox;
        state.prs = github::demo_prs();
        let id = "acme/cli#904";
        let pr = state.prs.iter_mut().find(|p| p.id == id).unwrap();
        let asked = chrono::Utc::now() - chrono::Duration::minutes(12);
        pr.asked = Some(model::Ask {
            at: asked.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            hours_ago: 0,
            again: false,
        });
        pr.additions = 120;
        pr.deletions = 30;
        state.record_pace(pace::Kind::Approve, id);
        assert_eq!(state.pace_events.len(), 1);
        assert_eq!(state.pace_events[0].verdict, pace::Verdict::Quick);
        assert_eq!(state.pace_events[0].wait_minutes, Some(12));

        let mut rt = Runtime::new(state, view, (920.0, 1500.0), Env::new(true, 2.0));
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);
        assert!(rt.find(&format!("pace-pill-{id}")).is_some(), "the row says it was quick");
        assert!(rt.find("pace-body").is_some());
        // Folding keeps the header and hides the rest.
        let (x, y) = rt.find("pace-header").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0 / 60.0);
        assert!(rt.state.pace_collapsed);
        assert!(rt.find("pace-header").is_some() && rt.find("pace-body").is_none());
        // Off: no panel, no pill, and nothing more is counted.
        rt.update(|s, _| {
            s.toggle_pace();
            s.record_pace(pace::Kind::Merge, "acme/cli#908");
        });
        rt.frame(3.0 / 60.0);
        assert!(rt.find("pace-header").is_none());
        assert!(rt.find(&format!("pace-pill-{id}")).is_none());
        assert_eq!(rt.state.pace_events.len(), 1);
    }
}
