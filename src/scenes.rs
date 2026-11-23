//! Demo data for previews and snapshots.

use crate::*;
use model::{IncomingReason, ReviewState};

/// The pull requests previews, scenes and tests start from: a few repositories' worth, in every
/// state the Inbox shows.
pub(crate) fn demo_prs() -> Vec<PullRequest> {
    let mut prs = vec![
        // --- acme/engine (High priority repo: assigned PRs + stale draft + unreviewed open PR) ---
        PullRequest {
            id: "acme/engine#418".to_string(),
            repo: "acme/engine".to_string(),
            number: 418,
            title: "metal-backend: coalesce offscreen Rgba16Float layer binds during pinch"
                .to_string(),
            url: "https://github.com/acme/engine/pull/418".to_string(),
            author: "kai".to_string(),
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::AssignedOrReviewRequested,
            reviewers: vec!["mira".to_string()],
            idle_hours: 1,
            unreviewed_hours: 1,
            head_ref: "metal-layer-coalesce".to_string(),
            head_repo: Some("acme/engine".into()),
            matched_rule: Some("Assigned to you".to_string()),
            additions: 142,
            deletions: 38,
            ..Default::default()
        },
        PullRequest {
            id: "acme/engine#412".to_string(),
            repo: "acme/engine".to_string(),
            number: 412,
            title: "taffy: preserve subpixel baseline cache across color-only commits".to_string(),
            url: "https://github.com/acme/engine/pull/412".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            ci_status: CiStatus::Passing,
            reviewers: Vec::new(), // No reviewer assigned -> NUDGE to send to colleague!
            idle_hours: 54,
            unreviewed_hours: 54, // > 48h (2 days) without review -> NUDGE!
            head_ref: "feat/baseline-cache".to_string(),
            head_repo: Some("acme/engine".into()),
            stack_base: Some("metal-layer-coalesce".into()),
            additions: 89,
            deletions: 14,
            ..Default::default()
        },
        PullRequest {
            id: "acme/engine#397".to_string(),
            repo: "acme/engine".to_string(),
            number: 397,
            title: "WIP: experimental CoreAudio low-latency spatial voice ring".to_string(),
            url: "https://github.com/acme/engine/pull/397".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: true,
            ci_status: CiStatus::Pending,
            idle_hours: 120, // 5 days idle draft -> NUDGE!
            unreviewed_hours: 120,
            head_ref: "draft/coreaudio-ring".to_string(),
            head_repo: Some("acme/engine".into()),
            additions: 412,
            deletions: 19,
            ..Default::default()
        },
        PullRequest {
            id: "acme/engine#421".to_string(),
            repo: "acme/engine".to_string(),
            number: 421,
            title: "windfoil: tighten CJK autohint stem dilation at 1x scale".to_string(),
            url: "https://github.com/acme/engine/pull/421".to_string(),
            author: "sam".to_string(),
            review_state: ReviewState::Commented,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::SubscribedRule,
            reviewers: vec!["kai".to_string()],
            idle_hours: 5,
            unreviewed_hours: 5,
            head_ref: "windfoil-cjk-1x".to_string(),
            head_repo: Some("acme/engine".into()),
            stack_base: Some("feat/baseline-cache".into()),
            matched_rule: Some("Core Runtime Non-Draft PRs".to_string()),
            additions: 64,
            deletions: 29,
            ..Default::default()
        },
        // --- acme/cli (Second repo: assigned review + your open PR waiting > 3d on reviewer) ---
        PullRequest {
            id: "acme/cli#904".to_string(),
            repo: "acme/cli".to_string(),
            number: 904,
            title: "acp: stream tool call diffs over zero-copy IPC channel".to_string(),
            url: "https://github.com/acme/cli/pull/904".to_string(),
            author: "juno".to_string(),
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::AssignedOrReviewRequested,
            reviewers: vec!["mira".to_string()],
            idle_hours: 3,
            unreviewed_hours: 3,
            head_ref: "acp-zero-copy-diffs".to_string(),
            head_repo: Some("acme/cli".into()),
            matched_rule: Some("Assigned to you".to_string()),
            additions: 275,
            deletions: 84,
            external: true,
            ..Default::default()
        },
        PullRequest {
            id: "acme/cli#889".to_string(),
            repo: "acme/cli".to_string(),
            number: 889,
            title: "session: retry transient rate-limit 529 with jittered backoff".to_string(),
            url: "https://github.com/acme/cli/pull/889".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            reviewers: vec!["juno".to_string()],
            idle_hours: 74,
            unreviewed_hours: 74, // Waiting on @rekram for 3 days -> NUDGE to ping colleague!
            head_ref: "fix/retry-529-backoff".to_string(),
            head_repo: Some("acme/cli".into()),
            additions: 53,
            deletions: 12,
            ..Default::default()
        },
        PullRequest {
            id: "acme/cli#908".to_string(),
            repo: "acme/cli".to_string(),
            number: 908,
            title: "cli: add `--json-events` flag for headless subagent pipelines".to_string(),
            url: "https://github.com/acme/cli/pull/908".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            review_state: ReviewState::Approved,
            ci_status: CiStatus::Passing,
            reviewers: vec!["kai".to_string()],
            idle_hours: 2,
            head_ref: "feat/json-events".to_string(),
            head_repo: Some("acme/cli".into()),
            additions: 118,
            deletions: 7,
            ..Default::default()
        },
        // --- acme/inbox (Third repo: active draft + changes requested) ---
        PullRequest {
            id: "acme/inbox#14".to_string(),
            repo: "acme/inbox".to_string(),
            number: 14,
            title: "sqlite: persist per-repo smart importance weights & snooze deadlines"
                .to_string(),
            url: "https://github.com/acme/inbox/pull/14".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            review_state: ReviewState::ChangesRequested,
            ci_status: CiStatus::Failing,
            reviewers: vec!["alex".to_string()],
            idle_hours: 6,
            unreviewed_hours: 6,
            head_ref: "sqlite-importance-weights".to_string(),
            head_repo: Some("acme/inbox".into()),
            additions: 310,
            deletions: 44,
            ..Default::default()
        },
        PullRequest {
            id: "acme/inbox#19".to_string(),
            repo: "acme/inbox".to_string(),
            number: 19,
            title: "draft: webhook websocket bridge for sub-second PR assignment pushes"
                .to_string(),
            url: "https://github.com/acme/inbox/pull/19".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: true,
            ci_status: CiStatus::Pending,
            idle_hours: 8, // Fresh draft (< 72h), not stale
            unreviewed_hours: 8,
            head_ref: "draft/ws-webhook-bridge".to_string(),
            head_repo: Some("acme/inbox".into()),
            additions: 195,
            ..Default::default()
        },
    ];
    for pr in &mut prs {
        if pr.number == 908 {
            pr.approved_by = vec!["kai".to_owned()];
        }
    }
    // Opened a while before it last moved, as real ones are.
    for pr in &mut prs {
        pr.opened_hours = pr.idle_hours * 4 + 20;
    }
    let base = prs[0].clone();
    let review = |repo: &str,
                  number: u64,
                  title: &str,
                  author: &str,
                  external: bool,
                  team: bool,
                  size: (u32, u32),
                  idle: u64| {
        let mut pr = base.clone();
        pr.id = format!("{repo}#{number}");
        pr.repo = repo.into();
        pr.number = number;
        pr.title = title.into();
        pr.url = format!("https://github.com/{repo}/pull/{number}");
        pr.author = author.into();
        pr.external = external;
        pr.incoming_reason = if team {
            IncomingReason::TeamReviewRequested
        } else {
            IncomingReason::AssignedOrReviewRequested
        };
        pr.matched_rule = None;
        pr.reviewers = if team {
            Vec::new()
        } else {
            vec!["mira".into()]
        };
        pr.head_ref = format!("demo-{number}");
        pr.head_repo = Some(repo.into());
        pr.stack_base = None;
        (pr.additions, pr.deletions) = size;
        pr.idle_hours = idle;
        pr.opened_hours = idle * 4 + 20;
        pr.unreviewed_hours = idle;
        pr
    };
    prs.push(review(
        "acme/cli",
        910,
        "docs: fix typos in the contributing guide",
        "juno",
        true,
        false,
        (4, 4),
        7,
    ));
    prs.push(review(
        "acme/cli",
        912,
        "parser: migrate the command parser to the new tokenizer",
        "kai",
        false,
        false,
        (1240, 860),
        9,
    ));
    prs.push(review(
        "acme/engine",
        920,
        "webhooks: retry deliveries with exponential backoff",
        "sam",
        false,
        false,
        (96, 31),
        11,
    ));
    prs.push(review(
        "acme/engine",
        925,
        "metrics: export the queue depth gauge",
        "kai",
        false,
        true,
        (58, 6),
        13,
    ));
    prs.push(review(
        "acme/cli",
        931,
        "i18n: update the Polish translations",
        "juno",
        true,
        true,
        (310, 290),
        15,
    ));
    prs
}


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
    s.ai.feature = true;
    s.ai.enabled = true;
    s.sync_status = SyncStatus::Connected {
        username: "octocat".to_string(),
    };
    s.prs = scenes::demo_prs();
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
        s.ai.judgments.insert(
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
/// Runs another scene's setup first.
fn run_scene(s: &mut App, name: &str) {
    if let Some((_, setup)) = SCENES.iter().find(|(n, _)| *n == name) {
        setup(s);
    }
}

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
    }),
    ("settings-agent", |s| {
        seed_preview(s);
        s.agent.enabled = true;
        s.page = Page::Inbox;
        s.settings_open = true;
    }),
    ("settings-agent-model", |s| {
        run_scene(s, "settings-agent");
        s.agent.model = "claude-opus-5-5".into();
        s.agent_model_open = true;
    }),
    ("settings-ai", |s| {
        seed_preview(s);
        s.ai.key = TextEdit::new("sk-or-v1-0123456789abcdef");
        s.ai.enabled = false;
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
        s.ai.smart_sort = Some(false);
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
    // Approve and Merge mid-flight: one approving, one just landed, and Merge armed.
    ("buttons", |s| {
        seed_preview(s);
        s.page = Page::Inbox;
        s.filter = InboxFilter::Inbox;
        let ago = |secs: f32| std::time::Instant::now() - std::time::Duration::from_secs_f32(secs);
        s.approving = vec!["acme/engine#418".into()];
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
    // The same rows with Fix, once per agent.
    ("unblock-claude", |s| {
        run_scene(s, "unblock");
        s.agent.enabled = true;
        s.agent.provider = agent::Provider::Claude;
    }),
    ("unblock-codex", |s| {
        run_scene(s, "unblock");
        s.agent.enabled = true;
        s.agent.provider = agent::Provider::Codex;
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
