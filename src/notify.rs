use std::process::Command;

use crate::model::{
    AutoMerge, CiStatus, IncomingReason, MergeReadiness, NotificationEvent, NotificationKind,
    Ownership, PrAttention, PullRequest, ReviewState,
};

/// A second review round notifies only while the request is this fresh. An older one is in the
/// Inbox when the app opens, which is when it would have been noticed anyway.
const ASKED_AGAIN_FRESH_HOURS: u64 = 24;

/// A short system sound, for moments worth a little celebration.
pub fn play_sound(name: &str, allow_os: bool) {
    if !allow_os {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let path = format!("/System/Library/Sounds/{name}.aiff");
        std::thread::spawn(move || {
            let _ = Command::new("/usr/bin/afplay").arg(path).status();
        });
    }
}

pub fn dispatch_system_notification(ev: &NotificationEvent, allow_os_post: bool) {
    post(&ev.id, &ev.title, &ev.body, Some(&ev.pr_url), allow_os_post);
}

pub fn post(id: &str, title: &str, body: &str, url: Option<&str>, allow_os_post: bool) {
    if !allow_os_post || !rune_mac::notify::available() {
        return;
    }
    #[cfg(target_os = "macos")]
    mac::post(id, title, body, url);
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Mutex;

    use block2::RcBlock;
    use objc2::{
        rc::{Retained, autoreleasepool},
        runtime::Bool,
    };
    use objc2_foundation::{NSDictionary, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };

    struct PendingNotification {
        id: String,
        title: String,
        body: String,
        url: Option<String>,
    }

    // Notification Center presents one permission prompt at a time. Keep alerts that come in
    // during the prompt and submit them after the same authorization request completes.
    static PENDING: Mutex<Vec<PendingNotification>> = Mutex::new(Vec::new());

    pub(super) fn post(id: &str, title: &str, body: &str, url: Option<&str>) {
        {
            let mut pending = PENDING.lock().unwrap();
            pending.push(PendingNotification {
                id: id.to_owned(),
                title: title.to_owned(),
                body: body.to_owned(),
                url: url.map(str::to_owned),
            });
            if pending.len() > 1 {
                return;
            }
        }

        let completion = RcBlock::new(|granted: Bool, error: *mut NSError| {
            let pending = std::mem::take(&mut *PENDING.lock().unwrap());
            if !granted.as_bool() {
                if let Some(error) = unsafe { error.as_ref() } {
                    eprintln!(
                        "macOS notification authorization: {}",
                        error.localizedDescription()
                    );
                }
                return;
            }
            autoreleasepool(|_| {
                let center = UNUserNotificationCenter::currentNotificationCenter();
                let posted = RcBlock::new(|error: *mut NSError| {
                    if let Some(error) = unsafe { error.as_ref() } {
                        eprintln!(
                            "macOS notification delivery: {}",
                            error.localizedDescription()
                        );
                    }
                });
                for notification in pending {
                    let content = UNMutableNotificationContent::new();
                    content.setTitle(&NSString::from_str(&notification.title));
                    content.setBody(&NSString::from_str(&notification.body));
                    if let Some(url) = notification.url {
                        let key = NSString::from_str("url");
                        let value = NSString::from_str(&url);
                        let info = NSDictionary::from_slices(&[&*key], &[&*value]);
                        // Both keys and values are property-list strings. Objective-C erases
                        // the dictionary's generic parameters.
                        unsafe {
                            content.setUserInfo(&Retained::cast_unchecked::<NSDictionary>(info));
                        }
                    }
                    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                        &NSString::from_str(&notification.id),
                        &content,
                        None,
                    );
                    center.addNotificationRequest_withCompletionHandler(&request, Some(&posted));
                }
            });
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &completion,
            );
    }
}

/// Opens a web page in the user's default browser (`open <url>` on macOS). Nothing else opens.
pub fn open_url_in_browser(url: &str, allow_os_open: bool) {
    if !allow_os_open || !is_web_page(url) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let u = url.to_owned();
        std::thread::spawn(move || {
            let _ = Command::new("/usr/bin/open").arg(u).status();
        });
    }
}

/// Links in a description and check links come from other people. `file:`, `smb:` or an app's own
/// scheme would start a program or mount a share, so only https pages open.
fn is_web_page(url: &str) -> bool {
    url.len() > 8
        && url
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Compares previous PR snapshot with new PR snapshot + thresholds to generate notification events:
/// 1. Instant Assignment: any incoming PR newly assigned / review-requested to you, and each later
///    round in which you are asked again after reviewing.
/// 2. My Open PR Nudge: your open PR has no reviewer or no review activity for `>= open_stale_hours`.
/// 3. My Draft PR Nudge: your draft PR has been untouched for `>= draft_stale_hours`.
/// 4. Your open PR moved away from or toward its merge: checks failed, conflicts appeared, comments
///    wait for an answer, or it became mergeable.
///
/// What can happen to a pull request more than once carries the hour in its id, so the next time
/// notifies too.
pub fn evaluate_inbox_notifications(
    previous_prs: &[PullRequest],
    current_prs: &[PullRequest],
    existing_notifications: &[NotificationEvent],
    draft_stale_hours: u64,
    open_stale_hours: u64,
    now_hour: u64,
) -> Vec<NotificationEvent> {
    let mut events = Vec::new();

    for pr in current_prs {
        let att = pr.attention(draft_stale_hours, open_stale_hours, now_hour);
        let was_present = previous_prs.iter().find(|p| p.id == pr.id);

        // Each new approval of yours, named. GitHub's review decision stays empty in repositories
        // that don't require reviews, and a second approval doesn't change it.
        let mut approved_now = false;
        if pr.ownership == Ownership::Mine
            && let Some(prev) = was_present
        {
            for login in pr
                .approved_by
                .iter()
                .filter(|l| !prev.approved_by.contains(l))
            {
                approved_now = true;
                let notif_id = format!("approved-{}-{login}-{now_hour}", pr.id);
                if !existing_notifications.iter().any(|n| n.id == notif_id) {
                    events.push(NotificationEvent {
                        id: notif_id,
                        title: format!("@{login} approved {}#{}", pr.repo, pr.number),
                        body: pr.title.clone(),
                        pr_url: pr.url.clone(),
                        kind: NotificationKind::ReviewActivity,
                        timestamp_label: "just now".to_string(),
                        unread: true,
                    });
                }
            }
        }

        if pr.ownership == Ownership::Mine
            && !pr.is_draft
            && let Some(prev) = was_present
        {
            let name = format!("{}#{}", pr.repo, pr.number);
            let mut moved = |what: &str, title: String, body: String| {
                events.push(NotificationEvent {
                    id: format!("{what}-{}-{now_hour}", pr.id),
                    title,
                    body,
                    pr_url: pr.url.clone(),
                    kind: NotificationKind::ReviewActivity,
                    timestamp_label: "just now".to_string(),
                    unread: true,
                });
            };
            if pr.ci_status == CiStatus::Failing && prev.ci_status != CiStatus::Failing {
                let checks: Vec<&str> = pr.failing_checks.iter().map(|c| c.name.as_str()).collect();
                let body = if checks.is_empty() {
                    pr.title.clone()
                } else {
                    format!("{} · {}", checks.join(", "), pr.title)
                };
                moved("ci-failed", format!("Checks failed on {name}"), body);
            }
            // Saved data does not say whether there were conflicts, so the first sync of a run
            // cannot tell that they are new.
            if pr.merge_readiness == MergeReadiness::Conflicts
                && !matches!(
                    prev.merge_readiness,
                    MergeReadiness::Conflicts | MergeReadiness::Unknown
                )
            {
                moved(
                    "conflicts",
                    format!("{name} has conflicts"),
                    pr.title.clone(),
                );
            }
            if let (Some(before), Some(now)) = (prev.unanswered_threads, pr.unanswered_threads)
                && now > before
            {
                let title = if now == 1 {
                    format!("A comment waits for your answer on {name}")
                } else {
                    format!("{now} comments wait for your answer on {name}")
                };
                moved("comments", title, pr.title.clone());
            }
            let can_merge = |p: &PullRequest| {
                p.merge_readiness.is_ready()
                    && p.attention(draft_stale_hours, open_stale_hours, now_hour)
                        == PrAttention::MyOpenApproved
            };
            // An approval that makes it mergeable is announced as the approval, above.
            if can_merge(pr)
                && pr.auto_merge != AutoMerge::On
                && prev.merge_readiness != MergeReadiness::Unknown
                && !can_merge(prev)
                && !approved_now
            {
                moved("can-merge", format!("{name} can merge"), pr.title.clone());
            }
        }

        match att {
            // Asked again after reviewing: the first request's notification was sent long ago.
            PrAttention::IncomingAssigned
                if pr
                    .asked
                    .as_ref()
                    .is_some_and(|ask| ask.again && ask.hours_ago <= ASKED_AGAIN_FRESH_HOURS) =>
            {
                let at = pr.asked.as_ref().map_or("", |ask| ask.at.as_str());
                let notif_id = format!("asked-again-{}-{at}", pr.id);
                if !existing_notifications.iter().any(|n| n.id == notif_id) {
                    events.push(NotificationEvent {
                        id: notif_id,
                        title: format!("{}#{} is back for your thumb", pr.repo, pr.number),
                        body: format!("@{}: {}", pr.author, pr.title),
                        pr_url: pr.url.clone(),
                        kind: NotificationKind::InstantAssigned,
                        timestamp_label: pr.activity_label(),
                        unread: true,
                    });
                }
            }
            PrAttention::IncomingAssigned => {
                let is_newly_assigned = match was_present {
                    None => true,
                    Some(prev) => prev.incoming_reason != IncomingReason::AssignedOrReviewRequested,
                };
                let notif_id = format!("assign-{}", pr.id);
                let already_sent = existing_notifications.iter().any(|n| n.id == notif_id);
                if (is_newly_assigned || previous_prs.is_empty()) && !already_sent {
                    events.push(NotificationEvent {
                        id: notif_id,
                        title: format!("Assigned to review {}#{}", pr.repo, pr.number),
                        body: format!("@{}: {}", pr.author, pr.title),
                        pr_url: pr.url.clone(),
                        kind: NotificationKind::InstantAssigned,
                        timestamp_label: pr.activity_label(),
                        unread: true,
                    });
                }
            }
            PrAttention::MyOpenNeedsNudge => {
                let notif_id = format!("open-nudge-{}", pr.id);
                let already_sent = existing_notifications.iter().any(|n| n.id == notif_id);
                if !already_sent {
                    let reason = if pr.reviewers.is_empty() {
                        "No reviewer assigned yet — send to a colleague!".to_string()
                    } else {
                        let d = (pr.unreviewed_hours / 24).max(1);
                        format!(
                            "No review from @{} in {d}d — nudge your reviewer!",
                            pr.reviewers.join(", @")
                        )
                    };
                    events.push(NotificationEvent {
                        id: notif_id,
                        title: format!("Open PR Nudge · {}#{}", pr.repo, pr.number),
                        body: format!("{} ({})", pr.title, reason),
                        pr_url: pr.url.clone(),
                        kind: NotificationKind::OpenPrStaleNudge,
                        timestamp_label: pr.activity_label(),
                        unread: true,
                    });
                }
            }
            PrAttention::MyDraftNeedsNudge => {
                let notif_id = format!("draft-nudge-{}", pr.id);
                let already_sent = existing_notifications.iter().any(|n| n.id == notif_id);
                if !already_sent {
                    let d = (pr.idle_hours / 24).max(1);
                    events.push(NotificationEvent {
                        id: notif_id,
                        title: format!("Forgotten Draft ({d}d idle) · {}#{}", pr.repo, pr.number),
                        body: format!("Resume or close: {}", pr.title),
                        pr_url: pr.url.clone(),
                        kind: NotificationKind::DraftStaleNudge,
                        timestamp_label: pr.activity_label(),
                        unread: true,
                    });
                }
            }
            _ => {
                if pr.ownership == Ownership::Mine
                    && let Some(prev) = was_present
                    && prev.review_state != pr.review_state
                    && !(approved_now && pr.review_state == ReviewState::Approved)
                {
                    let notif_id = format!(
                        "review-state-{}-{}-{now_hour}",
                        pr.id,
                        pr.review_state.as_str()
                    );
                    if !existing_notifications.iter().any(|n| n.id == notif_id) {
                        events.push(NotificationEvent {
                            id: notif_id,
                            title: format!(
                                "PR {} · {}#{}",
                                pr.review_state.as_str(),
                                pr.repo,
                                pr.number
                            ),
                            body: pr.title.clone(),
                            pr_url: pr.url.clone(),
                            kind: NotificationKind::ReviewActivity,
                            timestamp_label: "just now".to_string(),
                            unread: true,
                        });
                    }
                }
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_pages_open() {
        for url in [
            "https://github.com/o/r/pull/1",
            "HTTPS://ci.example.com/run/5",
        ] {
            assert!(is_web_page(url), "{url}");
        }
        for url in [
            "file:///System/Applications/Utilities/Terminal.app",
            "smb://attacker.example/share",
            "vscode://file/etc/passwd",
            "http://example.com",
            "-a Terminal",
            "https://",
            "https://example.com/a b",
            "/Applications/Calculator.app",
        ] {
            assert!(!is_web_page(url), "{url}");
        }
    }

    fn titles(before: &[PullRequest], after: &[PullRequest]) -> Vec<String> {
        evaluate_inbox_notifications(before, after, &[], 72, 48, 0)
            .into_iter()
            .map(|e| e.title)
            .collect()
    }

    #[test]
    fn each_new_approval_and_review_request_notifies() {
        // Your PR in a repository that doesn't require reviews: GitHub never sets a decision.
        let mut mine = crate::scenes::demo_prs()
            .into_iter()
            .find(|p| p.ownership == Ownership::Mine && !p.is_draft)
            .unwrap();
        mine.review_state = ReviewState::None;
        mine.approved_by = Vec::new();
        let mut kai = mine.clone();
        kai.approved_by = vec!["kai".into()];
        let mut both = kai.clone();
        both.approved_by = vec!["kai".into(), "mira".into()];
        let name = format!("{}#{}", mine.repo, mine.number);
        assert!(titles(&[mine.clone()], &[kai.clone()]).contains(&format!("@kai approved {name}")));
        let second = titles(&[kai.clone()], &[both.clone()]);
        assert!(second.contains(&format!("@mira approved {name}")));
        assert!(!second.iter().any(|t| t.contains("@kai")));

        // Where reviews are required, the approval is named once, not also as "PR Approved".
        let mut decided = kai.clone();
        decided.review_state = ReviewState::Approved;
        let mut waiting = mine.clone();
        waiting.review_state = ReviewState::ReviewRequested;
        assert_eq!(
            titles(&[waiting], &[decided]),
            vec![format!("@kai approved {name}")]
        );

        // Someone asks for your review.
        let mut theirs = crate::scenes::demo_prs()
            .into_iter()
            .find(|p| p.ownership == Ownership::Theirs)
            .unwrap();
        theirs.incoming_reason = IncomingReason::SubscribedRule;
        let mut asked = theirs.clone();
        asked.incoming_reason = IncomingReason::AssignedOrReviewRequested;
        assert!(
            titles(&[theirs], &[asked.clone()])
                .iter()
                .any(|t| t.starts_with("Assigned to review"))
        );

        // Asked again after you reviewed: a new notification per round, while the request is fresh.
        let round = |at: &str, hours_ago| {
            let mut pr = asked.clone();
            pr.asked = Some(crate::model::Ask {
                at: at.into(),
                hours_ago,
                again: true,
            });
            pr
        };
        let first = evaluate_inbox_notifications(
            &[asked.clone()],
            &[round("2026-11-20T10:00:00Z", 1)],
            &[],
            72,
            48,
            0,
        );
        assert_eq!(first.len(), 1);
        assert!(first[0].title.ends_with("is back for your thumb"));
        let same = evaluate_inbox_notifications(
            &[asked.clone()],
            &[round("2026-11-20T10:00:00Z", 2)],
            &first,
            72,
            48,
            0,
        );
        assert!(same.is_empty(), "the same request does not notify twice");
        let next = evaluate_inbox_notifications(
            &[asked.clone()],
            &[round("2026-11-21T09:00:00Z", 0)],
            &first,
            72,
            48,
            0,
        );
        assert_eq!(next.len(), 1, "the next round does");
        assert!(titles(&[asked.clone()], &[round("2026-11-17T10:00:00Z", 96)]).is_empty());
    }

    #[test]
    fn your_pull_request_moving_away_from_or_toward_its_merge_notifies() {
        let mut mine = crate::scenes::demo_prs()
            .into_iter()
            .find(|p| p.ownership == Ownership::Mine && !p.is_draft)
            .unwrap();
        mine.review_state = ReviewState::Approved;
        mine.approved_by = vec!["kai".into()];
        mine.ci_status = CiStatus::Pending;
        mine.merge_readiness = MergeReadiness::NeedsReview;
        mine.unanswered_threads = Some(0);
        let name = format!("{}#{}", mine.repo, mine.number);

        let mut failed = mine.clone();
        failed.ci_status = CiStatus::Failing;
        failed.failing_checks = vec![crate::model::Check {
            name: "lint".into(),
            url: String::new(),
        }];
        let events =
            evaluate_inbox_notifications(&[mine.clone()], &[failed.clone()], &[], 72, 48, 7);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, format!("Checks failed on {name}"));
        assert!(events[0].body.starts_with("lint · "));
        // Still failing an hour later is not news; failing again after a green run is.
        assert!(
            evaluate_inbox_notifications(&[failed.clone()], &[failed.clone()], &events, 72, 48, 8)
                .is_empty()
        );
        let again =
            evaluate_inbox_notifications(&[mine.clone()], &[failed.clone()], &events, 72, 48, 9);
        assert_eq!(again.len(), 1);
        assert_ne!(again[0].id, events[0].id);

        let mut ready = mine.clone();
        ready.ci_status = CiStatus::Passing;
        ready.merge_readiness = MergeReadiness::Ready;
        assert_eq!(
            titles(&[mine.clone()], &[ready.clone()]),
            vec![format!("{name} can merge")]
        );
        // With auto-merge on GitHub merges it; and what was saved by an earlier run says nothing
        // about merge state, so the first sync of a run announces neither readiness nor conflicts.
        let mut auto = ready.clone();
        auto.auto_merge = AutoMerge::On;
        assert!(titles(&[mine.clone()], &[auto]).is_empty());
        let mut saved = mine.clone();
        saved.merge_readiness = MergeReadiness::Unknown;
        assert!(titles(&[saved.clone()], &[ready]).is_empty());
        let mut conflicted = mine.clone();
        conflicted.merge_readiness = MergeReadiness::Conflicts;
        assert!(titles(&[saved], &[conflicted.clone()]).is_empty());
        assert_eq!(
            titles(&[mine.clone()], &[conflicted]),
            vec![format!("{name} has conflicts")]
        );

        let mut commented = mine.clone();
        commented.unanswered_threads = Some(2);
        assert_eq!(
            titles(&[mine], &[commented]),
            vec![format!("2 comments wait for your answer on {name}")]
        );
    }
}
