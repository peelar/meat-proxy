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
    /// Review requested from a team the user belongs to, not from them by name
    TeamReviewRequested,
    /// Matched a custom rule or watched repository subscription
    SubscribedRule,
}

impl IncomingReason {
    pub fn as_str(self) -> &'static str {
        match self {
            IncomingReason::NotIncoming => "MINE",
            IncomingReason::AssignedOrReviewRequested => "ASSIGNED",
            IncomingReason::TeamReviewRequested => "TEAM",
            IncomingReason::SubscribedRule => "WATCHED",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "ASSIGNED" => IncomingReason::AssignedOrReviewRequested,
            "TEAM" => IncomingReason::TeamReviewRequested,
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
    /// Review requested from a team you are in, not from you by name
    IncomingViaTeam,
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
            PrAttention::IncomingViaTeam => 5,
            PrAttention::IncomingWatched => 6,
            PrAttention::MyOpenWaiting => 7,
            PrAttention::MyDraftActive => 8,
        }
    }

    /// Which "what do I do" group the Inbox tab files this under: its position and heading.
    pub fn action_group(self) -> (u8, &'static str) {
        match self {
            PrAttention::IncomingAssigned => (0, "Review these"),
            PrAttention::MyOpenChangesRequested => (1, "Address feedback"),
            PrAttention::MyOpenApproved => (2, "Merge these"),
            PrAttention::IncomingViaTeam => (3, "Team reviews"),
            PrAttention::MyOpenNeedsNudge => (4, "Get a reviewer"),
            PrAttention::MyDraftNeedsNudge => (5, "Finish or close"),
            PrAttention::IncomingWatched
            | PrAttention::MyOpenWaiting
            | PrAttention::MyDraftActive => (6, "Other"),
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
                | PrAttention::IncomingViaTeam
        )
    }

    pub fn is_nudge(self) -> bool {
        matches!(
            self,
            PrAttention::MyOpenNeedsNudge | PrAttention::MyDraftNeedsNudge
        )
    }
}

/// Whether GitHub would let the PR be merged right now, and if not, why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeReadiness {
    #[default]
    Unknown,
    Ready,
    NeedsReview,
    /// Mergeable, but a check that no rule requires is pending or failing.
    Unstable,
    Behind,
    Conflicts,
}

impl MergeReadiness {
    /// From GraphQL `mergeable` and `mergeStateStatus`. `UNSTABLE` still merges: only checks no
    /// rule requires are pending or failing. `BLOCKED`, `BEHIND` and `DIRTY` need something first.
    pub fn from_github(mergeable: &str, status: &str) -> Self {
        match (mergeable, status) {
            ("CONFLICTING", _) | (_, "DIRTY") => MergeReadiness::Conflicts,
            (_, "CLEAN" | "HAS_HOOKS") => MergeReadiness::Ready,
            (_, "BLOCKED") => MergeReadiness::NeedsReview,
            (_, "UNSTABLE") => MergeReadiness::Unstable,
            (_, "BEHIND") => MergeReadiness::Behind,
            _ => MergeReadiness::Unknown,
        }
    }

    pub fn is_ready(self) -> bool {
        matches!(self, MergeReadiness::Ready | MergeReadiness::Unstable)
    }

    /// Why the merge button is off.
    pub fn blocked_reason(self) -> &'static str {
        match self {
            MergeReadiness::Ready => "Ready to merge",
            MergeReadiness::NeedsReview => "Waiting on a required review or check",
            MergeReadiness::Unstable => "Some checks haven't passed, but none is required",
            MergeReadiness::Behind => "Branch is behind its base",
            MergeReadiness::Conflicts => "Has merge conflicts",
            MergeReadiness::Unknown => "GitHub is still working out if this can merge",
        }
    }
}

/// The merge method the repository prefers for this user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeMethod {
    Merge,
    #[default]
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn from_github(s: &str) -> Self {
        match s {
            "MERGE" => MergeMethod::Merge,
            "REBASE" => MergeMethod::Rebase,
            _ => MergeMethod::Squash,
        }
    }

    pub fn flag(self) -> &'static str {
        match self {
            MergeMethod::Merge => "--merge",
            MergeMethod::Squash => "--squash",
            MergeMethod::Rebase => "--rebase",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MergeMethod::Merge => "merge commit",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        }
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
    #[serde(default)]
    pub merge_readiness: MergeReadiness,
    #[serde(default)]
    pub merge_method: MergeMethod,
}

impl PullRequest {
    /// How much this PR deserves a place near the top of the Inbox tab. Someone waiting on you
    /// comes first, then your own PRs that are blocked or ready to ship, then nudges; the longer
    /// a PR has waited, the higher it climbs within its group.
    pub fn importance(&self, draft_stale_hours: u64, open_stale_hours: u64, now_hour: u64) -> u32 {
        let base: u32 = match self.attention(draft_stale_hours, open_stale_hours, now_hour) {
            PrAttention::IncomingAssigned => 100,
            PrAttention::MyOpenChangesRequested => 80,
            PrAttention::MyOpenApproved => 70,
            PrAttention::IncomingViaTeam => 55,
            PrAttention::MyOpenNeedsNudge => 50,
            PrAttention::MyDraftNeedsNudge => 40,
            PrAttention::IncomingWatched => 20,
            PrAttention::MyOpenWaiting => 10,
            PrAttention::MyDraftActive => 5,
        };
        let waited_days = (self.unreviewed_hours.max(self.idle_hours) / 24).min(12) as u32;
        let ci =
            u32::from(self.ci_status == CiStatus::Failing && self.ownership == Ownership::Mine)
                * 10;
        base + waited_days * 2 + ci
    }

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
                IncomingReason::TeamReviewRequested => PrAttention::IncomingViaTeam,
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
            PrAttention::IncomingViaTeam => "TEAM REVIEW REQUESTED".to_string(),
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
    /// Historical/onboarding telemetry: how many PRs user opened here recently.
    pub my_pr_volume: u32,
    /// Historical/onboarding telemetry: how many reviews user was assigned here recently.
    pub assigned_review_volume: u32,
    /// Whether this repository section is collapsed in the inbox view.
    pub collapsed: bool,
    /// The user deleted this repository from the list. Kept so it is not re-added from synced PRs.
    pub removed: bool,
    /// Recent engagement points from GitHub (PRs opened and reviews done lately).
    pub activity: u32,
}

/// Share of a PR's weight that remains after it has sat idle. Fresh PRs count in full,
/// months-old ones barely register.
fn recency_percent(idle_hours: u64) -> u64 {
    let idle_days = idle_hours / 24;
    (100 / (1 + idle_days / 14)).max(5)
}

impl RepoConfig {
    /// Engagement alone can't outrank a repo that has real work waiting in it.
    pub const ACTIVITY_CAP: u32 = 40;

    /// Computes the automatic Smart Importance Score based on live inbox urgency + historical volume.
    pub fn smart_score(
        &self,
        prs: &[PullRequest],
        draft_stale_hours: u64,
        open_stale_hours: u64,
        now_hour: u64,
    ) -> u32 {
        let mut score = self.assigned_review_volume * 8
            + self.my_pr_volume * 5
            + self.activity.min(Self::ACTIVITY_CAP);
        if self.pinned {
            score += 500;
        }
        for pr in prs.iter().filter(|p| p.repo == self.name) {
            let weight: u64 = match pr.attention(draft_stale_hours, open_stale_hours, now_hour) {
                PrAttention::IncomingAssigned => 45,
                PrAttention::IncomingViaTeam => 16,
                PrAttention::MyOpenNeedsNudge => 38,
                PrAttention::MyOpenChangesRequested => 32,
                PrAttention::MyDraftNeedsNudge => 26,
                PrAttention::MyOpenApproved => 20,
                PrAttention::IncomingWatched => 12,
                PrAttention::MyOpenWaiting => 10,
                PrAttention::MyDraftActive => 6,
            };
            score += (weight * recency_percent(pr.idle_hours) / 100) as u32;
        }
        score
    }
}

/// One line describing a repository's PRs, for its collapsed header: the most useful counts first.
pub fn summarize_prs(prs: &[PullRequest]) -> String {
    let count = |f: &dyn Fn(&PullRequest) -> bool| prs.iter().filter(|pr| f(pr)).count();
    let parts = [
        (
            count(&|p| p.incoming_reason == IncomingReason::AssignedOrReviewRequested),
            "need your review",
        ),
        (
            count(&|p| p.incoming_reason == IncomingReason::TeamReviewRequested),
            "team reviews",
        ),
        (
            count(&|p| p.ownership == Ownership::Mine && p.review_state == ReviewState::Approved),
            "approved",
        ),
        (
            count(&|p| {
                p.ownership == Ownership::Mine && p.review_state == ReviewState::ChangesRequested
            }),
            "changes requested",
        ),
        (count(&|p| p.ci_status == CiStatus::Failing), "failing CI"),
        (
            count(&|p| p.ownership == Ownership::Mine && !p.is_draft),
            "yours",
        ),
        (
            count(&|p| p.ownership == Ownership::Mine && p.is_draft),
            "drafts",
        ),
    ];
    let mut out = vec![format!("{} open", prs.len())];
    out.extend(
        parts
            .iter()
            .filter(|(n, _)| *n > 0)
            .take(4)
            .map(|(n, label)| format!("{n} {label}")),
    );
    out.join(" · ")
}

/// Custom rule for matching other people's PRs into the unified inbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchRule {
    pub id: String,
    pub name: String,
    /// GitHub search query fragment, e.g. `review-requested:@me`, `assignee:@me`, `repo:owner/name`, `label:urgent`
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

/// Tabs of the inbox view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxFilter {
    /// The automatic, minimal view: only PRs that need something from you right now.
    Inbox,
    /// Every PR you opened (open and drafts).
    Mine,
    /// Other people's PRs (assigned, involved, or in your busiest repositories).
    Theirs,
    /// Everything, with no filtering.
    All,
}

impl InboxFilter {
    pub const TABS: [(InboxFilter, &'static str); 4] = [
        (InboxFilter::Inbox, "Inbox"),
        (InboxFilter::Mine, "Mine"),
        (InboxFilter::Theirs, "Theirs"),
        (InboxFilter::All, "All"),
    ];

    pub fn index(self) -> usize {
        Self::TABS.iter().position(|(f, _)| *f == self).unwrap_or(0)
    }

    pub fn from_index(idx: usize) -> Self {
        Self::TABS.get(idx).map_or(InboxFilter::Inbox, |(f, _)| *f)
    }

    /// Stable name for saving, so reordering the tabs never changes what a saved value means.
    pub fn name(self) -> &'static str {
        match self {
            InboxFilter::Inbox => "inbox",
            InboxFilter::Mine => "mine",
            InboxFilter::Theirs => "theirs",
            InboxFilter::All => "all",
        }
    }

    pub fn from_name(name: &str) -> Self {
        Self::TABS
            .iter()
            .map(|(f, _)| *f)
            .find(|f| f.name() == name)
            .unwrap_or(InboxFilter::Inbox)
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
            InboxFilter::Inbox => pr
                .attention(draft_stale_hours, open_stale_hours, now_hour)
                .requires_action(),
            InboxFilter::Mine => pr.ownership == Ownership::Mine,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> RepoConfig {
        RepoConfig {
            name: name.into(),
            manual_rank: 0,
            pinned: false,
            my_pr_volume: 0,
            assigned_review_volume: 0,
            collapsed: false,
            removed: false,
            activity: 0,
        }
    }

    fn my_pr(repo: &str, number: u64, idle_days: u64) -> PullRequest {
        PullRequest {
            id: format!("{repo}#{number}"),
            repo: repo.into(),
            number,
            title: "t".into(),
            url: String::new(),
            author: "me".into(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(),
            idle_hours: idle_days * 24,
            unreviewed_hours: idle_days * 24,
            head_ref: String::new(),
            head_repo: None,
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 0,
            deletions: 0,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
        }
    }

    #[test]
    fn repo_with_active_prs_outranks_repo_with_one_abandoned_pr() {
        let mut busy = repo("org/busy");
        busy.my_pr_volume = 4;
        let abandoned = {
            let mut r = repo("org/abandoned");
            r.my_pr_volume = 0; // volume only counts recent PRs
            r
        };
        let mut prs: Vec<_> = (1..=4).map(|n| my_pr("org/busy", n, 1)).collect();
        prs.push(my_pr("org/abandoned", 1, 150));
        let score = |r: &RepoConfig| r.smart_score(&prs, 72, 48, 0);
        assert!(score(&busy) > score(&abandoned) * 4);
    }

    #[test]
    fn summary_lists_the_useful_counts_first() {
        let mut approved = my_pr("org/a", 1, 1);
        approved.review_state = ReviewState::Approved;
        let mut incoming = my_pr("org/a", 2, 1);
        incoming.ownership = Ownership::Theirs;
        incoming.incoming_reason = IncomingReason::AssignedOrReviewRequested;
        let prs = vec![approved, incoming, my_pr("org/a", 3, 1)];
        assert_eq!(
            summarize_prs(&prs),
            "3 open · 1 need your review · 1 approved · 2 yours"
        );
        assert_eq!(summarize_prs(&[]), "0 open");
    }

    #[test]
    fn review_requested_by_name_outranks_a_team_request() {
        let mut named = my_pr("org/a", 1, 1);
        named.ownership = Ownership::Theirs;
        named.incoming_reason = IncomingReason::AssignedOrReviewRequested;
        let mut team = named.clone();
        team.incoming_reason = IncomingReason::TeamReviewRequested;
        let (named, team) = (named.attention(72, 48, 0), team.attention(72, 48, 0));
        assert_eq!(named, PrAttention::IncomingAssigned);
        assert_eq!(team, PrAttention::IncomingViaTeam);
        assert!(named.urgency_rank() < team.urgency_rank());
        assert!(
            team.requires_action(),
            "still shows in the Inbox tab, just lower"
        );
        assert_eq!(
            IncomingReason::from_db(team_reason().as_str()),
            team_reason()
        );
    }

    fn team_reason() -> IncomingReason {
        IncomingReason::TeamReviewRequested
    }

    #[test]
    fn inbox_ranks_people_waiting_on_you_before_your_own_prs() {
        let mut named = my_pr("org/a", 1, 1);
        named.ownership = Ownership::Theirs;
        named.incoming_reason = IncomingReason::AssignedOrReviewRequested;
        let mut team = named.clone();
        team.incoming_reason = IncomingReason::TeamReviewRequested;
        let mut changes = my_pr("org/a", 2, 1);
        changes.review_state = ReviewState::ChangesRequested;
        let mut approved = my_pr("org/a", 3, 1);
        approved.review_state = ReviewState::Approved;
        let score = |p: &PullRequest| p.importance(72, 48, 0);
        assert!(score(&named) > score(&changes));
        assert!(score(&changes) > score(&approved));
        assert!(score(&approved) > score(&team));
        let mut old_team = team.clone();
        old_team.idle_hours = 24 * 9;
        old_team.unreviewed_hours = 24 * 9;
        assert!(score(&old_team) > score(&team), "waiting longer climbs");
    }

    #[test]
    fn every_attention_has_an_action_group_and_the_groups_follow_priority() {
        use PrAttention::*;
        let order = [
            IncomingAssigned,
            MyOpenChangesRequested,
            MyOpenApproved,
            IncomingViaTeam,
            MyOpenNeedsNudge,
            MyDraftNeedsNudge,
        ];
        let groups: Vec<u8> = order.iter().map(|a| a.action_group().0).collect();
        assert!(groups.windows(2).all(|w| w[0] < w[1]), "{groups:?}");
        for a in [IncomingWatched, MyOpenWaiting, MyDraftActive] {
            assert!(!a.requires_action());
        }
    }

    #[test]
    fn only_prs_github_allows_are_ready_to_merge() {
        use MergeReadiness::*;
        assert_eq!(MergeReadiness::from_github("MERGEABLE", "CLEAN"), Ready);
        assert_eq!(MergeReadiness::from_github("MERGEABLE", "HAS_HOOKS"), Ready);
        assert_eq!(
            MergeReadiness::from_github("MERGEABLE", "BLOCKED"),
            NeedsReview
        );
        // Only optional checks are pending or failing: GitHub still lets you merge.
        assert_eq!(
            MergeReadiness::from_github("MERGEABLE", "UNSTABLE"),
            Unstable
        );
        assert!(Unstable.is_ready());
        assert_eq!(MergeReadiness::from_github("MERGEABLE", "BEHIND"), Behind);
        assert_eq!(
            MergeReadiness::from_github("CONFLICTING", "DIRTY"),
            Conflicts
        );
        assert_eq!(MergeReadiness::from_github("UNKNOWN", "UNKNOWN"), Unknown);
        assert!(!Unknown.is_ready() && !NeedsReview.is_ready());
        assert_eq!(MergeMethod::from_github("REBASE").flag(), "--rebase");
        assert_eq!(MergeMethod::from_github("whatever").flag(), "--squash");
    }

    #[test]
    fn engagement_is_capped_below_real_work() {
        let mut quiet = repo("org/quiet");
        quiet.activity = 10_000;
        let prs = vec![my_pr("org/waiting", 1, 3), my_pr("org/waiting", 2, 3)];
        let waiting = repo("org/waiting");
        assert!(quiet.smart_score(&[], 72, 48, 0) <= RepoConfig::ACTIVITY_CAP);
        assert!(waiting.smart_score(&prs, 72, 48, 0) > 0);
    }
}
