//! What meat proxy does on GitHub by itself, once the user turns a scope on in Settings.
//!
//! Every action is one that GitHub's own buttons do without asking twice, on the user's own pull
//! requests. Each happens at most once per commit: a new push is a new chance, and an action the
//! user undid on GitHub is not done again for the same commit.

use std::collections::HashMap;

use crate::model::{AutoMerge, CiStatus, MergeReadiness, Ownership, PullRequest, ReviewState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Update branch, while the repository requires branches to be up to date before merging.
    UpdateBranch,
    /// Enable auto-merge on an approved pull request that only waits on its checks.
    EnableAutoMerge,
    /// Re-request review from these people, who asked for changes before the last push.
    RerequestReview(Vec<String>),
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Action::UpdateBranch => "update-branch",
            Action::EnableAutoMerge => "auto-merge",
            Action::RerequestReview(_) => "rerequest",
        }
    }

    /// The notification that says it happened.
    pub fn announce(&self, pr: &PullRequest) -> String {
        let name = format!("{}#{}", pr.repo, pr.number);
        match self {
            Action::UpdateBranch => format!("Updated the branch of {name}"),
            Action::EnableAutoMerge => format!("Enabled auto-merge on {name}"),
            Action::RerequestReview(who) => {
                format!("Re-requested review from @{} on {name}", who.join(", @"))
            }
        }
    }
}

/// What one action came to: the pull request as it was when the action ran, the action, and
/// GitHub's answer.
pub type Outcome = (PullRequest, Action, Result<(), String>);

/// How long an action is remembered. A pull request that leaves the lists for a while (a lower
/// age limit, a search that failed once) must not have an action the user undid done again.
pub const DONE_KEPT_SECS: i64 = 30 * 86_400;

/// Names one action on one commit of one pull request.
pub fn key(pr: &PullRequest, action: &Action) -> String {
    format!("{}:{}:{}", pr.id, action.name(), pr.head_oid)
}

/// What to do now, given which switches are on (own branches; reviewers) and what was done (each
/// key with when).
pub fn plan(
    prs: &[PullRequest],
    branches: bool,
    reviewers: bool,
    done: &HashMap<String, i64>,
) -> Vec<(String, Action)> {
    let mut out = Vec::new();
    // Saved data has no commit; nothing happens until a sync says which commit it is.
    for pr in prs
        .iter()
        .filter(|pr| pr.ownership == Ownership::Mine && !pr.is_draft && !pr.head_oid.is_empty())
    {
        let mut actions = Vec::new();
        if branches {
            // GitHub reports a branch as behind only when the rest of the merge requirements
            // are met, so this does not re-run checks on a branch that waits for review.
            // Not while checks run: each update starts them over, and on a busy base that would
            // be every sync.
            if pr.merge_readiness == MergeReadiness::Behind
                && pr.can_update_branch
                && matches!(pr.ci_status, CiStatus::Passing | CiStatus::None)
            {
                actions.push(Action::UpdateBranch);
            }
            if pr.auto_merge == AutoMerge::Available
                && pr.review_state == ReviewState::Approved
                && pr.ci_status == CiStatus::Pending
                && pr.merge_readiness == MergeReadiness::NeedsReview
            {
                actions.push(Action::EnableAutoMerge);
            }
        }
        // Comments still waiting for an answer mean the push is not the whole reply yet; threads
        // not known (too many to fetch, or no sync yet) count as waiting.
        if reviewers
            && pr.review_state != ReviewState::Approved
            && pr.unanswered_threads == Some(0)
        {
            let due: Vec<String> = pr
                .due_for_another_look()
                .into_iter()
                .filter(|login| !crate::sections::is_bot(login))
                .map(str::to_owned)
                .collect();
            if !due.is_empty() {
                actions.push(Action::RerequestReview(due));
            }
        }
        for action in actions {
            if !done.contains_key(&key(pr, &action)) {
                out.push((pr.id.clone(), action));
            }
        }
    }
    out
}

/// Does it on GitHub, through the GitHub CLI.
pub fn perform(pr: &PullRequest, action: &Action) -> Result<(), String> {
    match action {
        Action::UpdateBranch => crate::github::update_branch(&pr.repo, pr.number, &pr.head_oid),
        Action::EnableAutoMerge => crate::github::set_auto_merge(&pr.url, pr.merge_method, true, &pr.head_oid),
        Action::RerequestReview(who) => crate::github::request_reviewers(&pr.repo, pr.number, who),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mine() -> PullRequest {
        let mut pr = crate::github::demo_prs()
            .into_iter()
            .find(|p| p.ownership == Ownership::Mine && !p.is_draft)
            .unwrap();
        pr.head_oid = "abc".into();
        pr.last_commit_at = "2026-11-20T12:00:00Z".into();
        pr.review_state = ReviewState::ReviewRequested;
        pr.ci_status = CiStatus::Passing;
        pr.merge_readiness = MergeReadiness::NeedsReview;
        pr.auto_merge = AutoMerge::Unavailable;
        pr.unanswered_threads = Some(0);
        pr
    }

    fn names(planned: &[(String, Action)]) -> Vec<&'static str> {
        planned.iter().map(|(_, a)| a.name()).collect()
    }

    #[test]
    fn a_branch_the_repository_wants_up_to_date_is_updated_once_per_commit() {
        let none = HashMap::new();
        let mut pr = mine();
        pr.merge_readiness = MergeReadiness::Behind;
        pr.can_update_branch = true;
        assert_eq!(names(&plan(&[pr.clone()], true, false, &none)), ["update-branch"]);
        // Off, or the reviewers scope alone, does nothing to branches.
        assert!(plan(&[pr.clone()], false, true, &none).is_empty());

        // Done for this commit: not again. A new commit is a new chance.
        let done: HashMap<String, i64> = [(key(&pr, &Action::UpdateBranch), 0)].into();
        assert!(plan(&[pr.clone()], true, false, &done).is_empty());
        pr.head_oid = "def".into();
        assert_eq!(plan(&[pr.clone()], true, false, &done).len(), 1);

        // Red or running checks, a draft, someone else's, or saved data with no commit: left alone.
        for change in [
            |p: &mut PullRequest| p.ci_status = CiStatus::Failing,
            |p: &mut PullRequest| p.ci_status = CiStatus::Pending,
            |p: &mut PullRequest| p.is_draft = true,
            |p: &mut PullRequest| p.ownership = Ownership::Theirs,
            |p: &mut PullRequest| p.head_oid.clear(),
        ] {
            let mut other = pr.clone();
            change(&mut other);
            assert!(plan(&[other], true, true, &none).is_empty());
        }
    }

    #[test]
    fn auto_merge_is_enabled_on_an_approved_pull_request_waiting_on_checks() {
        let none = HashMap::new();
        let mut pr = mine();
        pr.review_state = ReviewState::Approved;
        pr.ci_status = CiStatus::Pending;
        pr.auto_merge = AutoMerge::Available;
        assert_eq!(names(&plan(&[pr.clone()], true, false, &none)), ["auto-merge"]);
        // Already on, not offered by the repository, or held by conflicts: nothing.
        for change in [
            |p: &mut PullRequest| p.auto_merge = AutoMerge::On,
            |p: &mut PullRequest| p.auto_merge = AutoMerge::Unavailable,
            |p: &mut PullRequest| p.merge_readiness = MergeReadiness::Conflicts,
            |p: &mut PullRequest| p.review_state = ReviewState::ReviewRequested,
        ] {
            let mut other = pr.clone();
            change(&mut other);
            assert!(plan(&[other], true, false, &none).is_empty());
        }
    }

    #[test]
    fn review_is_re_requested_from_who_asked_for_changes_before_the_last_push() {
        let none = HashMap::new();
        let mut pr = mine();
        pr.review_state = ReviewState::ChangesRequested;
        pr.changes_requested = vec![
            ("kai".into(), "2026-11-19T09:00:00Z".into()),
            ("renovate[bot]".into(), "2026-11-19T09:00:00Z".into()),
        ];
        let planned = plan(&[pr.clone()], false, true, &none);
        assert_eq!(planned[0].1, Action::RerequestReview(vec!["kai".into()]));
        // Already asked again, or comments still to answer: wait.
        let mut asked = pr.clone();
        asked.requested_reviewers = vec!["kai".into()];
        assert!(plan(&[asked], false, true, &none).is_empty());
        let mut open = pr.clone();
        open.unanswered_threads = Some(1);
        assert!(plan(&[open], false, true, &none).is_empty());
        let mut unknown = pr.clone();
        unknown.unanswered_threads = None;
        assert!(plan(&[unknown], false, true, &none).is_empty(), "threads not known: wait");
        assert!(plan(&[pr], true, false, &none).is_empty(), "the branches scope pings nobody");
    }
}
