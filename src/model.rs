use serde::{Deserialize, Serialize};

/// Whether a PR belongs to the current user ("Mine") or someone else ("Theirs").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ownership {
    Mine,
    Theirs,
}

/// Review status of a Pull Request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewState {
    None,
    ReviewRequested,
    ChangesRequested,
    Approved,
    Commented,
}

impl ReviewState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewState::None => "NONE",
            ReviewState::ReviewRequested => "REVIEW_REQUESTED",
            ReviewState::ChangesRequested => "CHANGES_REQUESTED",
            ReviewState::Approved => "APPROVED",
            ReviewState::Commented => "COMMENTED",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "REVIEW_REQUESTED" => ReviewState::ReviewRequested,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            "APPROVED" => ReviewState::Approved,
            "COMMENTED" => ReviewState::Commented,
            _ => ReviewState::None,
        }
    }
}

/// CI check status of a Pull Request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CiStatus {
    Passing,
    Pending,
    Failing,
    None,
}

impl CiStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CiStatus::Passing => "PASSING",
            CiStatus::Pending => "PENDING",
            CiStatus::Failing => "FAILING",
            CiStatus::None => "NONE",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "PASSING" => CiStatus::Passing,
            "PENDING" => CiStatus::Pending,
            "FAILING" => CiStatus::Failing,
            _ => CiStatus::None,
        }
    }
}

/// Why an incoming ("Theirs") PR is in the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncomingReason {
    /// None (for "Mine" PRs)
    NotIncoming,
    /// Explicitly assigned or review-requested to `@me`
    AssignedOrReviewRequested,
    /// Matched a custom rule or watched repository subscription
    SubscribedRule,
}

impl IncomingReason {
    pub fn as_str(self) -> &'static str {
        match self {
            IncomingReason::NotIncoming => "MINE",
            IncomingReason::AssignedOrReviewRequested => "ASSIGNED",
            IncomingReason::SubscribedRule => "WATCHED",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "ASSIGNED" => IncomingReason::AssignedOrReviewRequested,
            "WATCHED" => IncomingReason::SubscribedRule,
            _ => IncomingReason::NotIncoming,
        }
    }
}

/// Computed urgency/status classification for a PR in the unified inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrAttention {
    /// Incoming PR directly assigned or review-requested to you (Highest priority)
    IncomingAssigned,
    /// Your open PR has no reviewer assigned OR nobody reviewed it for > open_stale_hours
    MyOpenNeedsNudge,
    /// Your open PR has changes requested
    MyOpenChangesRequested,
    /// Your draft PR has been untouched for > draft_stale_hours
    MyDraftNeedsNudge,
    /// Your open PR is approved and ready to merge
    MyOpenApproved,
    /// Incoming PR from a watched repo / rule (not directly assigned)
    IncomingWatched,
    /// Your open PR is comfortably waiting on reviewers (< open_stale_hours)
    MyOpenWaiting,
    /// Your draft PR is actively worked on (< draft_stale_hours)
    MyDraftActive,
}

impl PrAttention {
    /// Lower rank sorts higher inside a repository section.
    pub fn urgency_rank(self) -> u8 {
        match self {
            PrAttention::IncomingAssigned => 0,
            PrAttention::MyOpenNeedsNudge => 1,
            PrAttention::MyOpenChangesRequested => 2,
            PrAttention::MyDraftNeedsNudge => 3,
            PrAttention::MyOpenApproved => 4,
            PrAttention::IncomingWatched => 5,
            PrAttention::MyOpenWaiting => 6,
            PrAttention::MyDraftActive => 7,
        }
    }

    pub fn requires_action(self) -> bool {
        matches!(
            self,
            PrAttention::IncomingAssigned
                | PrAttention::MyOpenNeedsNudge
                | PrAttention::MyOpenChangesRequested
                | PrAttention::MyDraftNeedsNudge
                | PrAttention::MyOpenApproved
        )
    }

    pub fn is_nudge(self) -> bool {
        matches!(
            self,
            PrAttention::MyOpenNeedsNudge | PrAttention::MyDraftNeedsNudge
        )
    }
}

/// A Pull Request item in the unified inbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    /// Unique identifier (`owner/repo#number`)
    pub id: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub ownership: Ownership,
    pub is_draft: bool,
    pub review_state: ReviewState,
    pub ci_status: CiStatus,
    pub incoming_reason: IncomingReason,
    /// Requested reviewers or people who have reviewed
    pub reviewers: Vec<String>,
    /// Hours since last update/commit (for draft staleness)
    pub idle_hours: u64,
    /// Hours since last review activity or since opened without review (for open PR staleness)
    pub unreviewed_hours: u64,
    /// Branch name (for `gh pr checkout`)
    pub head_ref: String,
    #[serde(default)]
    pub head_repo: Option<String>,
    /// Non-default target branch eligible to form a stack.
    #[serde(default)]
    pub stack_base: Option<String>,
    /// Matched rule label if any
    pub matched_rule: Option<String>,
    /// Unix epoch hours until which nudges are snoozed (0 = not snoozed)
    pub snoozed_until_hour: u64,
    pub additions: u32,
    pub deletions: u32,
}

impl PullRequest {
    pub fn is_snoozed(&self, now_hour: u64) -> bool {
        self.snoozed_until_hour > now_hour
    }

    /// Computes how this PR should be prioritized and badged in the unified inbox.
    pub fn attention(
        &self,
        draft_stale_hours: u64,
        open_stale_hours: u64,
        now_hour: u64,
    ) -> PrAttention {
        let snoozed = self.is_snoozed(now_hour);
        match self.ownership {
            Ownership::Theirs => match self.incoming_reason {
                IncomingReason::AssignedOrReviewRequested => PrAttention::IncomingAssigned,
                _ => PrAttention::IncomingWatched,
            },
            Ownership::Mine => {
                if self.is_draft {
                    if !snoozed && self.idle_hours >= draft_stale_hours {
                        PrAttention::MyDraftNeedsNudge
                    } else {
                        PrAttention::MyDraftActive
                    }
                } else {
                    match self.review_state {
                        ReviewState::ChangesRequested => PrAttention::MyOpenChangesRequested,
                        ReviewState::Approved => PrAttention::MyOpenApproved,
                        _ => {
                            let no_reviewer = self.reviewers.is_empty();
                            let review_stale = self.unreviewed_hours >= open_stale_hours;
                            if !snoozed && (no_reviewer || review_stale) {
                                PrAttention::MyOpenNeedsNudge
                            } else {
                                PrAttention::MyOpenWaiting
                            }
                        }
                    }
                }
            }
        }
    }

    /// Human-readable age / staleness badge text.
    pub fn age_label(&self) -> String {
        let h = self.idle_hours;
        if h == 0 {
            "just now".to_string()
        } else if h < 24 {
            format!("{h}h ago")
        } else {
            let d = h / 24;
            format!("{d}d ago")
        }
    }

    /// Detailed status pill text for the row.
    pub fn status_badge(
        &self,
        draft_stale_hours: u64,
        open_stale_hours: u64,
        now_hour: u64,
    ) -> String {
        match self.attention(draft_stale_hours, open_stale_hours, now_hour) {
            PrAttention::IncomingAssigned => "ASSIGNED TO YOU".to_string(),
            PrAttention::IncomingWatched => {
                if let Some(r) = &self.matched_rule {
                    format!("WATCHED · {r}")
                } else {
                    "WATCHED REPO".to_string()
                }
            }
            PrAttention::MyDraftNeedsNudge => {
                let d = (self.idle_hours / 24).max(1);
                format!("DRAFT · {d}d IDLE — NUDGE")
            }
            PrAttention::MyDraftActive => {
                if self.is_snoozed(now_hour) {
                    "DRAFT · SNOOZED".to_string()
                } else {
                    "DRAFT".to_string()
                }
            }
            PrAttention::MyOpenNeedsNudge => {
                if self.reviewers.is_empty() {
                    "OPEN · NO REVIEWER — NUDGE".to_string()
                } else {
                    let d = (self.unreviewed_hours / 24).max(1);
                    format!("OPEN · {d}d UNREVIEWED — NUDGE")
                }
            }
            PrAttention::MyOpenChangesRequested => "OPEN · CHANGES REQ".to_string(),
            PrAttention::MyOpenApproved => "OPEN · APPROVED".to_string(),
            PrAttention::MyOpenWaiting => {
                if self.is_snoozed(now_hour) {
                    "OPEN · SNOOZED".to_string()
                } else if !self.reviewers.is_empty() {
                    format!("WAITING ON @{}", self.reviewers[0])
                } else {
                    "OPEN · WAITING".to_string()
                }
            }
        }
    }
}

/// Per-repository configuration & smart importance telemetry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoConfig {
    pub name: String,
    /// Manual rank index (0 = top). Used when OrderingMode::Custom is active.
    pub manual_rank: usize,
    /// Whether the user pinned this repository to the top.
    pub pinned: bool,
    /// Whether to subscribe to all non-draft PRs by others in this repo (without needing assignment).
    pub watch_all_open_prs: bool,
    /// Historical/onboarding telemetry: how many PRs user opened here recently.
    pub my_pr_volume: u32,
    /// Historical/onboarding telemetry: how many reviews user was assigned here recently.
    pub assigned_review_volume: u32,
    /// Whether this repository section is collapsed in the inbox view.
    pub collapsed: bool,
}

impl RepoConfig {
    /// Computes the automatic Smart Importance Score based on live inbox urgency + historical volume.
    pub fn smart_score(
        &self,
        prs: &[PullRequest],
        draft_stale_hours: u64,
        open_stale_hours: u64,
        now_hour: u64,
    ) -> u32 {
        let mut score = self.assigned_review_volume * 8 + self.my_pr_volume * 5;
        if self.pinned {
            score += 500;
        }
        for pr in prs.iter().filter(|p| p.repo == self.name) {
            score += match pr.attention(draft_stale_hours, open_stale_hours, now_hour) {
                PrAttention::IncomingAssigned => 45,
                PrAttention::MyOpenNeedsNudge => 38,
                PrAttention::MyOpenChangesRequested => 32,
                PrAttention::MyDraftNeedsNudge => 26,
                PrAttention::MyOpenApproved => 20,
                PrAttention::IncomingWatched => 12,
                PrAttention::MyOpenWaiting => 10,
                PrAttention::MyDraftActive => 6,
            };
        }
        score
    }
}

/// Custom rule for matching other people's PRs into the unified inbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchRule {
    pub id: String,
    pub name: String,
    /// GitHub search query fragment, e.g. `review-requested:@me`, `assignee:@me`, `repo:anomalyco/rune`, `label:urgent`
    pub query: String,
    pub exclude_drafts: bool,
    pub exclude_mine: bool,
    pub enabled: bool,
}

impl WatchRule {
    /// Builds the full GitHub search query string for GraphQL `search(query: ..., type: ISSUE)`.
    pub fn to_github_search_query(&self) -> String {
        let mut parts = vec!["is:pr".to_string(), "is:open".to_string()];
        if self.exclude_drafts {
            parts.push("-is:draft".to_string());
        }
        if self.exclude_mine {
            parts.push("-author:@me".to_string());
        }
        let trimmed = self.query.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
        parts.join(" ")
    }
}

/// Ordering strategy for repository groups in the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoOrderingMode {
    /// Automatically rank repositories by urgency + onboarding activity score.
    Smart,
    /// Fixed user-controlled order (drag-to-reorder or Up/Down/Pin).
    Custom,
}

/// Filter pill for the unified inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxFilter {
    /// Every PR in the inbox (Mine + Theirs)
    All,
    /// Only PRs requiring your action right now (Assigned to you, Nudge needed, Changes requested, Approved)
    ActionNeeded,
    /// Only PRs you opened (Open + Drafts)
    Mine,
    /// Only your Draft PRs
    MyDrafts,
    /// Only other people's PRs (Assigned + Watched)
    Theirs,
}

impl InboxFilter {
    pub fn index(self) -> usize {
        match self {
            InboxFilter::All => 0,
            InboxFilter::ActionNeeded => 1,
            InboxFilter::Mine => 2,
            InboxFilter::MyDrafts => 3,
            InboxFilter::Theirs => 4,
        }
    }

    pub fn from_index(idx: usize) -> Self {
        match idx {
            1 => InboxFilter::ActionNeeded,
            2 => InboxFilter::Mine,
            3 => InboxFilter::MyDrafts,
            4 => InboxFilter::Theirs,
            _ => InboxFilter::All,
        }
    }

    pub fn matches(
        self,
        pr: &PullRequest,
        draft_stale_hours: u64,
        open_stale_hours: u64,
        now_hour: u64,
    ) -> bool {
        match self {
            InboxFilter::All => true,
            InboxFilter::ActionNeeded => pr
                .attention(draft_stale_hours, open_stale_hours, now_hour)
                .requires_action(),
            InboxFilter::Mine => pr.ownership == Ownership::Mine,
            InboxFilter::MyDrafts => pr.ownership == Ownership::Mine && pr.is_draft,
            InboxFilter::Theirs => pr.ownership == Ownership::Theirs,
        }
    }
}

/// A notification entry recorded when an assignment or staleness nudge fires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pr_url: String,
    pub kind: NotificationKind,
    pub timestamp_label: String,
    pub unread: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationKind {
    InstantAssigned,
    OpenPrStaleNudge,
    DraftStaleNudge,
    ReviewActivity,
}

impl NotificationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationKind::InstantAssigned => "ASSIGNED",
            NotificationKind::OpenPrStaleNudge => "OPEN_NUDGE",
            NotificationKind::DraftStaleNudge => "DRAFT_NUDGE",
            NotificationKind::ReviewActivity => "REVIEW",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "ASSIGNED" => NotificationKind::InstantAssigned,
            "OPEN_NUDGE" => NotificationKind::OpenPrStaleNudge,
            "DRAFT_NUDGE" => NotificationKind::DraftStaleNudge,
            _ => NotificationKind::ReviewActivity,
        }
    }
}
