//! The Inbox: the top bar, the sections and each pull request's row.

use crate::*;

/// The logo and name. The Inbox shows them above its title; the setup page, in its bar.
pub(crate) fn brand(s: &App, k: &Kit) -> El {
    let keycap = if let Some(image) = &s.logo {
        el().size(22.0, 22.0).pixels(image.pixels())
    } else {
        el().size(22.0, 22.0)
    };
    row()
        .key("brand")
        .center_y()
        .gap(9.0)
        .on_press(|s: &mut App| {
            if s.page == Page::Inbox {
                s.selected_repo = None;
            }
        })
        .child(keycap)
        .child(
            text(crate::profile::app_name())
                .font(TextStyle::new(14.0, 650.0))
                .color(k.c.text_base),
        )
}

/// `hide_brand` while the intro flies its own logo and name into place.
pub(crate) fn top_bar(s: &App, k: &Kit, hide_brand: bool) -> El {
    let c = &k.c;
    let settings_btn = if s.page == Page::Inbox {
        rune_kit::IconButton::new(&GEAR)
            .variant(rune_kit::IconButtonVariant::Ghost)
            .control_size(ControlSize::Large)
            .label("Settings")
            .help("Settings (⌘,)")
            .on_press(|s: &mut App| s.settings_open = true)
            .build(k)
            .key("toggle-settings-btn")
    } else {
        el()
    };
    let context = match s.page {
        Page::Onboarding => "Setup".to_owned(),
        _ => s.sync_label(),
    };
    let sync_status = row()
        .key("sync-status")
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .help("Sync now (⌘R)")
        .on_press(|s: &mut App| s.sync_now())
        .child(text(context).font(k.ty.mono).color(c.text_muted));
    let context = if matches!(s.page, Page::Inbox) {
        sync_status
    } else {
        row()
            .center_y()
            .child(text("Setup").font(k.ty.mono).color(c.text_muted))
    };
    col()
        .key("top-header")
        .w_full()
        .child(
            row()
                .w_full()
                .h(50.0)
                .pl(84.0)
                .pr(24.0)
                .center_y()
                .justify_between()
                .child(if s.page == Page::Inbox {
                    el()
                } else {
                    brand(s, k)
                        .opacity(if hide_brand { 0.0 } else { 1.0 })
                        .opacity_motion(rune::Anim::INSTANT)
                })
                .child(
                    row()
                        .center_y()
                        .gap(14.0)
                        .child(context)
                        .child(settings_btn),
                ),
        )
        .child(el().w_full().h(1.0).fill(c.border_weak))
}

/// Offered next to the logo once a newer version is downloaded; pressing it restarts into that version.
fn update_button(s: &App, k: &Kit) -> Option<El> {
    let tag = s.update.ready.as_ref()?;
    Some(
        row()
            .key("install-update")
            .role("button")
            .label(format!("Restart to update to {tag}"))
            .help("A new version is ready. Meat Proxy restarts to install it.")
            .focusable(true)
            .center_y()
            .px(8.0)
            .py(3.0)
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .hover(k.c.layer_02)
            .on_press(|s: &mut App| s.install_update())
            .child(text(format!("Update to {tag}")).font(k.ty.small).color(k.c.text_faint)),
    )
}

/// Sort mode and reset, as two icons that keep their slots on every tab so the search box never
/// moves. Off the Inbox they are dimmed and do nothing. The brain is tinted while a model sorts;
/// its tooltip says which mode is on.
fn sort_icons(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let live = s.filter == InboxFilter::Inbox;
    let ai = s.smart_active();
    let available = s.smart_available();
    let tip = if !available {
        "Sorted by rules. Turn on AI sorting in Settings to sort with a model."
    } else if ai {
        "Sorted by alien intelligence: a model reads each review request. Click to sort by rules alone, with nothing sent to a model."
    } else {
        "Sorted by rules alone. Nothing is sent to a model. Click to sort with AI."
    };
    let mut toggle = row()
        .key("sort-mode")
        .role("button")
        .label(if ai { "Sorted by AI" } else { "Sorted by rules" })
        .size(24.0, 24.0)
        .radius(6.0)
        .center()
        .help(tip)
        .child(rune::icon(&BRAIN, 14.0).color(if ai && live {
            model_ink(k)
        } else {
            c.text_faint
        }));
    if available && live {
        toggle = toggle
            .cursor(rune::Cursor::Pointer)
            .hover(c.hover)
            .on_press(|s: &mut App| s.toggle_smart_sort());
    }
    let reset = icon_action(&RESET, "Reset to the default order")
        .disabled(!live || s.group_order_is_default())
        .on_press(|s: &mut App| s.reset_group_order())
        .build(k)
        .key("reset-group-order");
    row()
        .center_y()
        .gap(2.0)
        .children(s.ai.feature.then_some(toggle))
        .child(reset)
}

pub(crate) fn inbox_tools(s: &App, k: &Kit) -> El {
    let labels = InboxFilter::TABS.map(|(_, label)| label);
    let filter_seg = SegmentedControl::new(&labels, Some(s.filter.index()))
        .label("Filter")
        .width(Some(280.0))
        .on_pick(|s: &mut App, idx, _| {
            s.filter = InboxFilter::from_index(idx);
        })
        .build(k)
        .key("inbox-filter-segments");

    let search = TextInput::new(&s.search_query)
        .placeholder("Search pull requests…")
        .label("Search")
        .width(Some(170.0))
        .on_edit(|s: &mut App| &mut s.search_query)
        .build(k)
        .key("inbox-search-input");

    let collapsed = s.all_sections_collapsed();
    let collapse = icon_action(
        if collapsed { &EXPAND_ALL } else { &COLLAPSE_ALL },
        if collapsed { "Expand all" } else { "Collapse all" },
    )
    .on_press_tx(|s: &mut App, tx: &mut Tx| {
        tx.motion_curve(rune::Curve::tween(
            0.12,
            rune::Easing::cubic(0.2, 0.0, 0.0, 1.0),
        ));
        s.toggle_collapse_all()
    })
    .build(k)
    .key("collapse-all");

    row()
        .w_full()
        .center_y()
        .justify_between()
        .gap(8.0)
        .child(filter_seg)
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(search)
                .child(
                    row()
                        .center_y()
                        .gap(2.0)
                        .child(collapse)
                        .child(sort_icons(s, k)),
                ),
        )
}

// Matching the resting surface keeps the glyph quiet. Keyboard focus paints a
// contrasting tile behind it even when the pointer is in another row.
pub(crate) fn author_badge(s: &App, pr: &PullRequest, k: &Kit) -> El {
    let size = 17.0;
    let mut avatar = row()
        .size(size, size)
        .radius(size / 2.0)
        .clip_rounded(size / 2.0)
        .center()
        .fill(k.c.layer_02)
        .a11y_hidden();
    avatar = if let Some(image) = s.avatars.get(&pr.author) {
        avatar.child(el().size(size, size).pixels(image.pixels()))
    } else {
        let initial = pr
            .author
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('?');
        avatar.child(
            text(initial.to_string())
                .font(TextStyle::new(10.0, 620.0))
                .color(k.c.text_muted),
        )
    };
    let handle = format!("@{}", pr.author);
    let author = row().center_y().gap(5.0).help(handle.clone()).child(avatar);
    if pr.ownership == Ownership::Mine {
        let tint = rune::oklch(if k.dark { 0.29 } else { 0.94 }, 0.045, 295.0);
        let ink = rune::oklch(if k.dark { 0.86 } else { 0.42 }, 0.12, 295.0);
        author
            .pl(2.0)
            .pr(6.0)
            .py(1.0)
            .radius(10.0)
            .fill(tint)
            .child(text("You").font(k.ty.small_medium).color(ink))
    } else {
        let author = author.child(text(handle).font(k.ty.small).color(k.c.text_muted));
        if pr.external {
            author.child(text("external").font(k.ty.small).color(k.c.text_faint))
        } else {
            author
        }
    }
}

pub(crate) fn minimal_pr_row(s: &App, item: &stacks::StackRow, k: &Kit, show_repo: bool) -> El {
    let pr = item.pr;
    let c = &k.c;
    let att = pr.attention(
        s.draft_stale_hours,
        s.open_stale_hours,
        current_epoch_hour(),
    );

    // The status, its colour, and what a tooltip adds to it.
    let (status, tone, why) = match att {
        PrAttention::IncomingAssigned => match &pr.asked {
            // A day or more: say how long it has waited, and warn once it is as stale as the
            // user lets their own pull requests get.
            Some(ask) if ask.hours_ago >= 24 => (
                format!("Waiting on you {}", model::span(ask.hours_ago)),
                if ask.hours_ago >= s.open_stale_hours {
                    c.warning.fg
                } else {
                    mint(k)
                },
                if ask.again {
                    "Asked again after your last review"
                } else {
                    "Since your review was requested"
                },
            ),
            Some(ask) if ask.again => (
                "Review requested again".to_owned(),
                mint(k),
                "You reviewed this before; the author asked for another look",
            ),
            _ => ("Review requested".to_owned(), mint(k), ""),
        },
        PrAttention::IncomingViaTeam => ("Team review requested".to_owned(), c.text_muted, ""),
        PrAttention::MyOpenNeedsNudge if pr.reviewers.is_empty() => {
            ("Needs reviewer".to_owned(), c.warning.fg, "")
        }
        PrAttention::MyOpenNeedsNudge => (
            format!("Unreviewed {}d", (pr.unreviewed_hours / 24).max(1)),
            c.warning.fg,
            "",
        ),
        PrAttention::MyDraftNeedsNudge => ("Stale draft".to_owned(), c.warning.fg, ""),
        PrAttention::MyOpenChangesRequested
            if pr.review_state != model::ReviewState::ChangesRequested =>
        {
            (
                comments_to_answer(pr.unanswered()),
                c.warning.fg,
                "Review threads still open, where the last word is not yours",
            )
        }
        PrAttention::MyOpenChangesRequested => ("Changes requested".to_owned(), c.danger.fg, ""),
        PrAttention::MyOpenBlocked => match pr.blockers().first() {
            Some(Blocker::Behind) => (
                Blocker::Behind.label().to_owned(),
                c.warning.fg,
                "The repository merges only branches that are up to date with their base",
            ),
            Some(Blocker::ChecksFailing) => (pr.failing_label(), c.danger.fg, ""),
            _ => (
                Blocker::Conflicts.label().to_owned(),
                c.danger.fg,
                "It conflicts with its base branch",
            ),
        },
        PrAttention::MyOpenApproved => (pr.approval_label(), mint(k), ""),
        PrAttention::MyOpenWaiting => match pr.reviewers.as_slice() {
            [] => ("Waiting for review".to_owned(), c.text_muted, ""),
            [one] => (format!("Waiting on {one}"), c.text_muted, ""),
            [one, rest @ ..] => (
                format!("Waiting on {one} +{}", rest.len()),
                c.text_muted,
                "",
            ),
        },
        PrAttention::MyDraftActive => ("Draft".to_owned(), c.text_muted, ""),
        PrAttention::IncomingWatched => ("Watching".to_owned(), c.text_muted, ""),
    };
    let pr_url_open = pr.url.clone();
    let id_toggle = pr.id.clone();

    let mut right_side = row().center_y().gap(2.0);
    if s.filter == InboxFilter::Inbox && s.page == Page::Inbox {
        let ignored = s.ignored.contains(&pr.id);
        let id = pr.id.clone();
        let button = header_action(
            k,
            if ignored { &EYE } else { &EYE_OFF },
            if ignored {
                "Stop ignoring"
            } else {
                "Ignore: move it to Ignored"
            },
            format!("ignore-{}", pr.id),
            270.0,
            ignored,
        )
        .on_press(move |s: &mut App| s.toggle_ignore(&id));
        // It shows while the row is hovered, and always in the Ignored section.
        right_side = right_side.child(if ignored {
            button
        } else {
            button.group_opacity(0.0, 1.0)
        });
    }
    if let Some(pill) = pace_pill(s, k, pr) {
        right_side = right_side.child(pill);
    }
    if matches!(
        att,
        PrAttention::IncomingAssigned | PrAttention::IncomingViaTeam
    ) && !pr.is_draft
        && s.page == Page::Inbox
    {
        right_side = right_side.child(approve_control(s, k, pr));
    }
    if pr.ownership == Ownership::Mine && !pr.is_draft && s.page == Page::Inbox {
        let unblock = unblock_control(s, k, pr, att);
        // In Unblock the status says why it cannot merge; a Merge button that can do nothing
        // only says it again.
        let merge_is_dead =
            !pr.merge_readiness.is_ready() && s.auto_merge_target(&pr.id).is_none();
        let shows_merge = !(merge_is_dead && att == PrAttention::MyOpenBlocked);
        if let Some((button, _)) = unblock {
            right_side = right_side.child(button);
        }
        if shows_merge {
            right_side = right_side.child(merge_control(s, k, pr));
        }
    }
    let celebrating = s
        .celebrations
        .iter()
        .find(|(id, _)| *id == pr.id)
        .map(|(_, at)| at.elapsed().as_secs_f32());

    let mut metadata = row().center_y().gap(9.0);
    if show_repo {
        metadata = metadata.child(text(pr.repo.clone()).font(k.ty.small).color(c.text_muted));
    }
    metadata = metadata
        .child(
            text(format!("#{}", pr.number))
                .font(k.ty.mono)
                .color(c.text_muted),
        )
        .child(author_badge(s, pr, k))
        .child(
            text(pr.age_label())
                .font(k.ty.small)
                .color(c.text_muted)
                .help(pr.age_detail()),
        )
        .child(el().size(1.0, 10.0).fill(c.border_base))
        .child({
            let label = text(status.clone()).font(k.ty.small_medium).color(tone);
            if why.is_empty() { label } else { label.help(why) }
        });
    // A brain marks what the model judged; a rule's verdict needs none. In the Inbox the section
    // already says what it means, so the questions that came out yes are named only elsewhere.
    if s.smart_active()
        && let Some(judgment) = s.judgment(pr).filter(|j| j.by != "rules")
    {
        let fired = s.layout.fired(&judgment.answers);
        let in_model_section = s
            .action_group_of(pr, att)
            .and_then(|(id, _)| s.layout.sections.get(usize::from(id)))
            .is_some_and(sections::Section::uses_model);
        if !fired.is_empty() && (in_model_section || s.filter != InboxFilter::Inbox) {
            let mut mark = row()
                .center_y()
                .gap(4.0)
                .help(judgment.summary())
                .child(rune::icon(&BRAIN, 12.0).color(model_ink(k)));
            if s.filter != InboxFilter::Inbox {
                mark = mark.child(
                    text(fired.join(", ").replace(['-', '_'], " "))
                        .font(k.ty.small)
                        .color(model_ink(k)),
                );
            }
            metadata = metadata.child(mark);
        }
    }
    if let Some(parent) = item.parent {
        metadata = metadata.child(
            text(format!("↳ #{parent}"))
                .font(k.ty.small)
                .color(c.text_muted)
                .help(format!("Stacked on #{parent}")),
        );
    }
    for (label, tone, why) in blocker_chips(pr, &status, k) {
        let chip = text(label).font(k.ty.small_medium).color(tone);
        metadata = metadata.child(if why.is_empty() { chip } else { chip.help(why) });
    }
    let mut line = row()
        .key(format!("pr-row-{}", pr.id))
        .w_full()
        .pr(14.0)
        .pl(14.0 + item.depth.min(5) as f32 * 18.0)
        .py(10.0)
        .center_y()
        .gap(12.0)
        .hover_group()
        .hover(c.hover)
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut App| s.toggle_expanded(&id_toggle))
        .child(
            rune::icon(
                if item.depth == 0 {
                    &PR_ICON
                } else {
                    &STACK_BRANCH
                },
                16.0,
            )
            .color(if pr.is_draft {
                c.text_faint
            } else {
                c.text_muted
            }),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    // Only the title goes to GitHub; the rest of the row opens the description.
                    text(pr.title.clone())
                        .font(TextStyle::new(14.0, 530.0))
                        .color(c.text_base)
                        .truncate()
                        .link(
                            rune::Link::new(0..pr.title.len(), "open")
                                .hover(rune::LinkPaint::new().underline(c.text_base))
                                .help("Open on GitHub"),
                        )
                        .on_link(move |s: &mut App, _: &str, _: &mut Tx| {
                            notify::open_url_in_browser(&pr_url_open, s.allow_os_side_effects);
                        }),
                )
                .child(metadata),
        )
        .child(right_side);
    let approved_at = s
        .approved
        .iter()
        .find(|(id, _)| *id == pr.id)
        .map(|(_, at)| (mint(k), at.elapsed().as_secs_f32()));
    if let Some((tone, t)) = celebrating.map(|t| (merge_tone(k), t)).or(approved_at) {
        // The button's colour washes over the row once and ebbs away.
        line = line.fill(tone.alpha((0.16 * (1.0 - t / INK_WASH_SECS)).max(0.0)));
    }
    if let Some(age) = s.arrival_age(&pr.id) {
        // New here: it fades in and settles down a few pixels, then a mint tint ebbs away.
        let slide = (age / ARRIVAL_SLIDE_SECS).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - slide).powi(3);
        let glow = (1.0 - age / ARRIVAL_GLOW_SECS).clamp(0.0, 1.0);
        line = line
            .opacity(ease)
            .offset(0.0, (1.0 - ease) * -10.0)
            .fill(mint(k).alpha(0.16 * glow * glow));
    }
    if s.expanded.contains(&pr.id) {
        let indent = 14.0 + 28.0 + item.depth.min(5) as f32 * 18.0;
        return col()
            .w_full()
            .child(line)
            .child(description(s, k, pr, indent));
    }
    line
}

pub(crate) fn comments_to_answer(n: u32) -> String {
    if n == 1 {
        "1 comment to answer".to_owned()
    } else {
        format!("{n} comments to answer")
    }
}

/// What the row says after its status about the way to the merge: each thing in it, its colour
/// and a tooltip. On someone else's pull request these are the author's to clear: they are said
/// quietly, and the tooltip says whose move it is.
pub(crate) fn blocker_chips(pr: &PullRequest, status: &str, k: &Kit) -> Vec<(String, Color, String)> {
    let c = &k.c;
    let mine = pr.ownership == Ownership::Mine;
    let mut chips = Vec::new();
    for blocker in pr.blockers() {
        let (tone, why) = match blocker {
            Blocker::Conflicts if mine => (c.danger.fg, "It conflicts with its base branch"),
            Blocker::Conflicts => (c.text_muted, "The author has conflicts to resolve first"),
            Blocker::Behind if mine => (
                c.warning.fg,
                "The repository merges only branches that are up to date with their base",
            ),
            Blocker::Behind => (c.text_muted, "The author has to update the branch first"),
            // Named below, whether or not GitHub holds the merge for it.
            Blocker::ChecksFailing => continue,
        };
        chips.push((blocker.label().to_owned(), tone, why.to_owned()));
    }
    if pr.ci_status == CiStatus::Failing {
        let names: Vec<&str> = pr.failing_checks.iter().map(|c| c.name.as_str()).collect();
        let why = match (names.is_empty(), mine) {
            (true, true) => String::new(),
            (true, false) => "The author has failing checks to fix first".to_owned(),
            (false, _) => format!("Failed: {}", names.join(", ")),
        };
        let tone = if mine { c.danger.fg } else { c.text_muted };
        chips.push(("CI failing".to_owned(), tone, why));
    }
    if mine && pr.unanswered() > 0 {
        chips.push((
            comments_to_answer(pr.unanswered()),
            c.warning.fg,
            "Review threads still open, where the last word is not yours".to_owned(),
        ));
    }
    // The status already says the first of these.
    chips.retain(|(label, _, _)| !status.starts_with(label.as_str()));
    chips
}

/// The button for what holds up one of the user's pull requests, and whether it shows at rest.
/// Conflicts and failed checks take code to clear, so their button copies a prompt for a coding
/// agent; a branch that is only behind is updated from here. It shows at rest when it answers the
/// row's status, and waits for the pointer anywhere else.
pub(crate) fn unblock_control(s: &App, k: &Kit, pr: &PullRequest, att: PrAttention) -> Option<(El, bool)> {
    let blockers = pr.blockers();
    let first = (att == PrAttention::MyOpenBlocked)
        .then(|| blockers.first().copied())
        .flatten();
    let busy = s.unblocking.contains(&pr.id);
    let key = format!("unblock-{}", pr.id);
    let tone = unblock_tone(k);
    let id = pr.id.clone();
    // While GitHub holds the merge for a branch that is behind, updating it comes first: the
    // checks run again on the result.
    let update_first = blockers.contains(&Blocker::Behind) && pr.can_update_branch;
    let prompt = if update_first { None } else { pr.agent_prompt() };
    // The agent pushed a fix and nothing holds the pull request up now: say so where Fix was.
    if prompt.is_none()
        && s.agent_offered()
        && let Some((_, at)) = s.agent_fixed.iter().find(|(id, _)| *id == pr.id)
    {
        let (glyph, tone) = match s.agent.provider {
            agent::Provider::Claude => (&CLAUDE, oklch(if k.dark { 0.74 } else { 0.62 }, 0.13, 45.0)),
            agent::Provider::Codex => (&CODEX, tone),
        };
        let button = ink_button(k, tone, glyph, "Fixed", Ink::Done(at.elapsed().as_secs_f32()), key);
        return Some((
            row().center_y().help(format!("{} pushed a fix", s.agent.provider.label())).child(button),
            true,
        ));
    }
    let fix = if prompt.is_some() && s.agent_offered() {
        Some(s.agent.provider)
    } else {
        None
    };
    let (button, why, at_rest) = if let Some(provider) = fix {
        let (glyph, tone) = match provider {
            agent::Provider::Claude => (&CLAUDE, oklch(if k.dark { 0.74 } else { 0.62 }, 0.13, 45.0)),
            agent::Provider::Codex => (&CODEX, tone),
        };
        let running = s.agent_running.as_deref();
        let (label, ink) = if running == Some(pr.id.as_str()) {
            ("Fixing…", Ink::Busy)
        } else if running.is_some() {
            ("Fix", Ink::Disabled)
        } else {
            ("Fix", Ink::Rest)
        };
        let why = format!(
            "Run {} on this pull request in the background and say whether it pushed a fix",
            provider.label()
        );
        (
            ink_button(k, tone, glyph, label, ink, key)
                .on_press(move |s: &mut App| s.press_fix(&id)),
            why,
            first.is_some_and(|blocker| blocker != Blocker::Behind)
                || running == Some(pr.id.as_str()),
        )
    } else if let Some(prompt) = prompt {
        let copied = s
            .copied
            .as_ref()
            .filter(|(copied, _)| *copied == pr.id)
            .map(|(_, at)| at.elapsed().as_secs_f32());
        let button = match copied {
            Some(age) => ink_button(
                k,
                tone,
                &rune_kit::glyphs::CHECK,
                "Copied",
                Ink::Done(age),
                key,
            ),
            None => ink_button(k, tone, &COPY, "Copy prompt", Ink::Rest, key).on_press_tx(
                move |s: &mut App, tx: &mut Tx| {
                    tx.copy(prompt.clone());
                    s.copied = Some((id.clone(), std::time::Instant::now()));
                },
            ),
        };
        let conflicts = blockers.contains(&Blocker::Conflicts);
        let why = match (conflicts, pr.ci_status == CiStatus::Failing) {
            (true, true) => {
                "Copy a prompt for your coding agent: resolve the conflicts and fix the failed checks"
            }
            (true, false) => "Copy a prompt for your coding agent: resolve the conflicts",
            _ => "Copy a prompt for your coding agent: fix the failed checks",
        }
        .to_owned();
        (
            button,
            why,
            first.is_some_and(|blocker| blocker != Blocker::Behind) || copied.is_some(),
        )
    } else if pr.can_update_branch {
        let (label, ink) = if busy {
            ("Updating…", Ink::Busy)
        } else {
            ("Update branch", Ink::Rest)
        };
        (
            ink_button(k, tone, &UPDATE, label, ink, key)
                .on_press(move |s: &mut App| s.press_unblock(&id, Unblock::UpdateBranch)),
            "Merge the base branch into this one, as Update branch does on GitHub".to_owned(),
            first == Some(Blocker::Behind),
        )
    } else {
        return None;
    };
    let wrap = row().center_y().hover_group().help(why).child(button);
    Some(if at_rest || busy {
        (wrap, true)
    } else {
        (wrap.group_opacity(0.0, 1.0), false)
    })
}

/// The PR description as Markdown, under its row.
pub(crate) fn description(s: &App, k: &Kit, pr: &PullRequest, indent: f32) -> El {
    let c = &k.c;
    let source = clean_description(&pr.description);
    let body = if source.is_empty() {
        text("No description.").font(k.ty.small).color(c.text_faint)
    } else {
        let _ = s;
        let style = MarkdownStyle::from_kit(k)
            .body_font(TextStyle::new(12.5, 400.0))
            .heading_font(1, TextStyle::new(14.0, 650.0))
            .heading_font(2, TextStyle::new(13.5, 650.0))
            .heading_font(3, TextStyle::new(13.0, 650.0))
            .code_block_font(TextStyle::new(12.0, 400.0))
            .list(16.0, 3.0);
        Markdown::new(source)
            .style(style)
            .on_link(|s: &mut App, url: &str| {
                notify::open_url_in_browser(url, s.allow_os_side_effects)
            })
            .build(k)
    };
    col()
        .key(format!("pr-description-{}", pr.id))
        .w_full()
        .pl(indent)
        .pr(14.0)
        .pb(12.0)
        .child(
            col()
                .w_full()
                .fill(c.layer_02)
                .border(1.0, c.border_base)
                .radius(8.0)
                .px(12.0)
                .py(10.0)
                .child(
                    col()
                        .w_full()
                        .max_h(300.0)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(body),
                ),
        )
}

/// Drops the HTML comments PR templates leave in, and keeps a very long description short.
pub(crate) fn clean_description(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    let trimmed = out.trim();
    match trimmed.char_indices().nth(8000) {
        Some((at, _)) => format!("{}…", &trimmed[..at]),
        None => trimmed.to_owned(),
    }
}

pub(crate) fn approve_control(s: &App, k: &Kit, pr: &PullRequest) -> El {
    let id = pr.id.clone();
    let done = s
        .approved
        .iter()
        .find(|(a, _)| *a == pr.id)
        .map(|(_, at)| at.elapsed().as_secs_f32());
    let busy = s.approving.contains(&pr.id);
    let (label, ink) = if let Some(age) = done {
        ("Approved", Ink::Done(age))
    } else if busy {
        ("Approving…", Ink::Busy)
    } else {
        ("Approve", Ink::Rest)
    };
    let mut button = ink_button(
        k,
        mint(k),
        &rune_kit::glyphs::CHECK,
        label,
        ink,
        format!("approve-{}", pr.id),
    );
    if done.is_none() && !busy {
        button = button.on_press(move |s: &mut App| s.press_approve(&id));
    }
    let wrap = row()
        .center_y()
        .hover_group()
        .help("Approve without a comment")
        .child(button);
    // It shows while the row is hovered, and stays once it is busy or done.
    if busy || done.is_some() {
        wrap
    } else {
        wrap.group_opacity(0.0, 1.0)
    }
}

/// A pull request GitHub merged by itself, kept faded at the bottom of its section until cleared.
pub(crate) fn finished_row(k: &Kit, t: &model::Tracked) -> El {
    let c = &k.c;
    let pr = &t.pr;
    let (label, color) = match t.state {
        model::TrackedState::Merged => ("Auto-merged ✓", mint(k)),
        _ => ("Closed without merging", c.text_muted),
    };
    let id = pr.id.clone();
    row()
        .key(format!("finished-{}", pr.id))
        .w_full()
        .pl(14.0)
        .pr(14.0)
        .py(10.0)
        .center_y()
        .gap(12.0)
        .opacity(0.62)
        .child(rune::icon(&PR_ICON, 16.0).color(c.text_faint))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    text(pr.title.clone())
                        .font(TextStyle::new(14.0, 530.0))
                        .color(c.text_muted)
                        .truncate(),
                )
                .child(
                    row()
                        .center_y()
                        .gap(9.0)
                        .child(text(pr.repo.clone()).font(k.ty.small).color(c.text_muted))
                        .child(
                            text(format!("#{}", pr.number))
                                .font(k.ty.mono)
                                .color(c.text_muted),
                        )
                        .child(el().size(1.0, 10.0).fill(c.border_base))
                        .child(text(label).font(k.ty.small_medium).color(color)),
                ),
        )
        .child(
            icon_action(k.glyph.close, "Clear")
                .on_press(move |s: &mut App| s.clear_tracked(&id))
                .build(k)
                .key(format!("clear-{}", pr.id)),
        )
}

/// The Merge button on one of the user's PRs: off until GitHub says it can merge, armed by the
/// first press, and done once the merge lands, while the row leaves.
pub(crate) fn merge_control(s: &App, k: &Kit, pr: &PullRequest) -> El {
    let tone = merge_tone(k);
    let key = format!("merge-{}", pr.id);
    if let Some((_, at)) = s.celebrations.iter().find(|(id, _)| *id == pr.id) {
        let age = at.elapsed().as_secs_f32();
        return ink_button(k, tone, &rune_kit::glyphs::CHECK, "Merged", Ink::Done(age), key);
    }
    let id = pr.id.clone();
    let ready = pr.merge_readiness.is_ready();
    let merging = s.merging.contains(&pr.id);
    let armed = armed_left(&s.merge_armed, &pr.id);
    let busy = !s.merging.is_empty();
    // Not mergeable yet: GitHub can merge it by itself once it is, which is its own state.
    let auto_on = pr.auto_merge == model::AutoMerge::On;
    let auto_offer = !ready && s.auto_merge_target(&pr.id) == Some(true);
    let auto = auto_on || auto_offer;
    let auto_busy = s.is_auto_merging(&pr.id);

    // A stacked pull request merges into its base branch, not the default one: say which.
    let merge_label = if pr.stack_base.is_some() { "Merge to base" } else { "Merge" };
    let (glyph, label, ink) = if merging {
        (&MERGE, "Merging…", Ink::Busy)
    } else if let Some(left) = armed {
        (&MERGE, "Press again", Ink::Armed(left))
    } else if auto_on {
        (&BOLT, "Auto-merge on", if auto_busy { Ink::Busy } else { Ink::On })
    } else if auto_offer {
        (&BOLT, "Auto-merge", if auto_busy { Ink::Busy } else { Ink::Rest })
    } else if !ready || busy {
        (&MERGE, merge_label, Ink::Disabled)
    } else {
        (&MERGE, merge_label, Ink::Rest)
    };
    let mut button = ink_button(k, tone, glyph, label, ink, key);
    if matches!(ink, Ink::Rest | Ink::Armed(_) | Ink::On) {
        button = button.on_press(move |s: &mut App| {
            if auto {
                s.press_auto_merge(&id)
            } else {
                s.press_merge(&id)
            }
        });
    }

    row()
        .center_y()
        .hover_group()
        .help(if auto_on {
            "Merges by itself as soon as GitHub allows it. Click to turn off".to_owned()
        } else if auto_offer {
            format!(
                "Merge by itself once GitHub allows it ({}). Click to turn on",
                pr.merge_readiness.blocked_reason().to_lowercase()
            )
        } else if armed.is_some() {
            format!(
                "Press again to {} merge {}#{}",
                pr.merge_method.label(),
                pr.repo,
                pr.number
            )
        } else if ready && pr.merge_readiness == model::MergeReadiness::Unstable {
            format!(
                "Merge with {}. Some checks haven't passed, but none is required",
                pr.merge_method.label()
            )
        } else if ready {
            match pr.stack_base.as_deref() {
                Some(base) => format!(
                    "Merge with {} into {base}, not the default branch",
                    pr.merge_method.label()
                ),
                None => format!("Merge with {}", pr.merge_method.label()),
            }
        } else {
            pr.merge_readiness.blocked_reason().to_owned()
        })
        .child(button)
}

/// One light icon button on a repository header, tinted `hue` (oklch degrees).
pub(crate) fn header_action(
    k: &Kit,
    glyph: &'static rune::Icon,
    label: &str,
    key: String,
    hue: f32,
    active: bool,
) -> El {
    let (l, chroma) = match (k.dark, active) {
        (false, false) => (0.66, 0.06),
        (false, true) => (0.5, 0.14),
        (true, false) => (0.62, 0.06),
        (true, true) => (0.8, 0.12),
    };
    row()
        .key(key)
        .role("button")
        .label(label.to_owned())
        .help(label.to_owned())
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .size(24.0, 24.0)
        .radius(5.0)
        .center()
        .hover(k.c.layer_02)
        .child(
            rune::icon(glyph, 15.0)
                .color(oklch(l, chroma, hue))
                .a11y_hidden(),
        )
}

/// Hover actions on a repository header. Idle buttons show while the header is hovered; one
/// that is switched on stays visible.
pub(crate) fn repository_actions(s: &App, k: &Kit, repo: &RepoConfig) -> El {
    let (pin, remove) = (repo.name.clone(), repo.name.clone());
    let confirming = s.confirm_remove.as_deref() == Some(repo.name.as_str());
    let reveal = |el: El, on: bool| if on { el } else { el.group_opacity(0.0, 1.0) };

    row()
        .center_y()
        .gap(8.0)
        .child(reveal(
            header_action(
                k,
                &PIN,
                if repo.pinned {
                    "Unpin repository"
                } else {
                    "Pin repository"
                },
                format!("repo-pin-{}", repo.name),
                70.0,
                repo.pinned,
            )
            .on_press(move |s: &mut App| s.toggle_pin_repo(&pin)),
            repo.pinned,
        ))
        .child(reveal(
            header_action(
                k,
                &TRASH,
                "Remove repository",
                format!("repo-remove-{}", repo.name),
                25.0,
                confirming,
            )
            .on_press(move |s: &mut App| s.confirm_remove = Some(remove.clone())),
            confirming,
        ))
}

pub(crate) fn document_inbox_view(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let ordered = s.ordered_repos();

    let mut doc = col()
        .key("document-column")
        .w_full()
        .max_w(960.0)
        .py(24.0)
        .gap(16.0);
    let now = current_epoch_hour();
    let attention = s.prs.iter().filter(|p| s.needs_you(p, now)).count();
    doc = doc
        .child(row().center_y().gap(12.0).child(brand(s, k)).children(update_button(s, k)))
        .child(
            row()
                .w_full()
                .justify_between()
                .items_end()
                .child(col().gap(5.0).child(heading(k, "Pull Requests")).child(
                    if s.prs.is_empty() {
                        el()
                    } else {
                        text(format!(
                            "{} open  ·  {} need your thumb",
                            s.prs.len(),
                            attention
                        ))
                        .font(k.ty.body)
                        .color(c.text_muted)
                    },
                ))
                .child(
                    icon_action(&PLUS, "Add repository")
                        .on_press(|s: &mut App| {
                            s.add_repo_open = true;
                            if s.discovery.is_none() {
                                s.trigger_discovery();
                            }
                        })
                        .build(k)
                        .key("add-repo-open"),
                ),
        )
        .children((s.pace_enabled && s.page == Page::Inbox).then(|| pace_panel(s, k)))
        .child(inbox_tools(s, k));
    if let Some(msg) = &s.status_banner {
        doc = doc.child(
            row()
                .key("toast-status-banner")
                .w_full()
                .px(12.0)
                .py(6.0)
                .radius(6.0)
                .fill(c.layer_01)
                .border(1.0, c.border_weak)
                .center_y()
                .justify_between()
                .child(text(msg.clone()).font(k.ty.small).color(c.text_muted))
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut App| s.status_banner = None)
                        .build(k),
                ),
        );
    }

    if let Some(name) = s.confirm_remove.clone().filter(|_| s.page == Page::Inbox) {
        let confirmed = name.clone();
        doc = doc.child(
            row()
                .key("confirm-remove-repo")
                .w_full()
                .px(12.0)
                .py(6.0)
                .radius(6.0)
                .fill(c.layer_01)
                .border(1.0, c.border_weak)
                .center_y()
                .justify_between()
                .child(
                    text(format!("Remove {name} from your list?"))
                        .font(k.ty.small)
                        .color(c.text_base),
                )
                .child(
                    row()
                        .center_y()
                        .gap(4.0)
                        .child(
                            Button::new("Cancel")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Small)
                                .on_press(|s: &mut App| s.confirm_remove = None)
                                .build(k),
                        )
                        .child(
                            Button::new("Remove")
                                .variant(ButtonVariant::Danger)
                                .control_size(ControlSize::Small)
                                .on_press(move |s: &mut App| s.remove_repo(&confirmed))
                                .build(k)
                                .key("confirm-remove-repo-btn"),
                        ),
                ),
        );
    }

    if s.page == Page::Inbox
        && let SyncStatus::NeedsAuth { reason } = &s.sync_status
    {
        doc = doc.child(setup_row(
            k,
            &format!("GitHub sync unavailable. {reason}"),
            Button::new("Check GitHub")
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut App| {
                    s.page = Page::Onboarding;
                    s.status_banner = None;
                    s.trigger_sync();
                })
                .build(k),
        ));
    }

    let flat = s.filter == InboxFilter::Inbox;
    let groups: Vec<_> = ordered
        .iter()
        .enumerate()
        .filter(|(_, repo)| {
            !flat
                && s.selected_repo
                    .as_ref()
                    .is_none_or(|name| name == &repo.name)
        })
        .filter_map(|(idx, repo)| {
            let prs = s.prs_for_repo(&repo.name);
            // Repos the user is engaged in stay listed even when nothing is open right now.
            let keep_empty = (repo.pinned || repo.activity > 0)
                && s.filter == InboxFilter::All
                && s.search_query.text.trim().is_empty();
            (!prs.is_empty() || keep_empty).then_some((idx, repo, prs))
        })
        .collect();
    let order: Vec<String> = groups.iter().map(|(_, r, _)| r.name.clone()).collect();
    let group_count = groups.len();
    // While a section is dragged every section folds to its header, so all of them share a height.
    let dragging = s.repo_reorder.dragging().is_some();
    let pinned_shown = groups.iter().filter(|(_, r, _)| r.pinned).count();
    for (position, (_, repo, prs)) in groups.iter().enumerate() {
        if position == 0 && pinned_shown > 0 && !dragging {
            doc = doc.child(text("Pinned").font(k.ty.small).color(c.text_faint));
        }
        if position == pinned_shown && pinned_shown > 0 && !dragging {
            doc = doc.child(
                text("Ranked automatically")
                    .font(k.ty.small)
                    .color(c.text_faint),
            );
        }
        let order = order.clone();
        let lifted = s.repo_reorder.dragging() == Some(position);
        let folded = repo.collapsed || dragging;
        let grip = row()
            .key(format!("repo-grip-{}", repo.name))
            .role("button")
            .label(format!("Drag to reorder {}", repo.name))
            .size(20.0, 20.0)
            .center()
            .cursor(if lifted {
                rune::Cursor::Grabbing
            } else {
                rune::Cursor::Grab
            })
            .on_drag(
                move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                    rune::Phase::Down => {}
                    rune::Phase::Move => {
                        tx.animated();
                        s.repo_reorder.follow(
                            position,
                            group_count,
                            d.dy,
                            SECTION_HEIGHT + SECTION_GAP,
                        );
                    }
                    rune::Phase::Up => {
                        if let Some((from, to)) = s.repo_reorder.end(d.cancelled()) {
                            s.drop_repo(&order, from, to);
                        }
                    }
                },
            )
            .child(rune::icon(&GRIP, 14.0).color(c.text_faint).a11y_hidden());
        let repo_name_collapse = repo.name.clone();
        let repo_url = format!("https://github.com/{}", repo.name);

        let section_header = col()
            .key(format!("repo-header-{}", repo.name))
            .w_full()
            .child(
                row()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .px(14.0)
                    .fill(c.layer_01)
                    .hover_group()
                    .center_y()
                    .justify_between()
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        tx.motion_curve(rune::Curve::tween(
                            0.12,
                            rune::Easing::cubic(0.2, 0.0, 0.0, 1.0),
                        ));
                        s.toggle_collapse_repo(&repo_name_collapse)
                    })
                    .child(
                        row()
                            .center_y()
                            .gap(8.0)
                            .child(grip)
                            .child(rune::icon(&REPO_ICON, 15.0).color(c.text_muted))
                            .child(
                                row()
                                    .center_y()
                                    .cursor(rune::Cursor::Pointer)
                                    .help("Open on GitHub")
                                    .on_press(move |s: &mut App| {
                                        notify::open_url_in_browser(
                                            &repo_url,
                                            s.allow_os_side_effects,
                                        )
                                    })
                                    .child(
                                        text(repo.name.clone())
                                            .font(TextStyle::new(13.0, 650.0))
                                            .color(c.text_muted),
                                    ),
                            )
                            .child(if folded {
                                text(model::summarize_prs(prs))
                                    .font(k.ty.small)
                                    .color(c.text_faint)
                            } else {
                                el()
                            }),
                    )
                    .child(if s.page == Page::Inbox {
                        repository_actions(s, k, repo)
                    } else {
                        el()
                    }),
            )
            .child(el().w_full().h(1.0).fill(c.border_weak));

        let mut list = col()
            .w_full()
            .radius(9.0)
            .clip_rounded(9.0)
            .border(1.0, c.border_weak)
            .fill(surface(k))
            .offset(0.0, s.repo_reorder.shift(position))
            .child(section_header);
        if lifted {
            list = list.raise().offset_motion(rune::Anim::INSTANT);
        }
        if folded {
            doc = doc.child(list);
            continue;
        }
        if prs.is_empty() {
            list = list.child(
                text("No open pull requests")
                    .font(k.ty.small)
                    .color(c.text_muted)
                    .padding(12.0),
            );
        }
        for (idx, item) in stacks::rows(prs, &s.prs).iter().enumerate() {
            if idx > 0 {
                list = list.child(el().w_full().h(1.0).fill(c.border_weak));
            }
            list = list.child(minimal_pr_row(s, item, k, false));
        }

        doc = doc.child(list);
    }

    if flat {
        doc = doc.child(flat_inbox(s, k));
    } else if groups.is_empty() {
        doc = doc.child(
            col()
                .key("empty-inbox-state")
                .w_full()
                .py(48.0)
                .center()
                .gap(6.0)
                .child(text("Nothing here.").font(k.ty.body).color(c.text_muted))
                .child(
                    text("Switch filters or clear your search.")
                        .font(k.ty.small)
                        .color(c.text_faint),
                ),
        );
    }

    col()
        .key("inbox-viewport")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(
            // The scroll sits on a column: a row clips its children to its own height.
            col()
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0)
                .min_h(0.0)
                .child(row().w_full().px(28.0).justify_center().child(doc)),
        )
}

/// How many PRs the Inbox tab shows before "Show more".
pub(crate) const INBOX_LIMIT: usize = 100;

/// The Inbox tab: one list of the PRs that matter most right now, ranked by the app.
pub(crate) fn flat_inbox(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let now = current_epoch_hour();
    let mut items: Vec<&PullRequest> = s.prs.iter().filter(|pr| s.in_inbox(pr, now)).collect();
    let group = |pr: &PullRequest| {
        s.action_group_of(
            pr,
            pr.attention(s.draft_stale_hours, s.open_stale_hours, now),
        )
    };
    // A pull request no section claims is not listed.
    items.retain(|pr| group(pr).is_some());
    // Ones auto-merge finished stay at the bottom of their section, faded, until cleared.
    let finished: Vec<&model::Tracked> = s
        .tracked
        .iter()
        .filter(|t| t.state != model::TrackedState::Watching && s.matches_search(&t.pr))
        .collect();
    items.sort_by(|a, b| {
        let rank = |pr: &PullRequest| group(pr).map_or(usize::MAX, |(id, _)| s.group_rank(id));
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.external.cmp(&b.external))
            .then_with(|| a.idle_hours.cmp(&b.idle_hours))
            .then_with(|| a.id.cmp(&b.id))
    });

    if items.is_empty() && finished.is_empty() {
        return col()
            .key("empty-inbox-state")
            .w_full()
            .py(48.0)
            .center()
            .gap(6.0)
            .child(match &s.logo {
                Some(image) => el().size(72.0, 72.0).pixels(image.pixels()),
                None => el(),
            })
            .child(
                text("Nothing to approve. For now.")
                    .font(k.ty.body)
                    .color(c.text_muted),
            )
            .child(
                text("Mine, Theirs and All show everything else.")
                    .font(k.ty.small)
                    .color(c.text_faint),
            );
    }
    let total = items.len();
    let shown = if s.inbox_show_all {
        total
    } else {
        total.min(INBOX_LIMIT)
    };
    // One section per kind of action, like a repository section: drag to reorder, click to fold.
    let mut section = col().key("flat-inbox").w_full().gap(SECTION_GAP);
    let shown_items = &items[..shown];
    let mut displayed: Vec<(u8, &str)> = Vec::new();
    for pr in shown_items {
        if let Some(g) = group(pr)
            && !displayed.iter().any(|(p, _)| *p == g.0)
        {
            displayed.push(g);
        }
    }
    for t in &finished {
        if let Some(at) = s
            .layout
            .sections
            .iter()
            .position(|sec| sec.name == t.section)
            && !displayed.iter().any(|(p, _)| usize::from(*p) == at)
        {
            displayed.push((at as u8, s.layout.sections[at].name.as_str()));
        }
    }
    displayed.sort_by_key(|(p, _)| s.group_rank(*p));
    // Ignored sits at the bottom and is not dragged, so it is left out of the drag order.
    let order: Vec<u8> = displayed
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| *p != IGNORED_GROUP)
        .collect();
    let group_count = order.len();
    let dragging = s.group_reorder.dragging().is_some();
    for (index, (position, heading)) in displayed.iter().copied().enumerate() {
        let in_group = items
            .iter()
            .filter(|pr| group(pr).is_some_and(|(id, _)| id == position))
            .count();
        let lifted = s.group_reorder.dragging() == Some(index);
        let is_ignored = position == IGNORED_GROUP;
        let folded = if is_ignored {
            !s.ignored_open
        } else {
            dragging || s.collapsed_groups.contains(&position)
        };
        let drop_order = order.clone();
        let grip = row()
            .key(format!("group-grip-{position}"))
            .role("button")
            .label(format!("Drag to reorder {heading}"))
            .size(20.0, 20.0)
            .center()
            .cursor(if lifted {
                rune::Cursor::Grabbing
            } else {
                rune::Cursor::Grab
            })
            .on_drag(
                move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                    rune::Phase::Down => {}
                    rune::Phase::Move => {
                        tx.animated();
                        s.group_reorder.follow(
                            index,
                            group_count,
                            d.dy,
                            SECTION_HEIGHT + SECTION_GAP,
                        );
                    }
                    rune::Phase::Up => {
                        if let Some((from, to)) = s.group_reorder.end(d.cancelled()) {
                            s.drop_group(&drop_order, from, to);
                        }
                    }
                },
            )
            .child(rune::icon(&GRIP, 14.0).color(c.text_faint).a11y_hidden());
        let header = col()
            .key(format!("inbox-group-{position}"))
            .w_full()
            .child(
                row()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .px(14.0)
                    .fill(c.layer_01)
                    .center_y()
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        tx.motion_curve(rune::Curve::tween(
                            0.12,
                            rune::Easing::cubic(0.2, 0.0, 0.0, 1.0),
                        ));
                        if position == IGNORED_GROUP {
                            s.ignored_open = !s.ignored_open;
                        } else {
                            s.toggle_collapse_group(position);
                        }
                    })
                    .child(
                        row()
                            .center_y()
                            .gap(8.0)
                            .child(if is_ignored { el().size(20.0, 20.0) } else { grip })
                            .child(
                                text(heading)
                                    .font(TextStyle::new(13.0, 650.0))
                                    .color(c.text_muted),
                            )
                            .child(if in_group > 0 {
                                text(in_group.to_string())
                                    .font(k.ty.small)
                                    .color(c.text_faint)
                            } else {
                                el()
                            })
                            .child(if s.smart_active()
                                && s.layout.sections.get(usize::from(position)).is_some_and(sections::Section::uses_model)
                            {
                                rune::icon(&BRAIN, 13.0)
                                    .color(model_ink(k))
                                    .help("Sorted by alien intelligence: a model read each title and description")
                            } else {
                                el()
                            }),
                    ),
            )
            .child(el().w_full().h(1.0).fill(c.border_weak));
        let mut list = col()
            .w_full()
            .radius(9.0)
            .clip_rounded(9.0)
            .border(1.0, c.border_weak)
            .fill(surface(k))
            .offset(0.0, s.group_reorder.shift(index))
            .child(header);
        if lifted {
            list = list.raise().offset_motion(rune::Anim::INSTANT);
        }
        if !folded {
            for (idx, pr) in shown_items
                .iter()
                .filter(|pr| group(pr).is_some_and(|(id, _)| id == position))
                .enumerate()
            {
                if idx > 0 {
                    list = list.child(el().w_full().h(1.0).fill(c.border_weak));
                }
                let item = stacks::StackRow {
                    pr,
                    depth: 0,
                    parent: None,
                };
                list = list.child(minimal_pr_row(s, &item, k, true));
            }
            let mut first = !items
                .iter()
                .any(|pr| group(pr).is_some_and(|(id, _)| id == position));
            for t in finished.iter().filter(|t| t.section == heading) {
                if !first {
                    list = list.child(el().w_full().h(1.0).fill(c.border_weak));
                }
                first = false;
                list = list.child(finished_row(k, t));
            }
        }
        section = section.child(list);
    }
    if total > INBOX_LIMIT {
        section = section.child(
            row().child(
                Button::new(if s.inbox_show_all {
                    "Show less".to_owned()
                } else {
                    format!("Show {} more", total - shown)
                })
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| s.inbox_show_all = !s.inbox_show_all)
                .build(k)
                .key("inbox-show-more"),
            ),
        );
    }
    section
}

pub(crate) fn view(s: &App, cx: &mut Cx<App>) -> El {
    let mut k = Kit::neutral(cx.system_dark());
    k.c.bg_base = oklch(if k.dark { 0.15 } else { 0.985 }, 0.003, 175.0);
    // The off state of a switch is this color; the stock one vanishes on a white panel.
    k.c.layer_04 = oklch(if k.dark { 0.36 } else { 0.86 }, 0.004, 175.0);

    let intro = s
        .intro
        .filter(|_| s.page == Page::Onboarding && !cx.is_reduced_motion())
        .map(|at| at.elapsed().as_secs_f32());
    let body = match s.page {
        Page::Onboarding => onboarding_page_view(s, &k, intro),
        Page::Inbox => document_inbox_view(s, &k),
    };

    let root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(top_bar(s, &k, intro.is_some()))
        .child(body.grow(1.0).min_h(0.0));
    let root = match intro {
        Some(t) => {
            let window = cx
                .layout_size("root")
                .map_or((920.0, 680.0), |[w, h]| (w, h));
            let name = cx
                .layout_size("intro-name")
                .map_or((340.0, 77.0), |[w, h]| (w, h));
            root.child(intro_view(s, &k, t, window, name).pin(
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
            ))
        }
        None => root,
    };
    if s.settings_open && s.page == Page::Inbox {
        root.child(settings_modal(s, &k))
    } else if s.add_repo_open && s.page == Page::Inbox {
        root.child(repositories_modal(s, &k))
    } else {
        root
    }
}

pub(crate) fn on_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key == rune::Key::Escape && s.intro.is_some() {
        s.intro = None;
        return true;
    }
    if ev.key == rune::Key::Escape && s.merge_armed.is_some() {
        s.merge_armed = None;
        return true;
    }
    if ev.key == rune::Key::Escape && s.add_repo_open {
        s.add_repo_open = false;
        return true;
    }
    if ev.is_command(',') && s.page == Page::Inbox {
        s.settings_open = !s.settings_open;
        return true;
    }
    if ev.is_command('r') {
        s.sync_now();
        return true;
    }
    false
}
