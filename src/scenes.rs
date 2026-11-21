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
