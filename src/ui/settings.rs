//! The Settings and Repositories sheets.

use crate::*;

/// A switch for one choice about which pull requests the sync brings in.
pub(crate) fn sync_option_row(
    k: &Kit,
    label: &'static str,
    on: bool,
    key: &'static str,
    field: fn(&mut github::SyncOptions) -> &mut bool,
) -> El {
    setup_row(
        k,
        label,
        Switch::new(on)
            .label(label)
            .hide_label()
            .on_toggle(move |s: &mut App| {
                let flag = field(&mut s.sync_options);
                *flag = !*flag;
                let value = *flag;
                s.save_flag(key, value);
                s.trigger_sync();
            })
            .build(k)
            .key(format!("option-{key}")),
    )
}

/// Settings › Autonomous: two switches, by who an action reaches. What each does stays out of
/// sight; the notification after each action says it.
pub(crate) fn autonomy_section(s: &App, k: &Kit) -> El {
    let switch = |label: &'static str, on: bool, branches: bool, key: &'static str| {
        setup_row(
            k,
            label,
            Switch::new(on)
                .label(label)
                .hide_label()
                .on_toggle(move |s: &mut App| s.toggle_autonomy(branches))
                .build(k)
                .key(key),
        )
    };
    col()
        .w_full()
        .gap(10.0)
        .child(text("Autonomous").font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            text("Your own PRs only. You get a notification each time.")
                .wrap()
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(switch(
            "Update branches and auto-merge",
            s.autonomy_branches,
            true,
            "option-autonomy-branches",
        ))
        .child(switch(
            "Re-request reviews after I push",
            s.autonomy_reviewers,
            false,
            "option-autonomy-reviewers",
        ))
}

pub(crate) fn settings_content(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let open_idx = match s.open_stale_hours {
        24 => 0,
        48 => 1,
        72 => 2,
        _ => 3,
    };
    let draft_idx = match s.draft_stale_hours {
        48 => 0,
        72 => 1,
        120 => 2,
        _ => 3,
    };

    let notifications_row = setup_row(
        k,
        "Show notifications",
        row().center_y().gap(10.0).child(
            Switch::new(s.native_notifications)
                .label("Show notifications")
                .hide_label()
                .on_toggle(|s: &mut App| {
                    s.native_notifications = !s.native_notifications;
                    if s.native_notifications {
                        s.send_welcome_notification();
                    }
                    s.save_flag("native_notifications", s.native_notifications);
                    s.next_fast_poll_at = 0.0;
                })
                .build(k),
        ),
    );

    let thresholds_section = col()
        .w_full()
        .gap(10.0)
        .child(
            text("Notifications")
                .font(k.ty.ui_medium)
                .color(c.text_base),
        )
        .child(notifications_row)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text("Nudge my open PRs if unreviewed after")
                        .font(k.ty.body)
                        .color(c.text_muted),
                )
                .child(
                    SegmentedControl::new(&["1d", "2d", "3d", "5d"], Some(open_idx))
                        .label("Open PR Stale")
                        .width(Some(200.0))
                        .on_pick(|s: &mut App, idx, _| {
                            s.open_stale_hours = match idx {
                                0 => 24,
                                1 => 48,
                                2 => 72,
                                _ => 120,
                            };
                            s.persist_thresholds();
                        })
                        .build(k)
                        .key("open-threshold-seg"),
                ),
        )
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text("Nudge my draft PRs if untouched after")
                        .font(k.ty.body)
                        .color(c.text_muted),
                )
                .child(
                    SegmentedControl::new(&["2d", "3d", "5d", "7d"], Some(draft_idx))
                        .label("Draft PR Stale")
                        .width(Some(200.0))
                        .on_pick(|s: &mut App, idx, _| {
                            s.draft_stale_hours = match idx {
                                0 => 48,
                                1 => 72,
                                2 => 120,
                                _ => 168,
                            };
                            s.persist_thresholds();
                        })
                        .build(k)
                        .key("draft-threshold-seg"),
                ),
        );
    let ai_field = |label: &'static str, input: El| setup_row(k, label, input);
    let ai_section = col()
        .w_full()
        .gap(10.0)
        .child(text("AI").font(k.ty.ui_medium).color(c.text_base))
        .child(
            text("When on, the description and changed file paths of every PR that asks for your review, private repositories included, are sent to the model at this URL. Off, only rules sort.")
                .wrap()
                .font(k.ty.small)
                .color(c.text_faint),
        )
        .child(setup_row(
            k,
            "Use AI sorting",
            Switch::new(s.ai.enabled)
                .label("Use AI sorting")
                .hide_label()
                .on_toggle(|s: &mut App| s.toggle_ai())
                .build(k)
                .key("option-ai-enabled"),
        ))
        .child(ai_field(
            "API URL",
            TextInput::new(&s.ai.url)
                .label("AI API URL")
                .disabled(!s.ai.enabled)
                .placeholder("https://openrouter.ai/api/alpha/decisions")
                .width(Some(360.0))
                .on_edit(|s: &mut App| &mut s.ai.url)
                .build(k)
                .key("option-ai-url"),
        ))
        .child(ai_field(
            "Model",
            TextInput::new(&s.ai.model)
                .label("AI model")
                .disabled(!s.ai.enabled)
                .placeholder("typesafe/jev-1.13")
                .width(Some(360.0))
                .on_edit(|s: &mut App| &mut s.ai.model)
                .build(k)
                .key("option-ai-model"),
        ))
        .child(ai_field(
            "API key",
            TextInput::new(&s.ai.key)
                .label("AI API key")
                .disabled(!s.ai.enabled)
                .placeholder("sk-…")
                .secure()
                .width(Some(360.0))
                .on_edit(|s: &mut App| &mut s.ai.key)
                .build(k)
                .key("option-ai-key"),
        ));

    let doc = col()
        .key("settings-column")
        .w_full()
        .max_w(740.0)
        .pb(16.0)
        .gap(12.0)
        .child(panel(k).child(default_filter_row(s, k)))
        .child(
            panel(k)
                .child(
                    text("Pull requests")
                        .font(k.ty.ui_medium)
                        .color(c.text_base),
                )
                .child(setup_row(
                    k,
                    "Show team review requests in the Inbox",
                    Switch::new(s.show_team_reviews)
                        .label("Show team review requests in the Inbox")
                        .hide_label()
                        .on_toggle(|s: &mut App| s.toggle_show_team_reviews())
                        .build(k)
                        .key("option-show-team-reviews"),
                ))
                .child(setup_row(
                    k,
                    "Show outside contributors in the Inbox",
                    Switch::new(!s.hide_external)
                        .label("Show outside contributors in the Inbox")
                        .hide_label()
                        .on_toggle(|s: &mut App| s.toggle_hide_external())
                        .build(k)
                        .key("option-hide-external"),
                ))
                .child(sync_option_row(
                    k,
                    "Show archived repositories",
                    !s.sync_options.hide_archived,
                    "hide_archived",
                    |o| &mut o.hide_archived,
                ))
                .child(setup_row(
                    k,
                    "Show PRs opened in the last",
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(
                            text("days · 0 = no limit")
                                .font(k.ty.small)
                                .color(c.text_faint),
                        )
                        .child(
                            TextInput::new(&s.max_age_input)
                                .label("Show PRs opened in the last (days)")
                                .width(Some(64.0))
                                .on_edit(|s: &mut App| &mut s.max_age_input)
                                .build(k)
                                .key("option-max-age-days"),
                        ),
                )),
        )
        .child(panel(k).child(autonomy_section(s, k)))
        .child(panel(k).child(pace_section(s, k)))
        .children(s.ai.feature.then(|| panel(k).child(ai_section)))
        .child(panel(k).child(thresholds_section));

    col()
        .key("settings-viewport")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(16.0)
        .child(doc)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
}

/// The release this app was built as. A packaged app carries it in its Info.plist (the release tag,
/// which can be ahead of Cargo.toml); a `cargo run` build says so.
pub(crate) fn app_version() -> String {
    let from_bundle = std::env::current_exe().ok().and_then(|exe| {
        let plist = std::fs::read_to_string(exe.parent()?.parent()?.join("Info.plist")).ok()?;
        let after = plist
            .split("<key>CFBundleShortVersionString</key>")
            .nth(1)?;
        let value = after.split("<string>").nth(1)?.split("</string>").next()?;
        Some(value.trim().to_owned())
    });
    from_bundle.unwrap_or_else(|| format!("{} (dev build)", env!("CARGO_PKG_VERSION")))
}

pub(crate) fn settings_modal(s: &App, k: &Kit) -> El {
    let close = rune_kit::IconButton::new(k.glyph.close)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label("Close settings")
        .help("Close (Esc)")
        .on_press(|s: &mut App| s.settings_open = false)
        .build(k)
        .key("close-settings");
    let dialog = col()
        .key("settings-dialog")
        .role("dialog")
        .label("Settings")
        .w_full()
        .max_w(740.0)
        .h_full()
        .max_h(550.0)
        .min_h(0.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .child(
            row()
                .w_full()
                .padding(16.0)
                .center_y()
                .justify_between()
                .child(
                    row()
                        .center_y()
                        .gap(10.0)
                        .child(
                            text("Settings")
                                .font(TextStyle::new(22.0, 650.0))
                                .color(k.c.text_base),
                        )
                        .child(
                            text(format!("v{}", app_version()))
                                .font(TextStyle::new(11.0, 450.0))
                                .color(k.c.text_faint)
                                .key("app-version"),
                        ),
                )
                .child(close),
        )
        .child(settings_content(s, k))
        .modal(true, |s: &mut App| s.settings_open = false)
        .modal_click_outside(true);
    el().key("settings-overlay")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .padding(24.0)
        .fill(k.c.scrim)
        .center()
        .child(dialog)
}

pub(crate) fn repositories_modal(s: &App, k: &Kit) -> El {
    let close = rune_kit::IconButton::new(k.glyph.close)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label("Close")
        .help("Close (Esc)")
        .on_press(|s: &mut App| s.add_repo_open = false)
        .build(k)
        .key("close-add-repo");
    let dialog = col()
        .key("add-repo-dialog")
        .role("dialog")
        .label("Add repositories")
        .w_full()
        .max_w(560.0)
        .h_full()
        .max_h(560.0)
        .min_h(0.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_base)
        .child(
            row()
                .w_full()
                .padding(16.0)
                .center_y()
                .justify_between()
                .child(
                    text("Add repositories")
                        .font(TextStyle::new(22.0, 650.0))
                        .color(k.c.text_base),
                )
                .child(close),
        )
        .child(
            col()
                .w_full()
                .px(16.0)
                .pb(16.0)
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(add_repository_panel(s, k, false)),
        )
        .modal(true, |s: &mut App| s.add_repo_open = false)
        .modal_click_outside(true);
    el().key("add-repo-overlay")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .padding(24.0)
        .fill(k.c.scrim)
        .center()
        .child(dialog)
}
