//! Pace: meat proxy keeps score of how quickly the user clears the way for code, and shows it.
//!
//! Only what happens from here counts: an approval or a merge pressed in meat proxy. Speed is
//! measured from the moment the ball landed in the user's court: being asked to review for an
//! approval, and the pull request being approved (or pushed to after that) for a merge.
//!
//! Speed is not rewarded on its own. A big pull request approved faster than anyone could read it
//! earns no medal, only a raised eyebrow; merging an approved pull request quickly always does.

use chrono::{DateTime, Datelike, Days, FixedOffset, NaiveDate};

use crate::model::PullRequest;

/// Within this long, an approval or a merge is quick.
pub const QUICK_MINUTES: i64 = 60;
/// Changed lines past which an approval is never quick: reading it takes longer than that.
pub const BIG_LINES: u32 = 400;
/// A big pull request approved at more lines a minute than this was not read. Bold.
const SKIM_LINES_PER_MINUTE: i64 = 100;
/// Weeks of history the activity grid shows.
pub const WEEKS: usize = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Approve,
    Merge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Within the hour, and not a skim of a big one: it counts.
    Quick,
    /// Counted, no medal.
    Steady,
    /// A big pull request approved faster than it could be read.
    Bold,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Approve => "approve",
            Kind::Merge => "merge",
        }
    }

    pub fn from_db(s: &str) -> Self {
        if s == "merge" {
            Kind::Merge
        } else {
            Kind::Approve
        }
    }
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Quick => "quick",
            Verdict::Steady => "steady",
            Verdict::Bold => "bold",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "quick" => Verdict::Quick,
            "bold" => Verdict::Bold,
            _ => Verdict::Steady,
        }
    }
}

/// One approval or merge pressed in meat proxy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Unix seconds.
    pub at: i64,
    pub kind: Kind,
    pub pr_id: String,
    /// Minutes the pull request waited on the user; None when that is not known.
    pub wait_minutes: Option<i64>,
    pub lines: u32,
    pub verdict: Verdict,
}

fn minutes_since(iso: &str, now: i64) -> Option<i64> {
    let then = DateTime::parse_from_rfc3339(iso).ok()?.timestamp();
    Some(((now - then) / 60).max(0))
}

/// The user approved `pr` at `now` (Unix seconds).
pub fn approval(pr: &PullRequest, now: i64) -> Event {
    // Asked by name: from the ask. Otherwise from the opening, known only to the hour.
    let wait = pr
        .asked
        .as_ref()
        .and_then(|ask| minutes_since(&ask.at, now))
        .or((pr.opened_hours > 0).then_some(pr.opened_hours as i64 * 60));
    let lines = pr.additions + pr.deletions;
    let verdict = match wait {
        Some(m) if lines > BIG_LINES && m * SKIM_LINES_PER_MINUTE < i64::from(lines) => {
            Verdict::Bold
        }
        Some(m) if lines <= BIG_LINES && m <= QUICK_MINUTES => Verdict::Quick,
        _ => Verdict::Steady,
    };
    Event {
        at: now,
        kind: Kind::Approve,
        pr_id: pr.id.clone(),
        wait_minutes: wait,
        lines,
        verdict,
    }
}

/// The user merged their approved `pr` at `now`. It waited since the last approval, or since a
/// later push, whichever came last.
pub fn merge(pr: &PullRequest, now: i64) -> Event {
    let ready = (!pr.approved_at.is_empty()).then(|| {
        // Both are UTC timestamps in one format, so they compare as text.
        pr.approved_at.as_str().max(pr.last_commit_at.as_str())
    });
    let wait = ready.and_then(|at| minutes_since(at, now));
    Event {
        at: now,
        kind: Kind::Merge,
        pr_id: pr.id.clone(),
        wait_minutes: wait,
        lines: pr.additions + pr.deletions,
        verdict: match wait {
            Some(m) if m <= QUICK_MINUTES => Verdict::Quick,
            _ => Verdict::Steady,
        },
    }
}

/// `event` is faster than every other approval (or merge) of the seven days before it. Beating
/// nothing is no record, a tie is none either, and a bold approval never sets one.
pub fn new_best(events: &[Event], event: &Event) -> bool {
    let Some(wait) = event.wait_minutes.filter(|_| event.verdict != Verdict::Bold) else {
        return false;
    };
    let mut earlier = events
        .iter()
        .filter(|e| {
            e.kind == event.kind
                && e.verdict != Verdict::Bold
                && e.at <= event.at
                && e.at > event.at - 7 * 86_400
                && !(e.at == event.at && e.pr_id == event.pr_id)
        })
        .filter_map(|e| e.wait_minutes)
        .peekable();
    earlier.peek().is_some() && earlier.all(|w| wait < w)
}

/// Minutes as a short span: 8m, 3h, 2d.
pub fn span(minutes: i64) -> String {
    match minutes {
        ..1 => "under 1m".to_owned(),
        1..60 => format!("{minutes}m"),
        60..2880 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

/// One day of the grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Day {
    pub date: Option<NaiveDate>,
    pub approvals: u32,
    pub merges: u32,
    pub quick: u32,
    pub bold: u32,
}

impl Day {
    /// What the day weighs on the grid: a quick one counts double.
    fn score(&self) -> u32 {
        self.approvals + self.merges + self.quick
    }

    /// Everything cleared that day was a bold approval: drawn hollow, it earns no ink.
    pub fn only_bold(&self) -> bool {
        self.bold > 0 && self.bold == self.approvals + self.merges
    }
}

/// What the Pace panel shows, as of one moment in the user's time zone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// `WEEKS` columns of seven days, Monday first; days after today have no date.
    pub weeks: Vec<[Day; 7]>,
    /// Days in a row with at least one approval or merge, up to today (or yesterday, while today
    /// still has time).
    pub streak: u32,
    pub best_streak: u32,
    /// In the last seven days.
    pub quick_approvals: u32,
    pub quick_merges: u32,
    pub bold: u32,
    /// The shortest wait of the last seven days, bold approvals aside.
    pub fastest_approval: Option<i64>,
    pub fastest_merge: Option<i64>,
    /// Half of the merges of the last 30 days came this soon after approval, or faster.
    pub median_merge_minutes: Option<i64>,
    pub total: u32,
    /// The quartiles of the active days' scores: a day past each is inked one level darker, so
    /// the darkest cells stay rare whatever the user's usual pace is.
    pub cuts: [u32; 3],
}

impl Stats {
    pub fn new(events: &[Event], now: DateTime<FixedOffset>) -> Self {
        let offset = *now.offset();
        let day_of = |at: i64| {
            DateTime::from_timestamp(at, 0).map(|t| t.with_timezone(&offset).date_naive())
        };
        let today = now.date_naive();
        let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
        let start = monday - Days::new(7 * (WEEKS as u64 - 1));
        let mut weeks: Vec<[Day; 7]> = (0..WEEKS)
            .map(|w| {
                std::array::from_fn(|d| {
                    let date = start + Days::new((w * 7 + d) as u64);
                    Day {
                        date: (date <= today).then_some(date),
                        ..Day::default()
                    }
                })
            })
            .collect();
        let mut stats = Stats::default();
        let mut active: Vec<NaiveDate> = Vec::new();
        let mut merge_waits: Vec<i64> = Vec::new();
        let week_ago = now.timestamp() - 7 * 86_400;
        let month_ago = now.timestamp() - 30 * 86_400;
        for event in events {
            let Some(date) = day_of(event.at) else {
                continue;
            };
            active.push(date);
            let quick = event.verdict == Verdict::Quick;
            if event.at > week_ago {
                match (event.kind, event.verdict) {
                    (Kind::Approve, Verdict::Quick) => stats.quick_approvals += 1,
                    (Kind::Merge, Verdict::Quick) => stats.quick_merges += 1,
                    (_, Verdict::Bold) => stats.bold += 1,
                    _ => {}
                }
                if event.verdict != Verdict::Bold
                    && let Some(wait) = event.wait_minutes
                {
                    let best = match event.kind {
                        Kind::Approve => &mut stats.fastest_approval,
                        Kind::Merge => &mut stats.fastest_merge,
                    };
                    *best = Some(best.map_or(wait, |b| b.min(wait)));
                }
            }
            if event.kind == Kind::Merge
                && event.at > month_ago
                && let Some(wait) = event.wait_minutes
            {
                merge_waits.push(wait);
            }
            if date < start || date > today {
                continue;
            }
            let i = (date - start).num_days() as usize;
            let day = &mut weeks[i / 7][i % 7];
            match event.kind {
                Kind::Approve => day.approvals += 1,
                Kind::Merge => day.merges += 1,
            }
            day.quick += u32::from(quick);
            day.bold += u32::from(event.verdict == Verdict::Bold);
            stats.total += 1;
        }
        active.sort_unstable();
        active.dedup();
        // Runs of consecutive days, for the best one and the one that is still going.
        let mut run = 0u32;
        for (i, date) in active.iter().enumerate() {
            run = if i > 0 && *date == active[i - 1] + Days::new(1) {
                run + 1
            } else {
                1
            };
            stats.best_streak = stats.best_streak.max(run);
        }
        if active
            .last()
            .is_some_and(|last| *last == today || *last + Days::new(1) == today)
        {
            stats.streak = run;
        }
        merge_waits.sort_unstable();
        stats.median_merge_minutes = merge_waits
            .get(merge_waits.len().saturating_sub(1) / 2)
            .copied();
        let mut scores: Vec<u32> = weeks
            .iter()
            .flatten()
            .map(Day::score)
            .filter(|s| *s > 0)
            .collect();
        scores.sort_unstable();
        if let Some(last) = scores.len().checked_sub(1) {
            stats.cuts = [0.25, 0.5, 0.75].map(|p| scores[(last as f32 * p).round() as usize]);
        }
        stats.weeks = weeks;
        stats
    }

    /// How strongly a day is inked, 0 (nothing) to 4 (past the top quartile).
    pub fn level(&self, day: &Day) -> u8 {
        let score = day.score();
        if score == 0 {
            return 0;
        }
        1 + self.cuts.iter().filter(|cut| score > **cut).count() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ask;

    const NOW: &str = "2026-11-21T15:00:00Z";

    fn now() -> i64 {
        DateTime::parse_from_rfc3339(NOW).unwrap().timestamp()
    }

    fn asked(minutes_ago: i64, lines: u32) -> PullRequest {
        let mut pr = crate::github::demo_prs().remove(0);
        let at = DateTime::from_timestamp(now() - minutes_ago * 60, 0).unwrap();
        pr.asked = Some(Ask {
            at: at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            hours_ago: 0,
            again: false,
        });
        pr.additions = lines;
        pr.deletions = 0;
        pr
    }

    #[test]
    fn a_quick_approval_counts_unless_the_pull_request_is_too_big_to_have_read() {
        let verdict = |minutes, lines| approval(&asked(minutes, lines), now()).verdict;
        assert_eq!(verdict(20, 120), Verdict::Quick);
        assert_eq!(verdict(61, 120), Verdict::Steady, "past the hour");
        assert_eq!(approval(&asked(20, 120), now()).wait_minutes, Some(20));
        // 1,200 lines need 12 minutes at the skimming pace; under that is bold, over is steady:
        // a big one is never quick.
        assert_eq!(verdict(5, 1200), Verdict::Bold);
        assert_eq!(verdict(30, 1200), Verdict::Steady);
        // Not asked by name: timed from the opening, to the hour.
        let mut pr = asked(0, 50);
        pr.asked = None;
        pr.opened_hours = 3;
        assert_eq!(approval(&pr, now()).wait_minutes, Some(180));
    }

    #[test]
    fn a_merge_is_timed_from_the_approval_or_a_later_push() {
        let mut pr = asked(0, 2000);
        pr.approved_at = "2026-11-21T14:40:00Z".into();
        pr.last_commit_at = "2026-11-21T09:00:00Z".into();
        let quick = merge(&pr, now());
        assert_eq!(
            (quick.wait_minutes, quick.verdict),
            (Some(20), Verdict::Quick)
        );
        // Approved long ago, pushed 50 minutes ago: it was ready to go 50 minutes ago.
        pr.approved_at = "2026-11-19T10:00:00Z".into();
        pr.last_commit_at = "2026-11-21T14:10:00Z".into();
        assert_eq!(merge(&pr, now()).wait_minutes, Some(50));
        // Not approved as far as we know: counted, no medal.
        pr.approved_at.clear();
        let merged = merge(&pr, now());
        assert_eq!(
            (merged.wait_minutes, merged.verdict),
            (None, Verdict::Steady)
        );
    }

    #[test]
    fn the_fastest_of_the_week_is_a_record_only_when_it_beats_one() {
        let ev = |days_ago: i64, kind, wait, verdict| Event {
            at: now() - days_ago * 86_400,
            kind,
            pr_id: format!("a/b#{days_ago}{wait}"),
            wait_minutes: Some(wait),
            lines: 10,
            verdict,
        };
        let events = vec![
            // Too old to count this week.
            ev(9, Kind::Approve, 1, Verdict::Quick),
            ev(2, Kind::Approve, 6, Verdict::Quick),
            // Bold is never the fastest.
            ev(1, Kind::Approve, 0, Verdict::Bold),
            ev(1, Kind::Merge, 3, Verdict::Quick),
        ];
        let stats = Stats::new(&events, DateTime::parse_from_rfc3339(NOW).unwrap());
        assert_eq!((stats.fastest_approval, stats.fastest_merge), (Some(6), Some(3)));
        assert!(new_best(&events, &ev(0, Kind::Approve, 5, Verdict::Quick)));
        assert!(!new_best(&events, &ev(0, Kind::Approve, 6, Verdict::Quick)), "a tie");
        assert!(!new_best(&events, &ev(0, Kind::Approve, 2, Verdict::Bold)));
        assert!(!new_best(&events, &ev(0, Kind::Merge, 4, Verdict::Quick)), "slower");
        assert!(
            !new_best(&events[..1], &ev(0, Kind::Approve, 5, Verdict::Quick)),
            "nothing this week to beat"
        );
    }

    #[test]
    fn days_follow_the_users_clock_and_a_streak_survives_until_today_ends() {
        // Saturday 21 November, 16:00 in Warsaw.
        let local = DateTime::parse_from_rfc3339("2026-11-21T16:00:00+01:00").unwrap();
        let event = |iso: &str, verdict| Event {
            at: DateTime::parse_from_rfc3339(iso).unwrap().timestamp(),
            kind: Kind::Approve,
            pr_id: "a/b#1".into(),
            wait_minutes: Some(5),
            lines: 10,
            verdict,
        };
        let events = [
            // 23:30 UTC on Thursday is already Friday in Warsaw.
            event("2026-11-19T23:30:00Z", Verdict::Quick),
            event("2026-11-19T10:00:00Z", Verdict::Steady),
            // A run of three, two weeks back.
            event("2026-11-02T10:00:00Z", Verdict::Steady),
            event("2026-11-03T10:00:00Z", Verdict::Steady),
            event("2026-11-04T10:00:00Z", Verdict::Steady),
        ];
        let stats = Stats::new(&events, local);
        // Nothing today yet, but Thursday and Friday were active: the streak is two and alive.
        assert_eq!((stats.streak, stats.best_streak), (2, 3));
        assert_eq!(stats.quick_approvals, 1);
        let week = stats.weeks.last().unwrap();
        // Monday first: Thursday is 3, Friday 4, Saturday (today) 5, Sunday has no date.
        assert_eq!(
            (week[3].approvals, week[4].approvals, week[4].quick),
            (1, 1, 1)
        );
        // Inked against the user's own days: four days of one, and Friday's two (one quick) is
        // past every quartile.
        assert_eq!(
            (
                stats.level(&week[3]),
                stats.level(&week[4]),
                stats.level(&week[5])
            ),
            (1, 4, 0)
        );
        assert!(week[5].date.is_some() && week[6].date.is_none());
        // Two idle days end it.
        let later = DateTime::parse_from_rfc3339("2026-11-23T09:00:00+01:00").unwrap();
        assert_eq!(Stats::new(&events, later).streak, 0);
    }
}
