use std::process::Command;

use crate::model::{
    IncomingReason, NotificationEvent, NotificationKind, Ownership, PrAttention, PullRequest,
};

/// A short system sound, for moments worth a little celebration.
pub fn play_sound(name: &str, allow_os: bool) {
    if !allow_os {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let path = format!("/System/Library/Sounds/{name}.aiff");
        std::thread::spawn(move || {
            let _ = Command::new("afplay").arg(path).status();
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

/// Opens a PR URL directly in the user's default browser (`open <url>` on macOS).
pub fn open_url_in_browser(url: &str, allow_os_open: bool) {
    if !allow_os_open {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let u = url.to_owned();
        std::thread::spawn(move || {
            let _ = Command::new("open").arg(u).status();
        });
    }
}

/// Compares previous PR snapshot with new PR snapshot + thresholds to generate notification events:
/// 1. Instant Assignment: any incoming PR newly assigned / review-requested to you.
/// 2. My Open PR Nudge: your open PR has no reviewer or no review activity for `>= open_stale_hours`.
/// 3. My Draft PR Nudge: your draft PR has been untouched for `>= draft_stale_hours`.
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

        match att {
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
                {
                    let notif_id = format!("review-state-{}-{}", pr.id, pr.review_state.as_str());
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
