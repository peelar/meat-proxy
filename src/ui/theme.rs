//! Shared look: colours, icons, panels and the ink buttons.

use crate::*;

// --- Minimal Document UI ---

pub(crate) const GEAR: rune::Icon = rune::Icon {
    name: "settings-gear",
    view: 24.0,
    body: r#"<path stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" d="M10 3h4l.6 2.4 1.5.9 2.4-.7 2 3.4-1.8 1.7v2.5l1.8 1.7-2 3.4-2.4-.7-1.5.9L14 21h-4l-.6-2.5-1.5-.9-2.4.7-2-3.4 1.8-1.7v-2.5L3.5 9l2-3.4 2.4.7 1.5-.9Z"/><circle cx="12" cy="12" r="3.1" stroke="currentColor" stroke-width="1.7"/>"#,
};
pub(crate) const STACK_BRANCH: rune::Icon = rune::Icon {
    name: "stack-branch",
    view: 16.0,
    body: r#"<path d="M3 1v6a3 3 0 0 0 3 3h6m-3-3 3 3-3 3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub(crate) const PR_ICON: rune::Icon = rune::Icon {
    name: "pull-request",
    view: 20.0,
    body: r#"<circle stroke="currentColor" stroke-width="1.6" cx="5" cy="4" r="2"/><circle stroke="currentColor" stroke-width="1.6" cx="5" cy="16" r="2"/><circle stroke="currentColor" stroke-width="1.6" cx="15" cy="16" r="2"/><path stroke="currentColor" stroke-width="1.6" d="M5 6v8m10 0V9a4 4 0 0 0-4-4h-1m3-3-3 3 3 3"/>"#,
};
pub(crate) const REPO_ICON: rune::Icon = rune::Icon {
    name: "repository",
    view: 20.0,
    body: r#"<path stroke="currentColor" stroke-width="1.6" d="M4 15V4a1 1 0 0 1 1-1h11v14H6a2 2 0 0 1 0-4h10M8 6h5"/>"#,
};

pub(crate) fn mint(k: &Kit) -> Color {
    oklch(if k.dark { 0.83 } else { 0.44 }, 0.10, 161.0)
}

pub(crate) fn surface(k: &Kit) -> Color {
    oklch(if k.dark { 0.185 } else { 1.0 }, 0.003, 175.0)
}

pub(crate) fn panel(k: &Kit) -> El {
    col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .radius(9.0)
        .border(1.0, k.c.border_weak)
        .fill(surface(k))
}

pub(crate) fn heading(k: &Kit, label: &str) -> El {
    text(label)
        .font(TextStyle::new(31.0, 660.0))
        .color(k.c.text_base)
}

pub(crate) fn icon_action(glyph: &'static rune::Icon, label: &str) -> rune_kit::IconButton {
    rune_kit::IconButton::new(glyph)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .label(label.to_owned())
        .help(label.to_owned())
}

pub(crate) const PLUS: rune::Icon = rune::Icon {
    name: "add",
    view: 16.0,
    body: r#"<path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#,
};

/// Ignore a pull request: an eye with a slash through it. Stop ignoring: the eye.
pub(crate) const EYE_OFF: rune::Icon = rune::Icon {
    name: "ignore",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M1.6 8s2.3-4.2 6.4-4.2S14.4 8 14.4 8s-2.3 4.2-6.4 4.2S1.6 8 1.6 8Z"/><circle fill="none" stroke="currentColor" stroke-width="1.3" cx="8" cy="8" r="1.9"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" d="M2.8 13.2L13.2 2.8"/>"#,
};
pub(crate) const EYE: rune::Icon = rune::Icon {
    name: "stop-ignoring",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M1.6 8s2.3-4.2 6.4-4.2S14.4 8 14.4 8s-2.3 4.2-6.4 4.2S1.6 8 1.6 8Z"/><circle fill="none" stroke="currentColor" stroke-width="1.3" cx="8" cy="8" r="1.9"/>"#,
};

/// Merge: two branches joining.
pub(crate) const MERGE: rune::Icon = rune::Icon {
    name: "merge",
    view: 16.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="4.5" cy="3.5" r="1.6"/><circle cx="4.5" cy="12.5" r="1.6"/><circle cx="11.5" cy="9" r="1.6"/><path d="M4.5 5.1v5.8M4.5 5.1c0 2.4 2.2 3.9 5.4 3.9"/></g>"#,
};

/// Auto-merge: a bolt.
pub(crate) const BOLT: rune::Icon = rune::Icon {
    name: "auto-merge",
    view: 16.0,
    body: r#"<path fill="currentColor" d="M9.2 1.5 3.8 9h3.5l-.7 5.5L12.2 7H8.7l.5-5.5Z"/>"#,
};

/// A new best of the week: a cup.
pub(crate) const TROPHY: rune::Icon = rune::Icon {
    name: "trophy",
    view: 16.0,
    body: r#"<path fill="currentColor" fill-rule="evenodd" d="M4.5 2h7v1h2v1.5a2.5 2.5 0 0 1-2.2 2.5 3.5 3.5 0 0 1-2.55 2.4v2.1h2.25V13h-6v-1.5h2.25V9.4A3.5 3.5 0 0 1 4.7 7 2.5 2.5 0 0 1 2.5 4.5V3h2Zm7 2.4v1.4a1 1 0 0 0 .5-.9v-.5Zm-7 0H4v.5a1 1 0 0 0 .5.9Z"/>"#,
};

pub(crate) const TRASH: rune::Icon = rune::Icon {
    name: "remove",
    view: 16.0,
    body: r#"<path d="M3.5 4.8h9M6.4 4.8V3.4h3.2v1.4M4.8 4.8l.5 7.4h5.4l.5-7.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub(crate) const CLOSE: rune::Icon = rune::Icon {
    name: "close",
    view: 16.0,
    body: r#"<path d="M4.5 4.5l7 7M11.5 4.5l-7 7" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#,
};

/// Marks what a model decided, as opposed to a rule: a brain, two lobes with a fold in each.
pub(crate) const BRAIN: rune::Icon = rune::Icon {
    name: "model-brain",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M8 3.4C8 2.3 7.1 1.7 6.2 2c-1-.4-2.2.3-2.1 1.4-1.1.2-1.8 1.4-1.2 2.4C2 6.5 2 7.8 2.9 8.4c-.2 1.1.5 2.1 1.6 2.2.2 1.1 1.3 1.8 2.3 1.5.4.5.8.6 1.2.6"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M8 3.4C8 2.3 8.9 1.7 9.8 2c1-.4 2.2.3 2.1 1.4 1.1.2 1.8 1.4 1.2 2.4.9.7.9 2 0 2.6.2 1.1-.5 2.1-1.6 2.2-.2 1.1-1.3 1.8-2.3 1.5-.4.5-.8.6-1.2.6"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M8 3.4v9.1"/><path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M4.2 5.2c1 .2 1.7.9 1.8 1.8M11.8 5.2c-1 .2-1.7.9-1.8 1.8M4.5 10.6c.9-.2 1.5-.8 1.6-1.6M11.5 10.6c-.9-.2-1.5-.8-1.6-1.6"/>"#,
};

/// Back to the default: an arrow turning counter-clockwise.
pub(crate) const RESET: rune::Icon = rune::Icon {
    name: "reset-order",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M3.2 8a4.8 4.8 0 1 0 1.5-3.5M3 2.6v2.6h2.6"/>"#,
};

/// Update branch: two arrows chasing each other.
pub(crate) const UPDATE: rune::Icon = rune::Icon {
    name: "update-branch",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M12.9 7a5 5 0 0 0-8.7-2.3M3.1 9a5 5 0 0 0 8.7 2.3M4 2.3v2.6h2.6M12 13.7v-2.6H9.4"/>"#,
};

/// Copy: one sheet over another.
pub(crate) const COPY: rune::Icon = rune::Icon {
    name: "copy-prompt",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" d="M7.2 5.6h4.2a1.6 1.6 0 0 1 1.6 1.6v4.2a1.6 1.6 0 0 1-1.6 1.6H7.2a1.6 1.6 0 0 1-1.6-1.6V7.2a1.6 1.6 0 0 1 1.6-1.6ZM10.4 3.9v-.3a1.2 1.2 0 0 0-1.2-1.2H4A1.6 1.6 0 0 0 2.4 4v5.2a1.2 1.2 0 0 0 1.2 1.2h.3"/>"#,
};

/// The colour of the buttons that clear a blocker: a calm blue, apart from Approve's mint and
/// Merge's violet.
pub(crate) fn unblock_tone(k: &Kit) -> Color {
    oklch(if k.dark { 0.74 } else { 0.55 }, 0.13, 245.0)
}

/// The color of anything a model decided, so it reads as one thing apart from the rules.
pub(crate) fn model_ink(k: &Kit) -> Color {
    // Quiet on purpose: a hint of violet, close to the muted text, so it never competes with a status.
    oklch(if k.dark { 0.66 } else { 0.64 }, 0.045, 295.0)
}

pub(crate) const GRIP: rune::Icon = rune::Icon {
    name: "drag-handle",
    view: 16.0,
    body: r#"<circle cx="6" cy="4" r="1.1"/><circle cx="10" cy="4" r="1.1"/><circle cx="6" cy="8" r="1.1"/><circle cx="10" cy="8" r="1.1"/><circle cx="6" cy="12" r="1.1"/><circle cx="10" cy="12" r="1.1"/>"#,
};

pub(crate) const SECTION_GAP: f32 = 16.0;
/// Header plus its 1px rule and the section border.
pub(crate) const HEADER_HEIGHT: f32 = 40.0;
pub(crate) const SECTION_HEIGHT: f32 = HEADER_HEIGHT + 3.0;

pub(crate) const PIN: rune::Icon = rune::Icon {
    name: "pin",
    view: 24.0,
    body: r#"<path d="M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

/// What a Quiet ink button shows: at rest it has no fill, only its icon in its colour.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Ink {
    Rest,
    /// Waiting for the confirming press, with the share of the wait left.
    Armed(f32),
    /// Switched on (auto-merge): it keeps its tint.
    On,
    Busy,
    Disabled,
    /// Landed `age` seconds ago: it pops once.
    Done(f32),
}

/// Approve and Merge: no fill at rest, a tint under the pointer that deepens as the button sinks,
/// and the colour itself once it is armed or done. Every reward stays inside the button's box.
pub(crate) fn ink_button(
    k: &Kit,
    tone: Color,
    glyph: &'static rune::Icon,
    label: &str,
    ink: Ink,
    key: String,
) -> El {
    let c = &k.c;
    let live = matches!(ink, Ink::Rest | Ink::Armed(_) | Ink::On);
    let (text_ink, icon_ink) = match ink {
        Ink::Rest => (c.text_base, tone),
        Ink::Busy => (c.text_muted, c.text_muted),
        Ink::Disabled => (c.text_faint, c.text_faint),
        Ink::Armed(_) | Ink::On | Ink::Done(_) => (tone, tone),
    };
    let icon = if ink == Ink::Busy {
        rune_kit::Spinner::new().size(14.0).color(c.text_muted).build(k)
    } else {
        rune::icon(glyph, 14.0).color(icon_ink)
    };
    let mut button = row()
        .key(key)
        .role("button")
        .label(label.to_owned())
        .focusable(live)
        .disabled(!live)
        .h(26.0)
        .min_w(92.0)
        .px(10.0)
        .radius(6.0)
        .center()
        .gap(6.0)
        .cursor(if live {
            rune::Cursor::Pointer
        } else {
            rune::Cursor::Default
        })
        .child(icon.a11y_hidden())
        .child(text(label.to_owned()).font(TextStyle::new(12.5, 600.0)).color(text_ink));
    if live {
        button = button
            .hover(tone.alpha(0.12))
            .press(tone.alpha(0.12))
            .press_scale(0.94);
    }
    match ink {
        Ink::Armed(left) => {
            // The wait for the second press drains along the bottom edge.
            button = button.fill(tone.alpha(0.14)).child(
                row()
                    .pin(Some(6.0), None, Some(6.0), Some(2.0))
                    .h(2.0)
                    .instant_changes()
                    .child(el().h_full().w_frac(left.clamp(0.0, 1.0)).radius(1.0).fill(tone)),
            );
        }
        Ink::On => button = button.fill(tone.alpha(0.14)),
        Ink::Done(age) => {
            // A small overshoot from 0.88, once.
            let p = (age / 0.32).clamp(0.0, 1.0) - 1.0;
            let back = 1.0 + 2.7 * p * p * p + 1.7 * p * p;
            button = button
                .scale(0.88 + 0.12 * back)
                .scale_motion(rune::Anim::INSTANT);
        }
        _ => {}
    }
    button
}

/// Merge's violet, lighter on dark so it reads as text.
pub(crate) fn merge_tone(k: &Kit) -> Color {
    oklch(if k.dark { 0.72 } else { 0.56 }, if k.dark { 0.17 } else { 0.2 }, 295.0)
}

pub(crate) fn armed_left(armed: &Option<(String, std::time::Instant)>, id: &str) -> Option<f32> {
    armed
        .as_ref()
        .filter(|(armed, _)| armed == id)
        .map(|(_, at)| 1.0 - at.elapsed().as_secs_f32() / ARMED_SECS)
}

pub(crate) fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
