use rune::{
    Chrome, Color, Cx, El, KeyEvent, Latest, ScrollSpec, TextEdit, TextStyle, Tx, col, el, oklch,
    row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, SegmentedControl, Switch, TextInput};

mod avatars;
mod db;
mod github;
mod judge;
mod model;
mod notify;
mod reorder;
mod sections;
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
/// A pull request that just arrived slides in over this long, and its row stays tinted for the longer one.
const ARRIVAL_SLIDE_SECS: f32 = 0.45;
const ARRIVAL_GLOW_SECS: f32 = 2.6;
/// Gap between rows arriving together, so a batch cascades.
const ARRIVAL_STAGGER_SECS: f32 = 0.07;

/// The first-launch intro: the button springs in, a finger presses it, the name appears, then the
/// setup screen fades in. A click or Escape skips it; Reduce Motion never plays it.
const INTRO_SECS: f32 = 5.0;
/// When the finger reaches the button.
const INTRO_HIT: f32 = 1.45;

/// The section ignored pull requests move to, always last and folded until opened.
const IGNORED_GROUP: u8 = 255;

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
    pub boot_worker: Latest<Result<db::AppSnapshot, rusqlite::Error>>,
    pub sync_worker: Latest<Result<SyncOutput, String>>,
    pub judge: Option<judge::JudgeConfig>,
    /// Sort with the model's verdicts (true) or by rules alone (false); unset follows what exists.
    pub smart_sort: Option<bool>,
    /// The Inbox sections (built in).
    pub layout: sections::Layout,
    /// Hide outside contributors' pull requests from the Inbox tab.
    pub hide_external: bool,
    /// List requests made of a team you are in, not of you by name, in the Inbox too.
    pub show_team_reviews: bool,
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
        max_age_input: TextEdit::new(&github::DEFAULT_MAX_AGE_DAYS.to_string()),
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
        boot_worker: Latest::default(),
        sync_worker: Latest::default(),
        judge: None,
        smart_sort: None,
        layout: sections::Layout::default(),
        hide_external: false,
        show_team_reviews: false,
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
            && !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.set_setting("inbox_layout", &self.layout.fingerprint);
        }
        self.normalize_group_order();
        self.max_age_input = TextEdit::new(&self.sync_options.max_age_days.to_string());
        self.last_notif_modified = snap.last_notif_modified;
        self.organization = TextEdit::new(&snap.organization);
        self.judgments = snap.judgments;
        self.smart_sort = snap.smart_sort;
        self.hide_external = snap.hide_external;
        self.show_team_reviews = snap.show_team_reviews;
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
        (self.judge.is_some() || !self.judgments.is_empty())
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
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.set_setting("smart_sort", if next { "1" } else { "0" });
        }
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
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
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
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.set_setting(
                "show_team_reviews",
                if self.show_team_reviews { "1" } else { "0" },
            );
        }
    }

    pub fn toggle_hide_external(&mut self) {
        self.hide_external = !self.hide_external;
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.set_setting("hide_external", if self.hide_external { "1" } else { "0" });
        }
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
        if self.judge_off || self.preview || self.judge_worker.is_pending() {
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
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
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
        let (id, url, method) = (pr.id.clone(), pr.url.clone(), pr.merge_method);
        self.auto_merge_worker.request((), move |()| {
            (id, enable, github::set_auto_merge(&url, method, enable))
        });
    }

    fn save_tracked_row(&self, t: &model::Tracked) {
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.save_tracked(t);
        }
    }

    /// Remove the faded row of a pull request that auto-merge finished.
    pub fn clear_tracked(&mut self, pr_id: &str) {
        self.tracked.retain(|t| t.pr.id != pr_id);
        if !self.preview
            && let Ok(db) = Database::open_default()
        {
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
                    if !self.preview
                        && let Ok(db) = Database::open_default()
                    {
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
    if !s.preview && s.allow_os_side_effects {
        s.judge = judge::JudgeConfig::load();
    }
    s.auth_worker.set_waker(waker.clone());
    s.fast_notif_worker.set_waker(waker.clone());
    s.discovery_worker.set_waker(waker.clone());
    s.verify_worker.set_waker(waker.clone());
    s.auto_merge_worker.set_waker(waker.clone());
    s.resolve_worker.set_waker(waker.clone());
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
                s.note_arrivals(&out.prs);
                s.prs = out.prs;
                if s.rank_refresh_pending {
                    s.refresh_ranks();
                }
                s.queue_judgments();
                s.after_sync_track();
                let events: Vec<_> = events
                    .into_iter()
                    .filter(|e| !s.is_ignored_url(&e.pr_url))
                    .collect();
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
                        .opacity(if hide_brand { 0.0 } else { 1.0 })
                        .opacity_motion(rune::Anim::INSTANT)
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

/// Says how the Inbox is sorted, and lets you change it: AI (a model reads the review requests) or
/// rules alone. Next to it, a button puts the groups back in the default order.
fn sort_control(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let ai = s.smart_active();
    let available = s.smart_available();
    let tip = if !available {
        "Sorted by rules. Add a model API key in ~/.config/meat-proxy/judge.json to sort with AI."
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
    row().center_y().gap(2.0).child(toggle).child(reset)
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
    let dock = if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 };
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
                        (0.0, green.alpha(if k.dark { 0.42 } else { 0.24 } * (0.6 + 0.4 * press.max(0.0)))),
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
            .child(art(s.intro_art.first(), 200.0).absolute(0.0, 12.0).opacity(1.0 - logo))
            .child(art(s.intro_art.get(1), 200.0).absolute(0.0, 0.0).offset(0.0, sink).opacity(1.0 - logo))
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
                .offset(angle.cos() * distance * p, angle.sin() * distance * p + 40.0 * p * p)
                .opacity(if t < from { 0.0 } else { (p * 6.0).min(1.0) * (1.0 - phase(t, from + 0.7, from + 1.3)) })
                .child(text(label).font(TextStyle::new(14.0, 600.0)).color(mint(k))),
        ));
    }
    let fall = phase(t, 0.8, INTRO_HIT).powi(3);
    let lift = phase(t, 1.9, 2.6).powi(2);
    let drop = if t >= INTRO_HIT { sink } else { -360.0 * (1.0 - fall) };
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
        .offset(lerp(0.0, 115.0 - (cx - name_w / 2.0)), lerp(0.0, 25.0 - (name_top + name_h / 2.0)));
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
    layer
        .child(frame_exact(name))
        .child(frame_exact(
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
        doc = doc.child(text(message.clone()).wrap().font(k.ty.small).color(c.text_muted));
    }

    let (glyph, status) = if let Some(username) = connected {
        ("✓", format!("Signed in as @{username} through GitHub CLI."))
    } else if s.auth_worker.is_pending() {
        ("❯_", "Waiting for you to finish in the browser…".to_string())
    } else if s.loading || s.sync_worker.is_pending() || matches!(s.sync_status, SyncStatus::Checking) {
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
            .child(text("❯ gh auth login --web").font(k.ty.mono).color(c.text_base));
        for line in lines {
            term = term.child(text(line).wrap().font(k.ty.mono).color(c.text_muted));
        }
        card = card.child(el().w_full().h(1.0).fill(c.border_weak)).child(term);
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
            doc = doc.child(text(reason.clone()).wrap().font(k.ty.small).color(c.text_faint));
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

/// Auto-merge: a bolt.
const BOLT: rune::Icon = rune::Icon {
    name: "auto-merge",
    view: 16.0,
    body: r#"<path fill="currentColor" d="M9.2 1.5 3.8 9h3.5l-.7 5.5L12.2 7H8.7l.5-5.5Z"/>"#,
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
        PrAttention::MyOpenApproved => (pr.approval_label(), mint(k)),
        PrAttention::MyOpenWaiting => ("Waiting for review".to_owned(), c.text_muted),
        PrAttention::MyDraftActive => ("Draft".to_owned(), c.text_muted),
        PrAttention::IncomingWatched => ("Watching".to_owned(), c.text_muted),
    };
    let pr_url_open = pr.url.clone();

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
        .child(
            text(pr.age_label())
                .font(k.ty.small)
                .color(c.text_muted)
                .help(pr.age_detail()),
        )
        .child(el().size(1.0, 10.0).fill(c.border_base))
        .child(text(status).font(k.ty.small_medium).color(tone));
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
    line
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
    // Not mergeable yet: GitHub can merge it by itself once it is, which is its own state.
    let auto_on = pr.auto_merge == model::AutoMerge::On;
    let auto_offer = !ready && s.auto_merge_target(&pr.id) == Some(true);
    let auto = auto_on || auto_offer;
    let auto_busy = s.auto_merging.contains(&pr.id);

    let button = if merging {
        Button::new("Merging…")
            .variant(ButtonVariant::Outline)
            .disabled(true)
    } else if armed {
        Button::new("Confirm merge").variant(ButtonVariant::Submit)
    } else if auto_on {
        Button::new("Auto-merge on")
            .variant(ButtonVariant::Submit)
            .icon(&BOLT)
            .disabled(auto_busy)
    } else if auto_offer {
        Button::new("Auto-merge")
            .variant(ButtonVariant::Outline)
            .icon(&BOLT)
            .disabled(auto_busy)
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
    .on_press(move |s: &mut App| {
        if auto {
            s.press_auto_merge(&id)
        } else {
            s.press_merge(&id)
        }
    })
    .build(k)
    .key(format!("merge-{}", pr.id));

    row()
        .center_y()
        .help(if auto_on {
            "Merges by itself as soon as GitHub allows it. Click to turn off".to_owned()
        } else if auto_offer {
            format!(
                "Merge by itself once GitHub allows it ({}). Click to turn on",
                pr.merge_readiness.blocked_reason().to_lowercase()
            )
        } else if armed {
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
    let now = current_epoch_hour();
    let attention = s.prs.iter().filter(|p| s.needs_you(p, now)).count();
    doc = doc
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
    );

    let thresholds_section = col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text("Notifications")
                        .font(k.ty.ui_medium)
                        .color(c.text_base),
                )
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
            col()
                .gap(2.0)
                .child(
                    text("Extra GitHub searches")
                        .font(k.ty.body)
                        .color(c.text_base),
                )
                .child(
                    text("Add pull requests you are not involved in, such as every PR in a repo. They show under Theirs and All.")
                        .font(k.ty.small)
                        .color(c.text_faint),
                ),
        )
        .child(rules_list)
        .child(add_rule_bar);

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
            let window = cx.layout_size("root").map_or((920.0, 680.0), |[w, h]| (w, h));
            let name = cx.layout_size("intro-name").map_or((340.0, 77.0), |[w, h]| (w, h));
            root.child(intro_view(s, &k, t, window, name).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)))
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
    // What a model would have said about some of the demo review requests, and a rule about one.
    let model = "gpt-4o-mini";
    for (id, trivial, urgent, risky, by) in [
        ("acme/engine#418", 0.04, 0.83, 0.12, model),
        ("acme/cli#904", 0.03, 0.04, 0.81, model),
        ("acme/cli#910", 0.95, 0.03, 0.05, "rules"),
        ("acme/engine#920", 0.1, 0.05, 0.12, model),
        ("acme/cli#931", 0.96, 0.02, 0.03, model),
    ] {
        s.judgments.insert(
            id.to_owned(),
            judge::Judgment {
                answers: [("trivial", trivial), ("urgent", urgent), ("risky", risky)]
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
    let scenes: &[(&str, fn(&mut App))] = &[
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
            s.intro = Some(std::time::Instant::now() - std::time::Duration::from_secs_f32(INTRO_HIT + 0.12));
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
        assert_eq!(state.group_order, [6, 0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11]);
        // Drag the first to the bottom of what is shown: it lands after group 3.
        state.drop_group(&[6, 0, 3], 0, 2);
        assert_eq!(state.group_order, [0, 1, 2, 3, 6, 4, 5, 7, 8, 9, 10, 11]);
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
        state.judgments.insert(
            pr.id.clone(),
            said(&[("trivial", 0.9), ("urgent", 0.02), ("risky", 0.02)], "m"),
        );
        assert_eq!(
            section_name(&state, &pr).as_deref(),
            Some("Approve at will")
        );
        state
            .judgments
            .insert(huge.id.clone(), said(&[("trivial", 0.95)], "m"));
        assert_eq!(
            section_name(&state, &huge).as_deref(),
            Some("Approve at will"),
            "trivial beats big"
        );
        state.judgments.insert(
            pr.id.clone(),
            said(&[("trivial", 0.9), ("urgent", 0.7)], "m"),
        );
        assert_eq!(section_name(&state, &pr).as_deref(), Some("Do first"));
        state.judgments.insert(
            pr.id.clone(),
            said(&[("trivial", 0.9), ("risky", 0.8)], "m"),
        );
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
            .insert(mine.id.clone(), said(&[("trivial", 0.99)], "m"));
        assert_eq!(section_name(&state, &mine).as_deref(), Some("Merge these"));
    }

    #[test]
    fn rules_alone_ignore_the_models_answers_but_keep_the_rules_own() {
        let mut state = init();
        state.preview = true;
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
