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
mod stacks;

use db::Database;
use github::{SyncOutput, SyncStatus};
use model::{
    CiStatus, InboxFilter, NotificationEvent, Ownership, PrAttention, PullRequest, RepoConfig,
    RepoOrderingMode, WatchRule,
};

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
    repo_menu: Option<String>,
    pub page: Page,
    pub prs: Vec<PullRequest>,
    pub repos: Vec<RepoConfig>,
    pub rules: Vec<WatchRule>,
    pub notifications: Vec<NotificationEvent>,
    pub ordering_mode: RepoOrderingMode,
    pub filter: InboxFilter,
    pub selected_repo: Option<String>,
    pub draft_stale_hours: u64,
    pub open_stale_hours: u64,
    pub onboarded: bool,
    pub default_filter: InboxFilter,
    pub native_notifications: bool,
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
        repo_menu: None,
        page: Page::Onboarding,
        prs: Vec::new(),
        repos: Vec::new(),
        rules: Vec::new(),
        notifications: Vec::new(),
        ordering_mode: RepoOrderingMode::Smart,
        filter: InboxFilter::All,
        selected_repo: None,
        draft_stale_hours: 72,
        open_stale_hours: 48,
        onboarded: false,
        default_filter: InboxFilter::All,
        native_notifications: true,
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
        next_fast_poll_at: 0.0,
        notification_interval: github::DEFAULT_POLL_INTERVAL,
        next_full_sync_at: 60.0,
        allow_os_side_effects: !headless_or_silent,
    }
}

impl App {
    pub fn ordered_repos(&self) -> Vec<RepoConfig> {
        let mut list = self.repos.clone();
        match self.ordering_mode {
            RepoOrderingMode::Smart => {
                list.sort_by(|a, b| {
                    let sb = b.smart_score(
                        &self.prs,
                        self.draft_stale_hours,
                        self.open_stale_hours,
                        current_epoch_hour(),
                    );
                    let sa = a.smart_score(
                        &self.prs,
                        self.draft_stale_hours,
                        self.open_stale_hours,
                        current_epoch_hour(),
                    );
                    sb.cmp(&sa).then_with(|| a.manual_rank.cmp(&b.manual_rank))
                });
            }
            RepoOrderingMode::Custom => {
                list.sort_by(|a, b| {
                    b.pinned
                        .cmp(&a.pinned)
                        .then_with(|| a.manual_rank.cmp(&b.manual_rank))
                });
            }
        }
        list
    }

    pub fn prs_for_repo(&self, repo_name: &str) -> Vec<PullRequest> {
        let q = self.search_query.text.trim().to_ascii_lowercase();
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
            .filter(|pr| {
                if q.is_empty() {
                    true
                } else {
                    pr.title.to_ascii_lowercase().contains(&q)
                        || pr.author.to_ascii_lowercase().contains(&q)
                        || pr.repo.to_ascii_lowercase().contains(&q)
                        || pr.number.to_string().contains(&q)
                }
            })
            .cloned()
            .collect();

        list.sort_by(|a, b| {
            let ua = a
                .attention(
                    self.draft_stale_hours,
                    self.open_stale_hours,
                    current_epoch_hour(),
                )
                .urgency_rank();
            let ub = b
                .attention(
                    self.draft_stale_hours,
                    self.open_stale_hours,
                    current_epoch_hour(),
                )
                .urgency_rank();
            ua.cmp(&ub)
                .then_with(|| b.idle_hours.cmp(&a.idle_hours))
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
        self.sync_worker
            .request((), move |()| github::fetch_live(&repos, &rules));
    }

    pub fn persist_repos(&self) {
        if self.onboarded
            && self.page == Page::Inbox
            && !self.preview
            && let Ok(db) = Database::open_default()
        {
            let _ = db.save_repos(&self.repos);
            let _ = db.set_setting("ordering_mode", self.ordering_setting());
        }
    }

    fn set_repository_order(&mut self, mode: RepoOrderingMode) {
        if mode == RepoOrderingMode::Custom {
            let mut ordered = self.ordered_repos();
            for (idx, repo) in ordered.iter_mut().enumerate() {
                repo.manual_rank = idx;
            }
            self.repos = ordered;
        }
        self.ordering_mode = mode;
        self.persist_repos();
    }

    fn ordering_setting(&self) -> &'static str {
        match self.ordering_mode {
            RepoOrderingMode::Smart => "smart",
            RepoOrderingMode::Custom => "custom",
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
                        ("default_filter", &self.default_filter.index().to_string()),
                        ("ordering_mode", self.ordering_setting()),
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
            notify::post(
                "setup-complete",
                "meat-proxy is ready",
                "PR reminders will appear here.",
                None,
                self.allow_os_side_effects,
            );
        }
        self.trigger_sync();
    }

    pub fn move_repo(&mut self, from: usize, to: usize) {
        let mut ordered = self.ordered_repos();
        if from >= ordered.len() || to >= ordered.len() || from == to {
            return;
        }
        let item = ordered.remove(from);
        ordered.insert(to, item);
        for (idx, r) in ordered.iter_mut().enumerate() {
            r.manual_rank = idx;
        }
        self.repos = ordered;
        self.ordering_mode = RepoOrderingMode::Custom;
        self.persist_repos();
    }

    pub fn toggle_pin_repo(&mut self, repo_name: &str) {
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.pinned = !r.pinned;
        }
        self.persist_repos();
    }

    pub fn toggle_watch_repo(&mut self, repo_name: &str) {
        if let Some(r) = self.repos.iter_mut().find(|r| r.name == repo_name) {
            r.watch_all_open_prs = !r.watch_all_open_prs;
        }
        self.persist_repos();
    }

    pub fn snooze_pr_24h(&mut self, pr_id: &str) {
        let until = current_epoch_hour() + 24;
        if let Some(pr) = self.prs.iter_mut().find(|p| p.id == pr_id) {
            pr.snoozed_until_hour = until;
            self.status_banner = Some(format!("Snoozed {}#{} for 24h", pr.repo, pr.number));
        }
        if let Ok(db) = Database::open_default() {
            let _ = db.update_pr_snooze(pr_id, until);
        }
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
    s.avatars.setup(decoder, waker.clone());
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
    s.fast_notif_worker.set_waker(waker);
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

    if let Some(snapshot) = s.boot_worker.take() {
        s.loading = false;
        match snapshot {
            Ok(snap) => {
                s.prs = snap.prs;
                s.repos = snap.repos;
                s.rules = snap.rules;
                s.notifications = snap.notifications;
                s.ordering_mode = snap.ordering_mode;
                s.draft_stale_hours = snap.draft_stale_hours;
                s.open_stale_hours = snap.open_stale_hours;
                s.onboarded = snap.onboarded;
                s.default_filter = snap.default_filter;
                s.filter = snap.default_filter;
                s.native_notifications = snap.native_notifications;
                s.last_notif_modified = snap.last_notif_modified;
                if s.onboarded {
                    s.page = Page::Inbox;
                }
            }
            Err(error) => s.status_banner = Some(format!("Could not read saved settings. {error}")),
        }
        s.trigger_sync();
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
                s.repos = github::infer_repos_from_prs(&out.prs, &s.repos);
                s.prs = out.prs;
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
        if deliver_events(s, notifications.events) && !s.sync_worker.is_pending() {
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
    if s.onboarded {
        tx.wake_after(std::time::Duration::from_secs_f64(
            (s.next_fast_poll_at.min(s.next_full_sync_at) - clock).max(0.01),
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
        _ => match &s.sync_status {
            SyncStatus::Connected { username } => format!("@{username}"),
            SyncStatus::Checking => "Connecting…".to_owned(),
            SyncStatus::NeedsAuth { .. } => "Disconnected".to_owned(),
        },
    };
    let keycap = row()
        .size(24.0, 23.0)
        .radius(5.0)
        .center()
        .fill(oklch(0.85, 0.10, 161.0))
        .child(rune::icon(&rune_kit::glyphs::CHECK, 18.0).color(oklch(0.22, 0.03, 160.0)));
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
                            text("meat-proxy")
                                .font(TextStyle::new(14.0, 650.0))
                                .color(c.text_base),
                        ),
                )
                .child(
                    row()
                        .center_y()
                        .gap(14.0)
                        .child(text(context).font(k.ty.mono).color(c.text_muted))
                        .child(settings_btn),
                ),
        )
        .child(el().w_full().h(1.0).fill(c.border_weak))
}

fn inbox_tools(s: &App, k: &Kit) -> El {
    let labels = ["All", "Action", "Mine", "Drafts", "Theirs"];
    let filter_seg = SegmentedControl::new(&labels, Some(s.filter.index()))
        .label("Filter")
        .width(Some(350.0))
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
        .child(search)
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
                "Repository order",
                SegmentedControl::new(
                    &["Smart", "Fixed"],
                    Some(usize::from(s.ordering_mode == RepoOrderingMode::Custom)),
                )
                .label("Repository order")
                .width(Some(168.0))
                .on_pick(|s: &mut App, idx, _| {
                    s.set_repository_order(if idx == 0 {
                        RepoOrderingMode::Smart
                    } else {
                        RepoOrderingMode::Custom
                    });
                })
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
        doc = doc.child(choices)
            .child(text("Repositories").font(k.ty.ui_medium).color(c.text_base))
            .child(text("Smart order prioritizes PRs that need attention. Watch a repository to include others’ open PRs.").wrap()
                .font(k.ty.small).color(c.text_muted));

        let ordered = s.ordered_repos();
        let visible: Vec<_> = ordered
            .iter()
            .enumerate()
            .filter(|(_, repo)| {
                repo.pinned
                    || repo.watch_all_open_prs
                    || s.prs.iter().any(|pr| pr.repo == repo.name)
            })
            .collect();
        if visible.is_empty() {
            doc = doc.child(
                text("No repositories found in your open PRs or review queue.")
                    .font(k.ty.body)
                    .color(c.text_muted),
            );
        }
        for (position, &(idx, repo)) in visible.iter().enumerate() {
            let name = repo.name.clone();
            let up = position.checked_sub(1).map(|i| visible[i].0);
            let down = visible.get(position + 1).map(|(i, _)| *i);
            doc = doc.child(
                setup_row(
                    k,
                    &repo.name,
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(
                            Switch::new(repo.watch_all_open_prs)
                                .label("Watch open PRs")
                                .on_toggle(move |s: &mut App| s.toggle_watch_repo(&name))
                                .build(k),
                        )
                        .child(
                            Button::new("↑")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Small)
                                .disabled(up.is_none())
                                .on_press(move |s: &mut App| {
                                    if let Some(up) = up {
                                        s.move_repo(idx, up);
                                    }
                                })
                                .build(k),
                        )
                        .child(
                            Button::new("↓")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Small)
                                .disabled(down.is_none())
                                .on_press(move |s: &mut App| {
                                    if let Some(down) = down {
                                        s.move_repo(idx, down);
                                    }
                                })
                                .build(k),
                        ),
                )
                .key(format!("onboard-repo-{}", repo.name)),
            );
        }
    }

    let mut page = col().w_full().h_full().min_h(0.0).child(
        row()
            .w_full()
            .px(28.0)
            .justify_center()
            .child(doc)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .grow(1.0)
            .min_h(0.0),
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

fn default_filter_row(s: &App, k: &Kit) -> El {
    setup_row(
        k,
        "Default inbox",
        SegmentedControl::new(
            &["All", "Action", "Mine", "Drafts", "Theirs"],
            Some(s.default_filter.index()),
        )
        .label("Default inbox")
        .width(Some(324.0))
        .on_pick(|s: &mut App, idx, _| {
            s.default_filter = InboxFilter::from_index(idx);
            if s.onboarded
                && s.settings_open
                && let Ok(db) = Database::open_default()
            {
                let _ = db.set_setting("default_filter", &idx.to_string());
            }
        })
        .build(k)
        .key("default-inbox-filter"),
    )
}

const CLOCK: rune::Icon = rune::Icon {
    name: "snooze-clock",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="5.8" stroke="currentColor" stroke-width="1.3"/><path d="M8 4.6v3.6l2.5 1.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

const MORE: rune::Icon = rune::Icon {
    name: "more-actions",
    view: 16.0,
    body: r#"<circle cx="3.5" cy="8" r="1.2"/><circle cx="8" cy="8" r="1.2"/><circle cx="12.5" cy="8" r="1.2"/>"#,
};

// Matching the resting surface keeps the glyph quiet. Keyboard focus paints a
// contrasting tile behind it even when the pointer is in another row.
fn row_action(
    k: &Kit,
    glyph: &'static rune::Icon,
    label: &str,
    background: Color,
    open: bool,
) -> El {
    row()
        .role("button")
        .label(label)
        .help(label)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .size(26.0, 24.0)
        .radius(5.0)
        .center()
        .focus_highlight(oklch(if k.dark { 0.54 } else { 0.61 }, 0.025, 160.0))
        .child(
            rune::icon(glyph, 15.0)
                .color(if open { k.c.text_muted } else { background })
                .group_color(k.c.text_muted)
                .a11y_hidden(),
        )
}

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
        author.child(text(handle).font(k.ty.small).color(k.c.text_muted))
    }
}

fn minimal_pr_row(s: &App, item: &stacks::StackRow, k: &Kit) -> El {
    let pr = item.pr;
    let c = &k.c;
    let att = pr.attention(
        s.draft_stale_hours,
        s.open_stale_hours,
        current_epoch_hour(),
    );

    let (status, tone) = match att {
        PrAttention::IncomingAssigned => ("Review requested".to_owned(), mint(k)),
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
    let pr_url_copy = pr.url.clone();
    let pr_id_snooze = pr.id.clone();

    let mut right_side = row().center_y().gap(2.0);
    if att.is_nudge() {
        right_side = right_side
            .child(
                row_action(
                    k,
                    &rune_kit::glyphs::COPY,
                    "Copy PR link",
                    surface(k),
                    false,
                )
                .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    tx.copy(pr_url_copy.clone());
                    s.status_banner = Some("Copied link".to_string());
                })
                .key(format!("copy-ping-{}", pr.id)),
            )
            .child(
                row_action(k, &CLOCK, "Snooze for 24 hours", surface(k), false)
                    .on_press(move |s: &mut App| {
                        s.snooze_pr_24h(&pr_id_snooze);
                    })
                    .key(format!("snooze-{}", pr.id)),
            );
    }

    let mut metadata = row()
        .center_y()
        .gap(9.0)
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
    row()
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
            notify::open_url_in_browser(
                &pr_url_open,
                s.allow_os_side_effects,
            );
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
        .child(right_side)
}

fn repository_actions(
    s: &App,
    k: &Kit,
    repo: &RepoConfig,
    previous: Option<usize>,
    next: Option<usize>,
    index: usize,
    overlay: &mut Vec<El>,
) -> El {
    use rune_kit::{Menu, MenuItem, Placement};
    let name = repo.name.clone();
    let trigger = row_action(
        k,
        &MORE,
        &format!("Actions for {}", repo.name),
        k.c.layer_01,
        s.repo_menu.as_deref() == Some(repo.name.as_str()),
    )
    .on_press(move |s: &mut App| s.repo_menu = Some(name.clone()))
    .key(format!("repo-actions-{}", repo.name));
    let watch = repo.name.clone();
    let pin = repo.name.clone();
    let fixed = s.ordering_mode == RepoOrderingMode::Custom;
    Menu::new(format!("repo-menu-{}", repo.name))
        .placement(Placement::BottomEnd)
        .open(s.repo_menu.as_deref() == Some(repo.name.as_str()))
        .trigger(trigger)
        .min_width(206.0)
        .on_close(|s: &mut App| s.repo_menu = None)
        .item(
            MenuItem::new("Watch all open PRs")
                .checked(repo.watch_all_open_prs)
                .on_select(move |s: &mut App| {
                    s.repo_menu = None;
                    s.toggle_watch_repo(&watch);
                    s.trigger_sync();
                }),
        )
        .item(
            MenuItem::new("Pin repository")
                .checked(repo.pinned)
                .on_select(move |s: &mut App| {
                    s.repo_menu = None;
                    s.toggle_pin_repo(&pin);
                }),
        )
        .separator()
        .item(
            MenuItem::new("Move up")
                .disabled(previous.is_none())
                .on_select(move |s: &mut App| {
                    s.repo_menu = None;
                    if let Some(to) = previous {
                        s.move_repo(index, to);
                    }
                }),
        )
        .item(
            MenuItem::new("Move down")
                .disabled(next.is_none())
                .on_select(move |s: &mut App| {
                    s.repo_menu = None;
                    if let Some(to) = next {
                        s.move_repo(index, to);
                    }
                }),
        )
        .separator()
        .item(
            MenuItem::new("Smart repository order")
                .checked(!fixed)
                .on_select(move |s: &mut App| {
                    s.repo_menu = None;
                    s.set_repository_order(if fixed {
                        RepoOrderingMode::Smart
                    } else {
                        RepoOrderingMode::Custom
                    });
                }),
        )
        .build(k, overlay)
}

fn document_inbox_view(s: &App, k: &Kit, overlay: &mut Vec<El>) -> El {
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
            col().gap(5.0).child(heading(k, "Pull requests")).child(
                text(format!(
                    "{} open  ·  {} need attention",
                    s.prs.len(),
                    attention
                ))
                .font(k.ty.body)
                .color(c.text_muted),
            ),
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

    let groups: Vec<_> = ordered
        .iter()
        .enumerate()
        .filter(|(_, repo)| {
            s.selected_repo
                .as_ref()
                .is_none_or(|name| name == &repo.name)
        })
        .filter_map(|(idx, repo)| {
            let prs = s.prs_for_repo(&repo.name);
            let keep_empty = (repo.pinned || repo.watch_all_open_prs)
                && s.filter == InboxFilter::All
                && s.search_query.text.trim().is_empty();
            (!prs.is_empty() || keep_empty).then_some((idx, repo, prs))
        })
        .collect();
    for (position, (idx, repo, prs)) in groups.iter().enumerate() {
        let repo_name_filter = repo.name.clone();
        let is_filtered_repo = s.selected_repo.as_deref() == Some(repo.name.as_str());

        let section_header = col()
            .key(format!("repo-header-{}", repo.name))
            .w_full()
            .child(
                row()
                    .w_full()
                    .px(14.0)
                    .py(10.0)
                    .fill(c.layer_01)
                    .hover_group()
                    .center_y()
                    .justify_between()
                    .child(
                        row()
                            .grow(1.0)
                            .center_y()
                            .gap(8.0)
                            .cursor(rune::Cursor::Pointer)
                            .on_press(move |s: &mut App| {
                                if s.selected_repo.as_deref() == Some(repo_name_filter.as_str()) {
                                    s.selected_repo = None;
                                } else {
                                    s.selected_repo = Some(repo_name_filter.clone());
                                }
                            })
                            .child(rune::icon(&REPO_ICON, 15.0).color(c.text_muted))
                            .child(
                                text(repo.name.clone())
                                    .font(TextStyle::new(13.0, 650.0))
                                    .color(c.text_muted),
                            )
                            .child(if is_filtered_repo {
                                text("(filtered · click to show all)")
                                    .font(k.ty.small)
                                    .color(c.text_faint)
                            } else {
                                text(format!("{}", prs.len()))
                                    .font(k.ty.small)
                                    .color(c.text_faint)
                            }),
                    )
                    .child(if s.page == Page::Inbox {
                        let previous = groups[..position]
                            .iter()
                            .rev()
                            .find(|(_, r, _)| r.pinned == repo.pinned)
                            .map(|(i, _, _)| *i);
                        let next = groups[position + 1..]
                            .iter()
                            .find(|(_, r, _)| r.pinned == repo.pinned)
                            .map(|(i, _, _)| *i);
                        repository_actions(s, k, repo, previous, next, *idx, overlay)
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
            .child(section_header);
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
            list = list.child(minimal_pr_row(s, item, k));
        }

        doc = doc.child(list);
    }

    if groups.is_empty() {
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
            row()
                .w_full()
                .px(28.0)
                .justify_center()
                .child(doc)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0)
                .min_h(0.0),
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
        .gap(12.0)
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
                .py(6.0)
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
                .py(6.0)
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
                        .child(text(rule.name.clone()).font(k.ty.body).color(c.text_base))
                        .child(
                            text(rule.to_github_search_query())
                                .font(k.ty.mono)
                                .color(c.text_faint),
                        ),
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
                .placeholder("Label (e.g. Core PRs)")
                .width(Some(180.0))
                .on_edit(|s: &mut App| &mut s.new_rule_name)
                .build(k),
        )
        .child(
            TextInput::new(&s.new_rule_query)
                .placeholder("GitHub query (e.g. repo:anomalyco/rune label:core)")
                .width(Some(340.0))
                .on_edit(|s: &mut App| &mut s.new_rule_query)
                .build(k),
        )
        .child(
            Button::new("Add Rule")
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
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text("Subscription rules")
                        .font(k.ty.ui_medium)
                        .color(c.text_base),
                )
                .child(
                    Button::new("Sync now")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut App| s.trigger_sync())
                        .build(k),
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
        .child(
            panel(k).child(default_filter_row(s, k)).child(setup_row(
                k,
                "macOS notifications",
                row()
                    .center_y()
                    .gap(10.0)
                    .child(
                        Button::new("Send test notification")
                            .variant(ButtonVariant::Ghost)
                            .control_size(ControlSize::Small)
                            .disabled(!s.native_notifications)
                            .on_press(|s: &mut App| {
                                notify::post(
                                    "test-notification",
                                    "meat-proxy test",
                                    "PR reminders appear in Notification Center.",
                                    None,
                                    s.allow_os_side_effects,
                                )
                            })
                            .build(k)
                            .key("test-notification"),
                    )
                    .child(
                        Switch::new(s.native_notifications)
                            .label("macOS notifications")
                            .hide_label()
                            .on_toggle(|s: &mut App| {
                                s.native_notifications = !s.native_notifications;
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
        .child(panel(k).child(thresholds_section))
        .child(panel(k).child(rules_section));

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

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let mut k = Kit::neutral(cx.system_dark());
    k.c.bg_base = oklch(if k.dark { 0.15 } else { 0.985 }, 0.003, 175.0);

    let mut overlay = Vec::new();
    let body = match s.page {
        Page::Onboarding | Page::Preferences => onboarding_page_view(s, &k),
        Page::Inbox => document_inbox_view(s, &k, &mut overlay),
    };

    let root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(top_bar(s, &k))
        .child(body.grow(1.0).min_h(0.0))
        .children(overlay);
    if s.settings_open && s.page == Page::Inbox {
        root.child(settings_modal(s, &k))
    } else {
        root
    }
}

fn on_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.is_command(',') && s.page == Page::Inbox {
        s.repo_menu = None;
        s.settings_open = !s.settings_open;
        return true;
    }
    if ev.is_command('r') {
        s.trigger_sync();
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
            s.page = Page::Inbox;
            s.settings_open = true;
        }),
        ("rules", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.settings_open = true;
        }),
        ("action-needed", |s| {
            seed_preview(s);
            s.page = Page::Inbox;
            s.filter = InboxFilter::ActionNeeded;
        }),
    ];

    rune::App::new(rune::AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes,
        ..rune::AppSpec::new("meat-proxy", (920.0, 680.0), init, view)
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
        rt.state.default_filter = InboxFilter::ActionNeeded;

        let (x, y) = rt.find("confirm-onboarding-btn").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(3.0 / 60.0);
        assert_eq!(rt.state.page, Page::Inbox);
        assert_eq!(rt.state.filter, InboxFilter::ActionNeeded);
        assert!(rt.find("pr-row-anomalyco/rune#418").is_some());
    }

    #[test]
    fn saves_confirmed_preferences_for_next_launch() {
        let db = Database::open_in_memory().unwrap();
        db.save_onboarding(
            &[],
            &[],
            &[
                ("default_filter", "3"),
                ("ordering_mode", "custom"),
                ("draft_stale_hours", "120"),
                ("open_stale_hours", "72"),
                ("native_notifications", "0"),
            ],
        )
        .unwrap();

        let snapshot = db.load_snapshot().unwrap();
        assert!(snapshot.onboarded);
        assert_eq!(snapshot.default_filter, InboxFilter::MyDrafts);
        assert_eq!(snapshot.ordering_mode, RepoOrderingMode::Custom);
        assert_eq!(snapshot.draft_stale_hours, 120);
        assert_eq!(snapshot.open_stale_hours, 72);
        assert!(!snapshot.native_notifications);
    }
}
