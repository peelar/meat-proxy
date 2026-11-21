//! Demo data for previews and snapshots.

use crate::*;

/// Eighteen weeks of made-up approvals and merges for the preview scenes: busy on weekdays,
/// quiet at weekends, and a streak running into today.
pub(crate) fn demo_pace_events() -> Vec<pace::Event> {
    let now = chrono::Utc::now().timestamp();
    let mut out = Vec::new();
    for back in (0..pace::WEEKS as i64 * 7).rev() {
        let h = (back as u64).wrapping_mul(2_654_435_761) % 97;
        let date = chrono::Local::now().date_naive() - chrono::Days::new(back as u64);
        let weekend = chrono::Datelike::weekday(&date).number_from_monday() >= 6;
        let count = match back {
            0..=3 => 2 + h % 2,
            4 => 0,
            _ if weekend => u64::from(h.is_multiple_of(9)),
            _ => h % 6,
        };
        for i in 0..count {
            // The last week is the one the panel counts: mostly quick, one bold.
            let (merge, quick) = if back < 7 {
                (i == 1, i == 1 || (back as u64 + i) % 3 != 2)
            } else {
                ((h + i).is_multiple_of(3), !(h + i).is_multiple_of(4))
            };
            let wait = if quick { 4 + (h * 7 + i * 13) as i64 % 50 } else { 90 + h as i64 * 11 };
            out.push(pace::Event {
                at: now - back * 86_400 - i as i64 * 1800,
                kind: if merge { pace::Kind::Merge } else { pace::Kind::Approve },
                pr_id: format!("acme/demo#{back}{i}"),
                wait_minutes: Some(wait),
                lines: 40 + (h * 3) as u32,
                verdict: if !merge && back == 2 && i == 0 {
                    pace::Verdict::Bold
                } else if quick {
                    pace::Verdict::Quick
                } else {
                    pace::Verdict::Steady
                },
            });
        }
    }
    out
}

pub(crate) fn seed_preview(s: &mut App) {
    s.loading = false;
    s.pace_events = demo_pace_events();
    s.ai_feature = true;
    s.ai_enabled = true;
    s.sync_status = SyncStatus::Connected {
        username: "octocat".to_string(),
    };
    s.prs = github::demo_prs();
    if let Some(pr) = s.prs.iter_mut().find(|p| p.id == "acme/engine#418") {
        pr.description = "## What\n\nBinding the offscreen `Rgba16Float` layer once per pinch frame instead of once per draw. **Cuts about 40%** of encoder time on a 3-finger pinch.\n\n<!-- template: link the issue -->\n\n- coalesce binds in `metal/layer.rs`\n- keep the bind cache across passes\n- add a pinch trace to the bench\n\nFixes [#402](https://github.com/acme/engine/issues/402)".into();
    }
    s.repos = github::infer_repos_from_prs(&s.prs, &[]);
    s.last_synced = Some(std::time::Instant::now());
    // What a model would have said about some of the demo review requests, and a rule about one.
    let model = "gpt-4o-mini";
    for (id, reading, urgent, by) in [
        ("acme/engine#418", 0.5, 0.83, model),
        ("acme/cli#904", 0.9, 0.04, model),
        ("acme/cli#910", 0.03, 0.03, "rules"),
        ("acme/engine#920", 0.45, 0.05, model),
        ("acme/cli#931", 0.05, 0.02, model),
    ] {
        s.judgments.insert(
            id.to_owned(),
            judge::Judgment {
                answers: [("reading", reading), ("urgent", urgent)]
                    .into_iter()
                    .map(|(q, p)| (q.to_owned(), p))
                    .collect(),
                by: by.into(),
                fingerprint: String::new(),
            },
        );
    }
}

pub(crate) type Scene = (&'static str, fn(&mut App));

/// The states `--scene <name>` opens, for snapshots and demos; each starts from `init`.
pub(crate) static SCENES: &[Scene] = &[
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
