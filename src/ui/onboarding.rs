//! The first launch: the intro and the one-screen setup.

use crate::*;

pub(crate) fn phase(t: f32, from: f32, to: f32) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

pub(crate) fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p).powi(3)
}

/// Drawn exactly where the intro's clock puts it this frame: no spring follows the change.
pub(crate) fn frame_exact(e: El) -> El {
    let now = rune::Anim::INSTANT;
    e.scale_motion(now)
        .offset_motion(now)
        .opacity_motion(now)
        .rotate_motion(now)
        .blur_motion(now)
}

/// The first-launch intro at `t` seconds, over the whole `w`×`h` window: the approve button
/// springs in, a finger falls onto it, approvals fly off, the name appears, and then button and
/// name fly into the header, where the logo and name take over. Every value is computed from
/// `t`, so nothing springs. `name_w` and `name_h` are the full-size name's last laid-out size.
pub(crate) fn intro_view(
    s: &App,
    k: &Kit,
    t: f32,
    (w, h): (f32, f32),
    (name_w, name_h): (f32, f32),
) -> El {
    let c = &k.c;
    let green = oklch(0.57, 0.15, 150.0);
    let art = |image: Option<&rune::Image>, size: f32| match image {
        Some(image) => el().size(size, size).pixels(image.pixels()),
        None => el().size(size, size),
    };
    let since_hit = t - INTRO_HIT;
    // The press: the face sinks, the button squashes, then both spring back.
    let press = if since_hit >= 0.0 {
        (-7.0 * since_hit).exp() * (16.0 * since_hit).cos()
    } else {
        0.0
    };
    let sink = press.clamp(0.0, 1.0) * 10.0;
    let arrive = phase(t, 0.1, 0.9);
    let spring = 1.0 - (-5.5 * arrive).exp() * (10.0 * arrive).cos();
    let shown = phase(t, 0.1, 0.5);
    // Into the header: the header's logo is centred at (95, 25), its name starts at x 115.
    let p = phase(t, 4.0, 4.85);
    let dock = if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) / 2.0
    };
    let lerp = |a: f32, b: f32| a + (b - a) * dock;
    let (cx, cy) = (w / 2.0, h / 2.0 - 55.0);

    let mut layer = col()
        .key("intro")
        .w_full()
        .h_full()
        .instant_changes()
        .cursor(rune::Cursor::Pointer)
        .on_press(|s: &mut App| s.intro = None)
        .child(frame_exact(
            el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .fill(c.bg_base)
                .opacity(1.0 - phase(t, 4.3, INTRO_SECS)),
        ))
        .child(frame_exact(
            el().absolute(cx - 210.0, cy - 210.0)
                .size(420.0, 420.0)
                .radial_gradient(
                    [0.5, 0.5],
                    210.0,
                    [
                        (
                            0.0,
                            green.alpha(
                                if k.dark { 0.42 } else { 0.24 } * (0.6 + 0.4 * press.max(0.0)),
                            ),
                        ),
                        (1.0, green.alpha(0.0)),
                    ],
                )
                .opacity(shown * (1.0 - dock)),
        ));
    for i in 0..3 {
        let from = INTRO_HIT + i as f32 * 0.11;
        let p = ease_out(phase(t, from, from + 1.1));
        layer = layer.child(frame_exact(
            el().absolute(cx - 100.0, cy - 100.0)
                .size(200.0, 200.0)
                .radius(100.0)
                .border(3.0 - 2.0 * p, mint(k))
                .scale(1.0 + p * (1.9 + i as f32 * 0.5))
                .opacity(if t < from { 0.0 } else { 0.7 * (1.0 - p) }),
        ));
    }
    // The button turns into the logo as it lands in the header.
    let logo = phase(dock, 0.5, 1.0);
    layer = layer.child(frame_exact(
        col()
            .absolute(cx - 100.0, cy - 100.0)
            .size(200.0, 212.0)
            .scale(lerp((0.55 + 0.45 * spring) * (1.0 - 0.07 * press), 0.11))
            .offset(lerp(0.0, 95.0 - cx), lerp(0.0, 25.7 - (cy + 6.0)))
            .opacity(shown)
            .blur((1.0 - shown) * 12.0)
            .child(
                art(s.intro_art.first(), 200.0)
                    .absolute(0.0, 12.0)
                    .opacity(1.0 - logo),
            )
            .child(
                art(s.intro_art.get(1), 200.0)
                    .absolute(0.0, 0.0)
                    .offset(0.0, sink)
                    .opacity(1.0 - logo),
            )
            .child(art(s.logo.as_ref(), 200.0).absolute(0.0, 0.0).opacity(logo)),
    ));
    let approvals = [
        ("LGTM", -150.0, 200.0),
        ("+1", -115.0, 230.0),
        ("ship it", -65.0, 210.0),
        ("approved", -30.0, 250.0),
        ("👍", 20.0, 200.0),
        ("didn't read, looks good", 160.0, 215.0),
    ];
    for (i, (label, degrees, distance)) in approvals.into_iter().enumerate() {
        let from = INTRO_HIT + 0.04 + i as f32 * 0.035;
        let p = 1.0 - (1.0 - phase(t, from, from + 1.25)).powi(5);
        let angle = (degrees as f32).to_radians();
        let width = label.chars().count() as f32 * 7.5 + 22.0;
        layer = layer.child(frame_exact(
            row()
                .absolute(cx - width / 2.0, cy - 13.0)
                .px(11.0)
                .py(5.0)
                .radius(999.0)
                .fill(mint(k).alpha(0.12))
                .border(1.0, mint(k).alpha(0.3))
                .offset(
                    angle.cos() * distance * p,
                    angle.sin() * distance * p + 40.0 * p * p,
                )
                .opacity(if t < from {
                    0.0
                } else {
                    (p * 6.0).min(1.0) * (1.0 - phase(t, from + 0.7, from + 1.3))
                })
                .child(text(label).font(TextStyle::new(14.0, 600.0)).color(mint(k))),
        ));
    }
    let fall = phase(t, 0.8, INTRO_HIT).powi(3);
    let lift = phase(t, 1.9, 2.6).powi(2);
    let drop = if t >= INTRO_HIT {
        sink
    } else {
        -360.0 * (1.0 - fall)
    };
    layer = layer.child(frame_exact(
        art(s.intro_art.get(2), 110.0)
            .size(110.0, 275.0)
            .absolute(cx - 55.0, cy - 295.0)
            .offset(0.0, drop - lift * 360.0)
            .rotate(((1.0 - fall) * -4.0 + lift * 3.0).to_radians())
            .opacity(ease_out(phase(t, 0.8, 1.15)) * (1.0 - phase(t, 2.0, 2.6))),
    ));

    let name_top = cy + 110.0;
    let mut name = row()
        .key("intro-name")
        .absolute(cx - name_w / 2.0, name_top)
        .origin(0.0, 0.5)
        .scale(lerp(1.0, 14.0 / 64.0))
        .offset(
            lerp(0.0, 115.0 - (cx - name_w / 2.0)),
            lerp(0.0, 25.0 - (name_top + name_h / 2.0)),
        );
    for (i, ch) in "Meat Proxy".chars().enumerate() {
        let from = 2.4 + i as f32 * 0.055;
        let p = ease_out(phase(t, from, from + 0.7));
        name = name.child(frame_exact(if ch == ' ' {
            el().size(18.0, 10.0)
        } else {
            text(ch.to_string())
                .font(TextStyle::new(64.0, 720.0))
                .color(c.text_base)
                .opacity(p)
                .offset(0.0, (1.0 - p) * 22.0)
                .blur((1.0 - p) * 8.0)
        }));
    }
    let tagline = ease_out(phase(t, 3.0, 3.8)) * (1.0 - phase(t, 3.9, 4.2));
    layer.child(frame_exact(name)).child(frame_exact(
        row()
            .absolute(0.0, name_top + name_h + 4.0)
            .w_full()
            .justify_center()
            .opacity(tagline)
            .offset(0.0, (1.0 - tagline) * 12.0)
            .child(
                text("Keep up with the pace of LLM-generated code.")
                    .font(TextStyle::new(19.0, 450.0))
                    .color(c.text_muted),
            ),
    ))
}

pub(crate) fn onboarding_page_view(s: &App, k: &Kit, intro: Option<f32>) -> El {
    let c = &k.c;
    let connected = match &s.sync_status {
        SyncStatus::Connected { username } => Some(username),
        _ => None,
    };
    let mut doc = col()
        .key("setup-github")
        .w_full()
        .max_w(560.0)
        .py(48.0)
        .gap(14.0)
        .child(
            text("Connect GitHub.")
                .font(TextStyle::new(40.0, 720.0))
                .color(c.text_base),
        )
        .child(
            text("Meat Proxy puts the pull requests that need your thumb first. It reads GitHub through the GitHub CLI, signed in as you. No tokens to paste.")
                .wrap()
                .font(k.ty.body)
                .color(c.text_muted),
        );

    if let Some(notice) = &s.status_banner {
        doc = doc.child(notice_frame(k, notice).child(notice_view(k, notice)));
    }

    let (glyph, status) = if let Some(username) = connected {
        ("✓", format!("Signed in as @{username} through GitHub CLI."))
    } else if s.auth_worker.is_pending() {
        (
            "❯_",
            "Waiting for you to finish in the browser…".to_string(),
        )
    } else if s.loading
        || s.sync_worker.is_pending()
        || matches!(s.sync_status, SyncStatus::Checking)
    {
        ("❯_", "Checking GitHub CLI…".to_string())
    } else {
        ("❯_", "GitHub CLI is not signed in.".to_string())
    };
    let mut card = col()
        .w_full()
        .radius(10.0)
        .fill(surface(k))
        .border(1.0, c.border_weak)
        .child(
            row()
                .w_full()
                .px(16.0)
                .py(14.0)
                .center_y()
                .gap(12.0)
                .child(text(glyph).font(k.ty.mono).color(mint(k)))
                .child(text(status).font(k.ty.body).color(c.text_base)),
        );
    // What gh prints while it signs in, the one-time code among it.
    let lines: Vec<String> = s
        .auth_lines
        .lock()
        .map(|lines| lines.iter().rev().take(4).rev().cloned().collect())
        .unwrap_or_default();
    if connected.is_none() && (s.auth_worker.is_pending() || !lines.is_empty()) {
        let mut term = col()
            .key("onboard-terminal")
            .w_full()
            .px(16.0)
            .py(12.0)
            .gap(4.0)
            .child(
                text("❯ gh auth login --web")
                    .font(k.ty.mono)
                    .color(c.text_base),
            );
        for line in lines {
            term = term.child(text(line).wrap().font(k.ty.mono).color(c.text_muted));
        }
        card = card
            .child(el().w_full().h(1.0).fill(c.border_weak))
            .child(term);
    }
    doc = doc.child(card).child(
        row()
            .w_full()
            .px(16.0)
            .py(14.0)
            .radius(10.0)
            .fill(surface(k))
            .border(1.0, c.border_weak)
            .center_y()
            .gap(16.0)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(text("Tap me on the shoulder").font(TextStyle::new(15.0, 650.0)).color(c.text_base))
                    .child(
                        text("A macOS notification when a review is requested of you or your PR sits unreviewed.")
                            .wrap()
                            .font(k.ty.small)
                            .color(c.text_muted),
                    ),
            )
            .child(
                Switch::new(s.native_notifications)
                    .label("Tap me on the shoulder")
                    .hide_label()
                    .on_toggle(|s: &mut App| {
                        s.native_notifications = !s.native_notifications;
                        if s.native_notifications {
                            s.send_welcome_notification();
                        }
                    })
                    .build(k)
                    .key("onboard-notifications"),
            ),
    );

    if connected.is_some() {
        doc = doc.child(
            row().pt(6.0).child(
                Button::new("Open inbox")
                    .variant(ButtonVariant::Contrast)
                    .on_press(|s: &mut App| {
                        s.complete_onboarding();
                        s.trigger_discovery();
                    })
                    .build(k)
                    .key("onboard-open-inbox"),
            ),
        );
    } else {
        doc = doc
            .child(
                row()
                    .pt(6.0)
                    .gap(8.0)
                    .child(
                        Button::new("Authenticate with GitHub CLI")
                            .variant(ButtonVariant::Contrast)
                            .disabled(s.auth_worker.is_pending())
                            .on_press(|s: &mut App| {
                                if s.allow_os_side_effects {
                                    s.status_banner = None;
                                    s.auth_lines.lock().unwrap().clear();
                                    let lines = s.auth_lines.clone();
                                    s.auth_worker.request((), move |()| github::login_in_browser(lines));
                                }
                            })
                            .build(k)
                            .key("onboard-run-gh-auth"),
                    )
                    .child(
                        Button::new("Check connection")
                            .variant(ButtonVariant::Outline)
                            .disabled(s.sync_worker.is_pending())
                            .on_press(|s: &mut App| {
                                s.status_banner = None;
                                s.trigger_sync();
                            })
                            .build(k)
                            .key("onboard-recheck-gh"),
                    ),
            )
            .child(
                text("Or run gh auth login --hostname github.com --web in your terminal, then check the connection.")
                    .wrap()
                    .font(k.ty.small)
                    .color(c.text_muted),
            );
        if let SyncStatus::NeedsAuth { reason } = &s.sync_status
            && !s.auth_worker.is_pending()
        {
            doc = doc.child(
                text(reason.clone())
                    .wrap()
                    .font(k.ty.small)
                    .color(c.text_faint),
            );
        }
    }

    // The scroll sits on a column: a row clips its children to its own height and never overflows.
    let setup = col().w_full().h_full().min_h(0.0).child(
        col()
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .grow(1.0)
            .min_h(0.0)
            .child(row().w_full().px(28.0).justify_center().child(doc)),
    );
    match intro {
        // The setup screen fades in under the intro as it ends.
        Some(t) => setup
            .instant_changes()
            .opacity(phase(t, INTRO_SECS - 0.6, INTRO_SECS)),
        None => setup,
    }
}

pub(crate) fn setup_row(k: &Kit, label: &str, control: El) -> El {
    row()
        .w_full()
        .gap(10.0)
        .center_y()
        .justify_between()
        .child(text(label).font(k.ty.body).color(k.c.text_base))
        .child(control)
}

pub(crate) fn add_repository_panel(s: &App, k: &Kit, framed: bool) -> El {
    let c = &k.c;
    let org = s.organization.text.trim().to_ascii_lowercase();
    let mut panel = col().w_full().gap(10.0);
    if framed {
        panel = panel
            .padding(12.0)
            .radius(8.0)
            // One step off the repository rows (layer_01), still neutral.
            .fill(c.layer_02)
            .border(1.0, c.border_weak)
            .child(
                row()
                    .w_full()
                    .center_y()
                    .justify_between()
                    .child(
                        text("Add repositories")
                            .font(k.ty.ui_medium)
                            .color(c.text_base),
                    )
                    .child(
                        icon_action(&CLOSE, "Close")
                            .on_press(|s: &mut App| s.add_repo_open = false)
                            .build(k)
                            .key("add-repo-close"),
                    ),
            );
    }

    let mut org_row = row().w_full().center_y().gap(8.0).child(
        TextInput::new(&s.organization)
            .placeholder("Your organization, e.g. saleor")
            .width(Some(240.0))
            .on_edit(|s: &mut App| &mut s.organization)
            .build(k),
    );
    if let Some(found) = &s.discovery {
        for name in found.organizations.iter().take(4) {
            let pick = name.clone();
            org_row = org_row.child(
                Button::new(name.clone())
                    .variant(if name.to_ascii_lowercase() == org {
                        ButtonVariant::Contrast
                    } else {
                        ButtonVariant::Outline
                    })
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| {
                        s.organization = TextEdit::new(&pick);
                        s.persist_repos_now();
                    })
                    .build(k)
                    .key(format!("org-chip-{name}")),
            );
        }
    }
    panel = panel.child(org_row);

    match (&s.discovery, &s.discovery_error) {
        (_, Some(error)) => {
            panel = panel.child(
                text(error.clone())
                    .wrap()
                    .font(k.ty.small)
                    .color(c.text_muted),
            );
        }
        (None, None) => {
            panel = panel.child(
                text("Finding the repositories you work in…")
                    .font(k.ty.small)
                    .color(c.text_muted),
            );
        }
        (Some(found), None) => {
            let suggestions: Vec<_> = found
                .repos
                .iter()
                .filter(|r| org.is_empty() || r.owner().to_ascii_lowercase() == org)
                .filter(|r| {
                    !s.repos
                        .iter()
                        .any(|t| !t.removed && t.name.eq_ignore_ascii_case(&r.name))
                })
                .take(8)
                .collect();
            if suggestions.is_empty() {
                panel = panel.child(
                    text("No more recent repositories. Add one by name below.")
                        .font(k.ty.small)
                        .color(c.text_muted),
                );
            }
            for repo in suggestions {
                let name = repo.name.clone();
                panel = panel.child(
                    row()
                        .w_full()
                        .center_y()
                        .justify_between()
                        .child(text(repo.name.clone()).font(k.ty.body).color(c.text_base))
                        .child(
                            row()
                                .center_y()
                                .gap(8.0)
                                .child(
                                    text(format!(
                                        "{} PRs · {} reviews",
                                        repo.prs_90d, repo.reviews_90d
                                    ))
                                    .font(k.ty.small)
                                    .color(c.text_faint),
                                )
                                .child(
                                    icon_action(&PLUS, &format!("Add {}", repo.name))
                                        .on_press(move |s: &mut App| s.add_repo(&name))
                                        .build(k)
                                        .key(format!("add-repo-{}", repo.name)),
                                ),
                        ),
                );
            }
        }
    }

    let panel = panel.child(
        row()
            .w_full()
            .center_y()
            .gap(8.0)
            .child(
                TextInput::new(&s.new_repo)
                    .placeholder("Or add by name: owner/name")
                    .width(Some(240.0))
                    .on_edit(|s: &mut App| &mut s.new_repo)
                    .build(k),
            )
            .child(
                Button::new("Add")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .disabled(s.verify_worker.is_pending() || s.new_repo.text.trim().is_empty())
                    .on_press(|s: &mut App| {
                        let input = s.new_repo.text.clone();
                        s.verify_worker
                            .request((), move |()| github::verify_repo(&input));
                    })
                    .build(k)
                    .key("add-repo-by-name"),
            ),
    );
    match &s.status_banner {
        Some(notice) if !framed => {
            panel.child(notice_frame(k, notice).child(notice_view(k, notice)))
        }
        _ => panel,
    }
}

pub(crate) fn default_filter_row(s: &App, k: &Kit) -> El {
    setup_row(
        k,
        "Default inbox",
        SegmentedControl::new(
            &InboxFilter::TABS.map(|(_, label)| label),
            Some(s.default_filter.index()),
        )
        .label("Default inbox")
        .width(Some(260.0))
        .on_pick(|s: &mut App, idx, _| {
            s.default_filter = InboxFilter::from_index(idx);
            if s.onboarded
                && s.settings_open
                && let Some(db) = s.db()
            {
                let _ = db.set_setting("default_tab", s.default_filter.name());
            }
        })
        .build(k)
        .key("default-inbox-filter"),
    )
}
