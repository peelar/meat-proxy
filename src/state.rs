//! The app's state: what it shows and remembers, and the actions that change it.

use crate::*;

/// How long a pressed Merge button waits for its confirming press.
pub(crate) const ARMED_SECS: f32 = 4.0;
/// How long a row stays washed in its button's colour after an approve or merge lands.
pub(crate) const INK_WASH_SECS: f32 = 1.4;
/// How long a merged PR's row celebrates before it leaves the list.
pub(crate) const CELEBRATION_SECS: f32 = 1.9;
/// A pull request that just arrived slides in over this long, and its row stays tinted for the longer one.
pub(crate) const ARRIVAL_SLIDE_SECS: f32 = 0.45;
pub(crate) const ARRIVAL_GLOW_SECS: f32 = 2.6;
/// Gap between rows arriving together, so a batch cascades.
pub(crate) const ARRIVAL_STAGGER_SECS: f32 = 0.07;

/// The first-launch intro: the button springs in, a finger presses it, the name appears, then the
/// setup screen fades in. A click or Escape skips it; Reduce Motion never plays it.
pub(crate) const INTRO_SECS: f32 = 5.0;
/// How long typing in Settings pauses before what was typed applies.
pub(crate) const TYPING_SETTLES: std::time::Duration = std::time::Duration::from_millis(1500);
/// When the finger reaches the button.
pub(crate) const INTRO_HIT: f32 = 1.45;

/// The section ignored pull requests move to, always last and folded until opened.
pub(crate) const IGNORED_GROUP: u8 = 255;
/// How long after the notifications feed announced a review request the sync keeps quiet about
/// the same request, which it sees as a second round.
pub(crate) const HEARD_SECS: u64 = 300;
/// How long a Copy prompt button says "Copied".
pub(crate) const COPIED_SECS: f32 = 1.6;
/// How long a row says it was quick (or bold), and the Pace panel's number for it pulses.
pub(crate) const PACE_FLASH_SECS: f32 = 3.2;

/// What a button can do to clear what blocks a pull request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unblock {
    /// Bring the branch up to date with its base.
    UpdateBranch,
}

pub(crate) fn current_epoch_hour() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64 / 3600
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Onboarding,
    Inbox,
}

pub struct App {
    pub(crate) avatars: avatars::Avatars,
    pub(crate) settings_open: bool,
    pub(crate) logo: Option<rune::Image>,
    /// The intro's button base, button face and finger.
    pub(crate) intro_art: Vec<rune::Image>,
    /// When the first-launch intro started; cleared once it ends or is skipped.
    pub intro: Option<std::time::Instant>,
    /// What `gh auth login` has printed so far, shown on the setup screen.
    pub(crate) auth_lines: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    pub(crate) auth_lines_shown: usize,
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
    pub approving: Vec<String>,
    /// Approved from here, and when; the button stays done until a sync moves the row.
    pub approved: Vec<(String, std::time::Instant)>,
    pub approve_worker: Latest<(String, Result<(), String>)>,
    /// Updating a branch or running failed checks again: the pull request, which, and how it went.
    pub unblock_worker: Latest<(String, Unblock, Result<(), String>)>,
    pub unblocking: Vec<String>,
    /// Review requests the notifications feed announced, by URL, and when.
    pub(crate) heard: Vec<(String, std::time::Instant)>,
    /// The pull request whose agent prompt was just copied, and when: its button says so briefly.
    pub copied: Option<(String, std::time::Instant)>,
    /// PRs just merged, with the moment they were: the row celebrates, then leaves the list.
    pub celebrations: Vec<(String, std::time::Instant)>,
    /// Pull requests a sync just brought in, with when each starts to appear.
    pub arrivals: Vec<(String, std::time::Instant)>,
    pub merge_worker: Latest<(String, Result<(), String>)>,
    /// Turning auto-merge on or off: the pull request, which, and how it went.
    /// One job per pull request whose auto-merge is being changed. A `Latest` keeps only its
    /// newest request, so each press gets its own: three at once finish independently.
    pub auto_merge_jobs: Vec<AutoMergeJob>,
    /// Wakes the app when a job lands; set once the app is up.
    pub waker: Option<rune::Waker>,
    /// Pull requests with auto-merge on, and the ones GitHub merged by itself (shown faded).
    pub tracked: Vec<model::Tracked>,
    /// Asks GitHub what became of tracked pull requests that left the lists.
    pub resolve_worker: Latest<Vec<(String, Result<String, String>)>>,
    pub last_synced: Option<std::time::Instant>,
    /// The status text as last drawn, to redraw only when it changes.
    pub sync_label_shown: String,
    /// Order of the Inbox tab's action groups (section positions).
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
    pub status_banner: Option<String>,
    /// The saved data, opened on first use and kept open; see `App::db`.
    pub(crate) db: std::cell::OnceCell<Database>,
    /// The saved data could not be read at launch. Setup must not run over it then: finishing
    /// setup replaces the saved pull requests.
    pub boot_failed: bool,
    pub boot_worker: Latest<Result<db::AppSnapshot, rusqlite::Error>>,
    pub sync_worker: Latest<Result<SyncOutput, String>>,
    /// A sync was asked for while one ran; it starts when that one lands.
    pub sync_again: bool,
    /// The Inbox sections (built in).
    pub layout: sections::Layout,
    /// Hide outside contributors' pull requests from the Inbox tab.
    pub hide_external: bool,
    /// List requests made of a team you are in, not of you by name, in the Inbox too.
    pub show_team_reviews: bool,
    /// The experimental AI sorting, its settings and what the model said.
    pub ai: Ai,
    /// What the typed settings (the age limit and the AI fields) held when last seen, and since
    /// when: they apply once the typing stops, not on each keystroke, so "14" never applies "1"
    /// and a half-typed address never gets the key.
    pub(crate) typed: Option<(String, std::time::Instant)>,
    /// The Pace panel's numbers, with the events (count, newest) and minute they were worked out for: the
    /// panel is drawn every frame while something animates, and they change far less often.
    pub(crate) pace_stats: std::cell::RefCell<Option<(PaceKey, pace::Stats)>>,
    /// Pull requests moved to the Ignored section, and whether that section is open.
    pub ignored: std::collections::HashSet<String>,
    pub ignored_open: bool,
    pub auth_worker: Latest<Result<(), String>>,
    pub fast_notif_worker: Latest<Result<github::NotificationPoll, String>>,
    pub discovery_worker: Latest<Result<github::Discovery, String>>,
    pub verify_worker: Latest<Result<String, String>>,
    pub discovery: Option<github::Discovery>,
    /// Repositories seen to be private while they are hidden (`MEAT_PROXY_HIDE_PRIVATE`).
    pub private_repos: std::collections::HashSet<String>,
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
    /// Settings › Autonomous: what Meat Proxy does on GitHub by itself, on the user's own PRs.
    pub autonomy_branches: bool,
    pub autonomy_reviewers: bool,
    /// Actions already taken (`autonomy::key`) with when, so each happens once per commit.
    pub autonomy_done: std::collections::HashMap<String, i64>,
    pub autonomy_worker: Latest<Vec<autonomy::Outcome>>,
    /// Settings › Fix with an agent, the model as typed, and the pull request an agent is on.
    pub agent: agent::Config,
    pub agent_model_open: bool,
    pub agent_running: Option<String>,
    pub agent_worker: Latest<(PullRequest, Result<agent::Outcome, String>)>,
    /// Pull requests the agent pushed a fix to this session, and when: the row says Fixed.
    pub agent_fixed: Vec<(String, std::time::Instant)>,
    /// Pace: the score of quick approvals and merges, on unless switched off in Settings; its
    /// panel above the Inbox, and whether it is folded.
    pub pace_enabled: bool,
    pub pace_collapsed: bool,
    pub pace_events: Vec<pace::Event>,
    /// What was just counted, and when: the row says so, and the panel's number pulses.
    pub pace_flash: Vec<(pace::Event, std::time::Instant)>,
}

/// A request to turn auto-merge on or off for one pull request, and the thread running it.
pub struct AutoMergeJob {
    pub id: String,
    pub enable: bool,
    pub worker: Latest<Result<(), String>>,
}

/// What the Pace panel's numbers depend on: how many events, the newest one's time, the minute.
pub(crate) type PaceKey = (usize, i64, i64);

/// The AI feature, apart from the rest of the app: hidden unless `MEAT_PROXY_AI=1`.
pub struct Ai {
    /// The experimental AI feature is visible at all (`MEAT_PROXY_AI=1`).
    pub feature: bool,
    /// Whether the decision model may be asked at all. Off until chosen: turning it on sends the
    /// description and changed file paths of review requests, private repositories included.
    pub enabled: bool,
    /// The endpoint, when one is set up and usable.
    pub config: Option<judge::JudgeConfig>,
    /// Sort with the model's verdicts (true) or by rules alone (false); unset follows what exists.
    pub smart_sort: Option<bool>,
    pub url: TextEdit,
    pub model: TextEdit,
    pub key: TextEdit,
    /// The three fields as last applied, to notice an edit.
    pub(crate) applied: (String, String, String),
    pub judgments: std::collections::HashMap<String, judge::Judgment>,
    pub worker: Latest<judge::Batch>,
    /// Spent on the model in this run of the app, against the config's cap.
    pub spent: f64,
    /// An error that would repeat (bad key, no credit): no more asking until the app restarts.
    pub off: bool,
    /// Tells the running batch to stop: AI sorting was switched off.
    pub(crate) stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Ai {
    fn new() -> Self {
        Ai {
            feature: judge::feature_enabled(),
            enabled: false,
            config: None,
            smart_sort: None,
            url: TextEdit::new(judge::DECISIONS_URL),
            model: TextEdit::new(judge::DECISIONS_MODEL),
            key: TextEdit::new(""),
            applied: (
                judge::DECISIONS_URL.into(),
                judge::DECISIONS_MODEL.into(),
                String::new(),
            ),
            judgments: std::collections::HashMap::new(),
            worker: Latest::default(),
            spent: 0.0,
            off: false,
            stop: Default::default(),
        }
    }
}

pub(crate) fn init() -> App {
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
        approving: Vec::new(),
        approved: Vec::new(),
        approve_worker: Latest::default(),
        celebrations: Vec::new(),
        arrivals: Vec::new(),
        merge_worker: Latest::default(),
        auto_merge_jobs: Vec::new(),
        waker: None,
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
        status_banner: None,
        db: std::cell::OnceCell::new(),
        boot_failed: false,
        boot_worker: Latest::default(),
        sync_worker: Latest::default(),
        sync_again: false,
        layout: sections::Layout::default(),
        hide_external: false,
        show_team_reviews: false,
        ai: Ai::new(),
        typed: None,
        pace_stats: std::cell::RefCell::new(None),
        ignored: std::collections::HashSet::new(),
        private_repos: std::collections::HashSet::new(),
        ignored_open: false,
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
        agent: agent::Config::default(),
        agent_model_open: false,
        agent_running: None,
        agent_worker: Latest::default(),
        agent_fixed: Vec::new(),
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
    pub(crate) fn db(&self) -> Option<&Database> {
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
        if profile::hide_private() {
            // The saved data does not say which repositories are private. Show nothing of it
            // until the first sync does.
            self.prs.clear();
            self.notifications.clear();
        }
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
        self.ai.judgments = snap.judgments;
        self.ai.smart_sort = snap.smart_sort;
        self.hide_external = snap.hide_external;
        self.show_team_reviews = snap.show_team_reviews;
        self.ai.enabled = snap.ai_enabled;
        self.agent = snap.agent;
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
    pub(crate) fn note_arrivals(&mut self, incoming: &[PullRequest]) {
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
    pub(crate) fn arrival_age(&self, pr_id: &str) -> Option<f32> {
        let (_, at) = self.arrivals.iter().find(|(id, _)| id == pr_id)?;
        let age = std::time::Instant::now()
            .saturating_duration_since(*at)
            .as_secs_f32();
        (age < ARRIVAL_GLOW_SECS).then_some(age)
    }

    /// What a judgment says about this pull request, if one was made.
    pub fn judgment(&self, pr: &PullRequest) -> Option<&judge::Judgment> {
        self.ai.judgments.get(&pr.id)
    }

    /// Whether a model has anything to say here: a key is set, or verdicts were saved earlier.
    pub fn smart_available(&self) -> bool {
        self.ai.feature
            && self.ai.enabled
            && (self.ai.config.is_some() || !self.ai.judgments.is_empty())
            && self.layout.asks_anything()
    }

    /// Sorting with the model's verdicts. The user's choice wins; without one, the model is used
    /// when it is there.
    pub fn smart_active(&self) -> bool {
        self.ai.smart_sort.unwrap_or_else(|| self.smart_available()) && self.smart_available()
    }

    pub fn toggle_smart_sort(&mut self) {
        if !self.smart_available() {
            return;
        }
        let next = !self.smart_active();
        self.ai.smart_sort = Some(next);
        self.save_flag("smart_sort", next);
    }

    /// What the sorting goes by: with AI, the model's answers or a rule's; with rules alone, only
    /// what a rule can say (bots, tiny changes), whether or not it was saved.
    pub(crate) fn answers_for(&self, pr: &PullRequest) -> std::collections::HashMap<String, f32> {
        match self.judgment(pr) {
            Some(j) if self.smart_active() || j.by == "rules" => j.answers.clone(),
            _ => judge::rule_judgment(pr)
                .map(|j| j.answers)
                .unwrap_or_default(),
        }
    }

    /// The default order of the sections on screen: the order they are listed in.
    pub(crate) fn default_group_order(&self) -> Vec<u8> {
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
    pub(crate) fn is_ignored_url(&self, url: &str) -> bool {
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

    pub(crate) fn set_ai_fields(&mut self, url: &str, model: &str, key: &str) {
        self.ai.url = TextEdit::new(url);
        self.ai.model = TextEdit::new(model);
        self.ai.key = TextEdit::new(key);
        self.ai.applied = (url.into(), model.into(), key.into());
    }

    /// Applies the AI fields once they differ from what is in effect: saves `judge.json` and asks
    /// again for anything the model had not been able to judge.
    pub(crate) fn apply_ai_fields(&mut self) -> bool {
        let now = (
            self.ai.url.text.trim().to_owned(),
            self.ai.model.text.trim().to_owned(),
            self.ai.key.text.trim().to_owned(),
        );
        if now == self.ai.applied || now.0.is_empty() || now.1.is_empty() {
            return false;
        }
        let cfg = judge::JudgeConfig::from_fields(&now.0, &now.1, &now.2, self.ai.config.as_ref());
        if !self.preview && self.allow_os_side_effects {
            let _ = cfg.save();
        }
        self.ai.applied = now;
        self.ai.config = cfg.usable().then_some(cfg);
        self.ai.off = false;
        self.queue_judgments();
        true
    }

    pub fn toggle_ai(&mut self) {
        self.ai.enabled = !self.ai.enabled;
        self.save_flag("ai_enabled", self.ai.enabled);
        if !self.ai.enabled {
            self.ai.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
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

    pub(crate) fn save_flag(&self, key: &str, value: bool) {
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
    pub(crate) fn action_group_of(&self, pr: &PullRequest, attention: PrAttention) -> Option<(u8, &str)> {
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
    pub(crate) fn hidden_from_inbox(&self, pr: &PullRequest, now: u64) -> bool {
        (self.hide_external && pr.external)
            || (!self.show_team_reviews
                && pr.attention(self.draft_stale_hours, self.open_stale_hours, now)
                    == PrAttention::IncomingViaTeam)
    }

    /// What the Inbox is about, whatever is typed in the search box.
    pub(crate) fn needs_you(&self, pr: &PullRequest, now: u64) -> bool {
        !self.ignored.contains(&pr.id)
            && InboxFilter::Inbox.matches(pr, self.draft_stale_hours, self.open_stale_hours, now)
            && !self.hidden_from_inbox(pr, now)
    }

    /// Whether the Inbox tab lists it: it asks something of you, and nothing hides it.
    pub(crate) fn in_inbox(&self, pr: &PullRequest, now: u64) -> bool {
        // An ignored pull request is listed, in its own section, while it would still be there.
        // Your own pull request waiting on its reviewers asks nothing of you, but belongs here: it
        // is where you look for what is open. It does not count as needing you.
        let waiting = !self.ignored.contains(&pr.id)
            && pr.attention(self.draft_stale_hours, self.open_stale_hours, now)
                == PrAttention::MyOpenWaiting;
        let listed = self.needs_you(pr, now)
            || waiting
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
        let Some(cfg) = self.ai.config.clone() else {
            return;
        };
        if !self.ai.feature
            || !self.ai.enabled
            || self.ai.off
            || self.preview
            || self.ai.worker.is_pending()
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
            &self.ai.judgments,
            &cfg,
            &questions,
        );
        let settled = plan.settled;
        if !settled.is_empty() {
            self.store_judgments(settled);
        }
        if plan.to_ask.is_empty() || self.ai.spent >= cfg.max_spend_usd {
            return;
        }
        let spent = self.ai.spent;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.ai.stop = stop.clone();
        self.ai.worker.request(plan.to_ask, move |prs| {
            judge::run_batch(&cfg, &questions, prs, spent, &stop)
        });
    }

    pub(crate) fn store_judgments(&mut self, judged: Vec<(String, judge::Judgment)>) {
        if let Some(db) = self.db() {
            let live: Vec<String> = self.prs.iter().map(|p| p.id.clone()).collect();
            let _ = db.save_judgments(&judged, &live);
        }
        self.ai.judgments.extend(judged);
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
            crate::profile::app_name(),
            "Notifications are on. You'll see a note here when a review is requested of you or a PR needs a nudge.",
            None,
            self.allow_os_side_effects,
        );
    }

    /// Keeps the saved order a full list of the known groups.
    pub(crate) fn normalize_group_order(&mut self) {
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

    pub(crate) fn persist_groups(&self) {
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

    /// One press approves. Merge asks for a second press; an approval only adds a review.
    pub fn press_approve(&mut self, pr_id: &str) {
        if !self.approving.is_empty() || self.approved.iter().any(|(id, _)| id == pr_id) {
            return;
        }
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
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
            _ => return,
        }
        self.unblocking.push(pr_id.to_owned());
    }

    /// Whether the Fix button is on offer: the setting is on and nothing else is running.
    /// Whether rows show Fix instead of Copy prompt. A preview shows it too, but never runs it.
    pub fn agent_offered(&self) -> bool {
        self.agent.enabled && (self.preview || self.allow_os_side_effects)
    }

    pub fn agent_ready(&self) -> bool {
        self.agent.enabled && !self.preview && self.allow_os_side_effects
    }

    /// Start the configured agent on a pull request's prompt, detached. One at a time.
    pub fn press_fix(&mut self, pr_id: &str) {
        if !self.agent_ready() || self.agent_running.is_some() {
            return;
        }
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        let Some(prompt) = pr.agent_prompt() else {
            return;
        };
        let (pr, cfg) = (pr.clone(), self.agent.clone());
        self.agent_running = Some(pr.id.clone());
        self.agent_worker.request((), move |()| {
            let result = agent::run(&pr, &prompt, &cfg);
            (pr, result)
        });
    }

    fn save_setting(&self, key: &str, value: &str) {
        if let Some(db) = self.db() {
            let _ = db.set_setting(key, value);
        }
    }

    pub fn toggle_agent(&mut self) {
        self.agent.enabled = !self.agent.enabled;
        self.save_flag("agent_enabled", self.agent.enabled);
    }

    pub fn pick_agent_provider(&mut self, provider: agent::Provider) {
        self.agent.provider = provider;
        self.save_setting("agent_provider", provider.id());
        // The other agent's model means nothing to this one.
        self.pick_agent_model(agent::DEFAULT_MODEL);
    }

    /// Keeps the model chosen in Settings; the first option leaves it to the agent.
    pub fn pick_agent_model(&mut self, choice: &str) {
        let model = agent::model_value(self.agent.provider, choice);
        self.agent_model_open = false;
        if model != self.agent.model {
            self.save_setting("agent_model", &model);
            self.agent.model = model;
        }
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
        if self.preview || self.is_auto_merging(pr_id) {
            return;
        }
        let Some(pr) = self.prs.iter().find(|p| p.id == pr_id) else {
            return;
        };
        let (url, method, head) = (pr.url.clone(), pr.merge_method, pr.head_oid.clone());
        let worker = match &self.waker {
            Some(waker) => Latest::new(waker.clone()),
            None => Latest::default(),
        };
        worker.request((), move |()| github::set_auto_merge(&url, method, enable, &head));
        self.auto_merge_jobs.push(AutoMergeJob {
            id: pr_id.to_owned(),
            enable,
            worker,
        });
    }

    /// Whether its auto-merge is being turned on or off right now.
    pub fn is_auto_merging(&self, pr_id: &str) -> bool {
        self.auto_merge_jobs.iter().any(|job| job.id == pr_id)
    }

    pub(crate) fn save_tracked_row(&self, t: &model::Tracked) {
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

    pub(crate) fn set_pinned(&mut self, repo_name: &str, pinned: bool) {
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.pinned = pinned;
            if pinned {
                r.manual_rank = usize::MAX / 2;
            }
        }
        self.persist_repos_now();
    }

    pub(crate) fn matches_search(&self, pr: &PullRequest) -> bool {
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
    pub(crate) fn persist_repos_now(&self) {
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
}
