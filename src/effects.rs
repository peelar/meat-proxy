//! What happens between frames: workers landing, timers, notifications and animations.

use crate::*;

pub(crate) fn setup(s: &mut App, waker: rune::Waker) {
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
    s.ai.worker.set_waker(waker.clone());
    if s.ai.feature && !s.preview && s.allow_os_side_effects {
        let raw = judge::JudgeConfig::load_raw();
        s.ai.config = raw.clone().filter(judge::JudgeConfig::usable);
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
    s.agent_worker.set_waker(waker.clone());
    s.merge_worker.set_waker(waker);
    s.boot_worker.request((), |()| {
        Database::open_default().and_then(|db| db.load_snapshot())
    });
}

/// Records the events that are new and, unless `quiet`, shows each as a notification. Returns the
/// pull requests the new ones are about.
pub(crate) fn deliver_events(s: &mut App, events: Vec<NotificationEvent>, quiet: bool) -> Vec<String> {
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

pub(crate) fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    if s.preview {
        return false;
    }
    let mut changed = false;
    changed |= apply_typed(s, tx);
    changed |= on_boot(s);
    changed |= on_setup(s, tx);
    changed |= on_sync(s, clock);
    changed |= tick_intro(s, tx);
    changed |= on_judgments(s);
    on_notifications(s, clock);
    run_timers(s, clock);
    changed |= on_autonomy(s);
    changed |= on_actions(s);
    changed |= tick_buttons(s, tx);
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

/// Applies what was typed in Settings once the typing has paused.
fn apply_typed(s: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
    let typed = [&s.max_age_input, &s.ai.url, &s.ai.model, &s.ai.key, &s.agent_model]
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
        s.apply_agent_model();
    }
    changed
}

/// The saved data, read at launch.
fn on_boot(s: &mut App) -> bool {
    let mut changed = false;
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
        // Setup can't be finished over unreadable data, so there is nothing to fetch for.
        if !s.boot_failed {
            s.trigger_sync();
            s.trigger_discovery();
        }
        changed = true;
    }
    changed
}

/// Setup's workers: repositories found, one added by name, and signing in.
fn on_setup(s: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
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
    changed
}

/// A sync landed: the new lists, what they notify, and what is saved.
fn on_sync(s: &mut App, clock: f64) -> bool {
    let mut changed = false;
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
                        let _ = db.prune();
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
    changed
}

/// The intro and rows sliding in, a frame at a time.
fn tick_intro(s: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
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
    changed
}

/// The model's verdicts landed.
fn on_judgments(s: &mut App) -> bool {
    let mut changed = false;
    if let Some(batch) = s.ai.worker.take() {
        s.ai.spent += batch.usage.cost_usd;
        s.ai.off |= batch.fatal;
        for error in batch.errors.iter().take(3) {
            eprintln!("meat-proxy judge: {error}");
        }
        // Verdicts that land after AI sorting was switched off are not used.
        if s.ai.enabled {
            s.store_judgments(batch.judged);
        }
        changed = true;
    }
    changed
}

/// GitHub's notifications feed answered.
fn on_notifications(s: &mut App, clock: f64) {
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
}

/// Starts the notifications poll and the full sync when they are due.
fn run_timers(s: &mut App, clock: f64) {
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
}

/// What the Autonomous switches did on GitHub.
fn on_autonomy(s: &mut App) -> bool {
    let mut changed = false;
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
    changed
}

/// The row buttons' jobs landed: auto-merge, resolve, merge, unblock and approve.
fn on_actions(s: &mut App) -> bool {
    let mut changed = false;
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
                    }
                ));
            }
        }
        changed = true;
    }
    if let Some((id, result)) = s.agent_worker.take() {
        s.agent_running = None;
        let name = s.prs.iter().find(|p| p.id == id).cloned();
        let sound_on = s.native_notifications && s.allow_os_side_effects;
        match (result, name) {
            (Ok(outcome), Some(pr)) => {
                notify::post(&format!("agent-{id}"), "meat proxy", &outcome.announce(&pr), Some(&pr.url), sound_on);
                match outcome {
                    agent::Outcome::Pushed => s.trigger_sync(),
                    agent::Outcome::NothingPushed { said, folder } => {
                        s.status_banner = Some(format!(
                            "The agent pushed nothing to {}#{}. {said} (clone kept in {})",
                            pr.repo,
                            pr.number,
                            folder.display()
                        ));
                    }
                }
            }
            (Err(error), pr) => {
                let at = pr.map_or(String::new(), |p| format!(" on {}#{}", p.repo, p.number));
                notify::post(&format!("agent-{id}"), "meat proxy", &format!("The agent failed{at}"), None, sound_on);
                s.status_banner = Some(format!("Fix failed{at}. {error}"));
            }
            (Ok(_), None) => {}
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
    changed
}

/// Armed buttons, washes, pace flashes and celebrations, a frame at a time.
fn tick_buttons(s: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
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
    changed
}
