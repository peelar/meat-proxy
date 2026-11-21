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

mod effects;
mod judge_eval;
mod scenes;
mod state;
mod ui;

use effects::*;
use judge_eval::*;
use scenes::*;
use state::*;
use ui::*;

use db::Database;
use github::{SyncOutput, SyncStatus};
use model::{
    Blocker, CiStatus, InboxFilter, NotificationEvent, Ownership, PrAttention, PullRequest,
    RepoConfig, WatchRule,
};

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
