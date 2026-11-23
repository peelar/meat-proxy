//! Pace: the panel above the Inbox and the pills on rows.

use crate::*;

// --- Pace ---

pub(crate) const PACE_CELL: f32 = 12.0;
pub(crate) const PACE_GAP: f32 = 3.0;
/// The streak, the score beside it, their labels, and the grid's chrome: tabular figures, so a
/// number that rolls keeps its width.
pub(crate) const PACE_HERO: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(52.0, 700.0).lh(56.0).tracking(-1.5)
};
pub(crate) const PACE_NUMBER: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(26.0, 650.0).lh(30.0)
};
pub(crate) const PACE_UNIT: TextStyle = TextStyle::new(16.0, 550.0).lh(30.0);
pub(crate) const PACE_LABEL: TextStyle = TextStyle::new(11.0, 500.0);
pub(crate) const PACE_SUB: TextStyle = TextStyle::new(11.0, 400.0);
pub(crate) const PACE_CHROME: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(10.0, 500.0)
};

/// The grid's ink for a level: a quiet mint ramp that only reaches the button's mint at the top.
pub(crate) fn pace_ink(k: &Kit, level: u8) -> Color {
    let ramp = if k.dark {
        [(0.34, 0.035), (0.47, 0.06), (0.63, 0.085), (0.83, 0.10)]
    } else {
        [(0.90, 0.04), (0.78, 0.075), (0.62, 0.10), (0.44, 0.10)]
    };
    match level {
        0 => k.c.text_base.alpha(if k.dark { 0.07 } else { 0.06 }),
        n => {
            let (l, chroma) = ramp[usize::from(n.min(4)) - 1];
            oklch(l, chroma, 161.0)
        }
    }
}

/// One day of the grid, with what was cleared in its tooltip. A day of only bold approvals is
/// drawn hollow; today is ringed, dashed while it still waits for its first one.
pub(crate) fn pace_cell(k: &Kit, stats: &pace::Stats, day: &pace::Day, size: f32, today: Option<chrono::NaiveDate>) -> El {
    let c = &k.c;
    let Some(date) = day.date else {
        return el().size(size, size);
    };
    let mut done = Vec::new();
    if day.approvals > 0 {
        done.push(plural(day.approvals, "approval", "approvals"));
    }
    if day.merges > 0 {
        done.push(plural(day.merges, "merge", "merges"));
    }
    let mut what = if done.is_empty() {
        "nothing cleared".to_owned()
    } else {
        done.join(", ")
    };
    if day.quick > 0 {
        what.push_str(&format!(", {} quick", day.quick));
    }
    if day.bold > 0 {
        what.push_str(&format!(", {} bold", day.bold));
    }
    let mut cell = el()
        .size(size, size)
        .radius(size / 4.0)
        .help(format!("{} · {what}", date.format("%a %-d %b")));
    cell = if day.only_bold() {
        cell.border(1.0, c.text_muted.alpha(0.6)).border_dash(2.0, 1.5)
    } else {
        cell.fill(pace_ink(k, stats.level(day)))
    };
    if Some(date) == today {
        cell = if day.approvals + day.merges == 0 {
            cell.border(1.0, c.text_muted).border_dash(2.0, 2.0)
        } else {
            let lit = cell.border(1.5, c.text_base.alpha(0.9));
            if k.dark {
                lit.shadow(0.0, 8.0, mint(k).alpha(0.45))
            } else {
                lit
            }
        };
    }
    cell
}

/// The count that is landing right now, how long ago it landed, and the score as it stood before.
pub(crate) fn pace_landing(s: &App, now: chrono::DateTime<chrono::FixedOffset>) -> Option<(f32, &pace::Event, pace::Stats)> {
    let (event, at) = s.pace_flash.iter().max_by_key(|(_, at)| *at)?;
    let age = at.elapsed().as_secs_f32();
    if age > 1.0 {
        return None;
    }
    let before: Vec<pace::Event> = s
        .pace_events
        .iter()
        .filter(|e| !(e.at == event.at && e.pr_id == event.pr_id && e.kind == event.kind))
        .cloned()
        .collect();
    Some((age, event, pace::Stats::new(&before, now)))
}

/// A number that rolls up from what it was, like an odometer, while a count lands.
pub(crate) fn rolling(before: Option<String>, after: String, style: TextStyle, tone: Color, age: f32) -> El {
    let p = ease_out(phase(age, 0.2, 0.48));
    match before.filter(|b| *b != after && p < 1.0) {
        None => text(after).font(style).color(tone),
        Some(before) => col().h(style.line_height).clip_rounded(0.0).child(
            col()
                .offset(0.0, -style.line_height * p)
                .offset_motion(rune::Anim::INSTANT)
                .child(text(before).font(style).color(tone))
                .child(text(after).font(style).color(tone)),
        ),
    }
}

/// "53m" as a number and its unit.
pub(crate) fn split_unit(value: &str) -> (String, String) {
    let at = value
        .find(|ch: char| ch.is_alphabetic())
        .unwrap_or(value.len());
    (value[..at].to_owned(), value[at..].to_owned())
}

/// One column of the score: the number, what it counts, and over when.
pub(crate) fn pace_column(k: &Kit, value: El, label: &str, sub: El) -> El {
    col()
        .gap(3.0)
        .child(value)
        .child(text(label.to_owned()).font(PACE_LABEL).color(k.c.text_muted))
        .child(sub)
}

/// The newest count of this kind that beat the week's fastest, if one is still being celebrated:
/// how long ago it landed, and its wait.
pub(crate) fn pace_record(s: &App, kind: pace::Kind) -> Option<(f32, i64)> {
    s.pace_flash
        .iter()
        .filter(|(e, _)| e.kind == kind && pace::new_best(&s.pace_events, e))
        .map(|(e, at)| (at.elapsed().as_secs_f32(), e.wait_minutes.unwrap_or_default()))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Above the Inbox: the streak, this week's quick approvals and merges, and a grid of the last
/// weeks, GitHub-style. Click the header to fold it to one line; the fold is remembered.
pub(crate) fn pace_panel(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let now = chrono::Local::now().fixed_offset();
    let today = now.date_naive();
    let last = s.pace_events.last().map_or(0, |e| e.at);
    let key = (s.pace_events.len(), last, now.timestamp() / 60);
    let stats = match &*s.pace_stats.borrow() {
        Some((at, stats)) if *at == key => stats.clone(),
        _ => pace::Stats::new(&s.pace_events, now),
    };
    *s.pace_stats.borrow_mut() = Some((key, stats.clone()));
    let landing = pace_landing(s, now);
    let age = landing.as_ref().map_or(f32::MAX, |(age, _, _)| *age);
    let before = |f: &dyn Fn(&pace::Stats) -> String| landing.as_ref().map(|(_, _, b)| f(b));
    let collapsed = s.pace_collapsed;
    let days: Vec<&pace::Day> = stats.weeks.iter().flatten().filter(|d| d.date.is_some()).collect();
    let cleared_today = days.last().is_some_and(|d| d.approvals + d.merges > 0);

    let caption = if collapsed {
        let quick = stats.quick_approvals + stats.quick_merges;
        let mut summary = Vec::new();
        if stats.streak > 0 {
            summary.push(format!("{}-day streak", stats.streak));
        }
        if quick > 0 {
            summary.push(format!("{quick} quick this week"));
        }
        if summary.is_empty() {
            None
        } else {
            Some(text(summary.join("  ·  ")).font(k.ty.small).color(c.text_muted))
        }
    } else if stats.bold > 0 {
        Some(row()
            .center_y()
            .gap(5.0)
            .child(rune::icon(&EYE, 12.0).color(c.warning.fg).a11y_hidden())
            .child(
                text(format!(
                    "{} this week. Faster than they read; no medal.",
                    plural(stats.bold, "bold approval", "bold approvals")
                ))
                .font(k.ty.small)
                .color(c.text_faint),
            ))
    } else {
        Some(
            text("Quick: within the hour of being asked, or of the approval for a merge")
                .font(k.ty.small)
                .color(c.text_faint),
        )
    };
    // Folded, the last week as a strip, with the streak's days joined into one bar.
    let week = &days[days.len().saturating_sub(7)..];
    let active = |i: usize| week.get(i).is_some_and(|d| d.approvals + d.merges > 0);
    let mut strip = row().center_y();
    for (i, day) in week.iter().enumerate() {
        let (joined_left, joined_right) = (
            i > 0 && active(i) && active(i - 1),
            active(i) && active(i + 1),
        );
        if i > 0 && !joined_left {
            strip = strip.child(el().w(2.0));
        }
        let r = 2.5;
        strip = strip.child(pace_cell(k, &stats, day, 9.0, None).corners([
            if joined_left { 0.0 } else { r },
            if joined_right { 0.0 } else { r },
            if joined_right { 0.0 } else { r },
            if joined_left { 0.0 } else { r },
        ]));
    }

    let header = row()
        .key("pace-header")
        .w_full()
        .center_y()
        .justify_between()
        .hover_group()
        .role("button")
        .label(if collapsed { "Show Scoreboard" } else { "Hide Scoreboard" })
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            tx.motion_curve(rune::Curve::tween(0.16, rune::Easing::cubic(0.2, 0.0, 0.0, 1.0)));
            s.toggle_pace_collapsed();
        })
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(rune::icon(&BOLT, 14.0).color(mint(k)).a11y_hidden())
                .child(text("Scoreboard").font(k.ty.ui_medium).color(c.text_base))
                .children(caption.is_some().then(|| el().size(1.0, 10.0).fill(c.border_base)))
                .children(caption),
        )
        .child(
            row()
                .center_y()
                .gap(12.0)
                .child(if collapsed { strip } else { el() })
                .child(
                    rune::icon(k.glyph.chevron_down, 14.0)
                        .color(c.text_muted)
                        .rotate(if collapsed { -std::f32::consts::FRAC_PI_2 } else { 0.0 })
                        .group_opacity(0.5, 1.0)
                        .a11y_hidden(),
                ),
        );
    let card = col()
        .key("pace-panel")
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(6.0)
        .radius(9.0)
        .border(1.0, c.border_weak)
        .fill(surface(k))
        .child(header);
    if collapsed {
        return card;
    }

    // The score: four columns on one baseline. The streak waits in grey until today has one.
    let streak_tone = match (stats.streak, cleared_today) {
        (0, _) => c.text_faint,
        (_, false) => c.text_muted,
        _ => c.text_base,
    };
    let hero = pace_column(
        k,
        rolling(
            before(&|b| b.streak.to_string()),
            stats.streak.to_string(),
            PACE_HERO,
            streak_tone,
            age,
        ),
        "day streak",
        text(if stats.best_streak > 0 {
            format!("best {}", stats.best_streak)
        } else {
            "clear one to start".to_owned()
        })
        .font(PACE_SUB)
        .color(c.text_faint),
    );
    let number = |value: String, was: Option<String>, tone: Color| {
        let (n, unit) = split_unit(&value);
        let was = was.map(|w| split_unit(&w).0);
        row()
            .items_end()
            .gap(1.0)
            // The hero's baseline sits this much higher in its taller line.
            .pb(5.0)
            .child(rolling(was, n, PACE_NUMBER, tone, age))
            .child(if unit.is_empty() {
                el()
            } else {
                text(unit).font(PACE_UNIT).color(c.text_muted)
            })
    };
    let tile = |value: El, label: &str, sub: El| {
        pace_column(k, value, label, sub).grow(1.0).min_w(92.0)
    };
    // Under each count, the week's fastest; while a new one is celebrated, it says so in its ink.
    let fastest = |best: Option<i64>, kind: pace::Kind, tone: Color| match (pace_record(s, kind), best) {
        (Some((_, wait)), _) => row()
            .center_y()
            .gap(4.0)
            .child(rune::icon(&TROPHY, 11.0).color(tone).a11y_hidden())
            .child(
                text(format!("new best · {}", pace::span(wait)))
                    .font(TextStyle::new(11.0, 600.0))
                    .color(tone),
            ),
        (None, Some(wait)) => text(format!("fastest {} · 7 days", pace::span(wait)))
            .font(PACE_SUB)
            .color(c.text_faint),
        (None, None) => text("last 7 days").font(PACE_SUB).color(c.text_faint),
    };
    let divider = || el().size(1.0, 40.0).fill(c.border_weak).self_center();
    let median = |st: &pace::Stats| st.median_merge_minutes.map_or("–".to_owned(), pace::span);
    let score = row()
        .grow(1.0)
        .min_w(0.0)
        .items_end()
        .gap(16.0)
        .pt(18.0)
        .pb(16.0)
        .child(hero)
        .child(divider())
        .child(tile(
            number(
                stats.quick_approvals.to_string(),
                before(&|b| b.quick_approvals.to_string()),
                if stats.quick_approvals > 0 { mint(k) } else { c.text_faint },
            ),
            "quick approvals",
            fastest(stats.fastest_approval, pace::Kind::Approve, mint(k)),
        ))
        .child(divider())
        .child(tile(
            number(
                stats.quick_merges.to_string(),
                before(&|b| b.quick_merges.to_string()),
                if stats.quick_merges > 0 { merge_tone(k) } else { c.text_faint },
            ),
            "quick merges",
            fastest(stats.fastest_merge, pace::Kind::Merge, merge_tone(k)),
        ))
        .child(divider())
        .child(tile(
            number(median(&stats), before(&median), c.text_base),
            "approval to merge",
            text("median, 30 days").font(PACE_SUB).color(c.text_faint),
        ));

    // The grid: weeks left to right, Monday on top, months named where they start.
    let step = PACE_CELL + PACE_GAP;
    let grid_w = pace::WEEKS as f32 * step - PACE_GAP;
    let mut months = el().w(grid_w).h(13.0);
    let mut last_month = None;
    for (w, week) in stats.weeks.iter().enumerate() {
        let Some(date) = week[0].date else { continue };
        let month = chrono::Datelike::month(&date);
        // Named on the first full week of a month, and not squeezed against the right edge.
        if last_month != Some(month)
            && w + 2 < pace::WEEKS
            && (last_month.is_some() || chrono::Datelike::day(&date) <= 7)
        {
            months = months.child(
                text(date.format("%b").to_string())
                    .font(PACE_CHROME)
                    .color(c.text_faint)
                    .absolute(w as f32 * step, 0.0),
            );
        }
        last_month = Some(month);
    }
    let weekdays = (0..7).fold(col().gap(PACE_GAP).w(22.0), |col, d| {
        col.child(
            row().h(PACE_CELL).center_y().child(
                text(match d {
                    0 => "Mon",
                    2 => "Wed",
                    4 => "Fri",
                    _ => "",
                })
                .font(PACE_CHROME)
                .color(c.text_faint),
            ),
        )
    });
    // What just landed: today's cell pops and sends out one ring, for a quick one only.
    let ripple = landing
        .as_ref()
        .filter(|(age, event, _)| *age < 0.95 && event.verdict == pace::Verdict::Quick)
        .map(|(age, event, _)| {
            let tone = if event.kind == pace::Kind::Merge { merge_tone(k) } else { mint(k) };
            (*age, tone)
        });
    let mut cells = row().gap(PACE_GAP);
    for (w, week) in stats.weeks.iter().enumerate() {
        let mut column = col().gap(PACE_GAP);
        for (d, day) in week.iter().enumerate() {
            let mut cell = pace_cell(k, &stats, day, PACE_CELL, Some(today))
                .key(format!("pace-day-{w}-{d}"))
                // Unfolding, the grid washes in from the oldest week.
                .transition(rune::Transition::scale(0.6))
                .enter_delay(w as f32 * 0.012 + d as f32 * 0.008);
            if let (Some((age, tone)), Some(true)) = (ripple, day.date.map(|date| date == today)) {
                let pop = phase(age, 0.35, 0.8);
                let ring = ease_out(phase(age, 0.35, 0.95));
                cell = el()
                    .size(PACE_CELL, PACE_CELL)
                    .raise()
                    .child(
                        cell.absolute(0.0, 0.0)
                            .scale(1.0 + 0.3 * (pop * std::f32::consts::PI).sin())
                            .scale_motion(rune::Anim::INSTANT),
                    )
                    .child(
                        el().size(PACE_CELL, PACE_CELL)
                            .radius(PACE_CELL / 4.0)
                            .border(1.5, tone)
                            .absolute(0.0, 0.0)
                            .scale(1.0 + 1.8 * ring)
                            .scale_motion(rune::Anim::INSTANT)
                            .opacity(if age < 0.35 { 0.0 } else { 0.6 * (1.0 - ring) })
                            .opacity_motion(rune::Anim::INSTANT),
                    );
            }
            column = column.child(cell);
        }
        cells = cells.child(column);
    }
    let legend = (0..5u8)
        .fold(
            row()
                .gap(PACE_GAP)
                .center_y()
                .group_opacity(0.0, 1.0)
                .child(text("Less").font(PACE_CHROME).color(c.text_faint).pr(3.0)),
            |legend, n| legend.child(el().size(10.0, 10.0).radius(2.5).fill(pace_ink(k, n))),
        )
        .child(text("More").font(PACE_CHROME).color(c.text_faint).pl(3.0));
    let grid = col()
        .gap(4.0)
        .hover_group()
        .child(row().gap(6.0).child(el().w(22.0)).child(months))
        .child(row().gap(6.0).child(weekdays).child(cells))
        .child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .pl(28.0)
                .child(
                    text(format!("{} cleared in {} weeks", stats.total, pace::WEEKS))
                        .font(PACE_CHROME)
                        .color(c.text_faint),
                )
                .child(legend),
        );
    card.child(
        row()
            .key("pace-body")
            .w_full()
            .gap(28.0)
            .center_y()
            .transition(rune::Transition::blur(4.0))
            .child(score)
            .child(grid),
    )
}

/// On a row whose approval or merge was just counted: how quick it was, or an outline and a
/// raised eyebrow for a big one approved faster than it reads. It rises in sharp, a quick one
/// catches the light once, and it drifts off.
pub(crate) fn pace_pill(s: &App, k: &Kit, pr: &PullRequest) -> Option<El> {
    let c = &k.c;
    let (event, age) = s.pace_flash_of(&pr.id)?;
    let record = pace::new_best(&s.pace_events, event);
    let wait = event.wait_minutes.map(pace::span).unwrap_or_default();
    let rise = ease_out(phase(age, 0.0, 0.35));
    let leave = phase(age, PACE_FLASH_SECS - 0.5, PACE_FLASH_SECS);
    let pill = row()
        .key(format!("pace-pill-{}", pr.id))
        .h(22.0)
        .px(8.0)
        .radius(11.0)
        .clip_rounded(11.0)
        .gap(4.0)
        .center_y()
        .opacity(rise * (1.0 - leave))
        .opacity_motion(rune::Anim::INSTANT)
        .offset(0.0, (1.0 - rise) * 8.0 - 4.0 * leave)
        .offset_motion(rune::Anim::INSTANT)
        .scale(0.92 + 0.08 * rise)
        .scale_motion(rune::Anim::INSTANT)
        .blur(6.0 * (1.0 - rise) + 4.0 * leave)
        .blur_motion(rune::Anim::INSTANT);
    let label = |s: String, tone: Color| text(s).font(TextStyle::new(11.5, 600.0)).color(tone);
    Some(match event.verdict {
        // A new best is celebrated even when it was not within the hour.
        pace::Verdict::Quick | pace::Verdict::Steady
            if event.verdict == pace::Verdict::Quick || record =>
        {
            let tone = if event.kind == pace::Kind::Merge { merge_tone(k) } else { mint(k) };
            let mut pill = pill
                .fill(tone.alpha(0.14))
                .border_gradient(1.0, tone.alpha(0.35), tone.alpha(0.10))
                .help(if record {
                    "Faster than any other this week"
                } else if event.kind == pace::Kind::Merge {
                    "Merged within the hour of it being ready"
                } else {
                    "Approved within the hour of being asked"
                })
                .child(rune::icon(if record { &TROPHY } else { &BOLT }, 12.0).color(tone).a11y_hidden())
                .child(label(
                    format!("{} · {wait}", if record { "New best" } else { "Quick" }),
                    tone,
                ));
            let sweep = phase(age, 0.30, 0.85);
            if sweep > 0.0 && sweep < 1.0 {
                let shine = c.white.alpha(if k.dark { 0.22 } else { 0.55 });
                pill = pill.child(
                    el().size(36.0, 22.0)
                        .absolute(0.0, 0.0)
                        .offset(-36.0 + 216.0 * ease_out(sweep), 0.0)
                        .offset_motion(rune::Anim::INSTANT)
                        .linear_gradient(
                            std::f32::consts::FRAC_PI_2,
                            [(0.0, shine.alpha(0.0)), (0.5, shine), (1.0, shine.alpha(0.0))],
                        ),
                );
            }
            pill
        }
        pace::Verdict::Bold => {
            let lines = if event.lines >= 1000 {
                format!("{:.1}k", f64::from(event.lines) / 1000.0)
            } else {
                event.lines.to_string()
            };
            let blink = 1.0 - 0.8 * (phase(age, 0.5, 0.7) * std::f32::consts::PI).sin();
            pill.fill(c.text_base.alpha(0.06))
                .border(1.0, c.text_muted.alpha(0.5))
                .border_dash(3.0, 2.0)
                .help("Faster than anyone reads that much. Counted, no medal")
                .child(
                    rune::icon(&EYE, 12.0)
                        .color(c.warning.fg)
                        .opacity(blink)
                        .opacity_motion(rune::Anim::INSTANT)
                        .a11y_hidden(),
                )
                .child(label(format!("{lines} lines in {wait}"), c.text_muted))
        }
        pace::Verdict::Quick | pace::Verdict::Steady => return None,
    })
}

/// Settings › Pace: one switch for the whole thing.
pub(crate) fn pace_section(s: &App, k: &Kit) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(text("Scoreboard").font(SECTION_TITLE).color(k.c.text_base))
        .child(
            text("Counts the pull requests you approve and merge from here, says on the row when one was quick, and shows your streak and median time to merge in a Scoreboard above the Inbox. A big pull request approved faster than it can be read earns no credit. Off, nothing is counted, flagged or shown.")
                .wrap()
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(setup_row(
            k,
            "Enable scoreboard",
            Switch::new(s.pace_enabled)
                .label("Enable scoreboard")
                .hide_label()
                .on_toggle(|s: &mut App| s.toggle_pace())
                .build(k)
                .key("option-pace"),
        ))
}
