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

/// Collapse every group: two chevrons pointing in. Expanding them is the same pair pointing out.
pub(crate) const COLLAPSE_ALL: rune::Icon = rune::Icon {
    name: "collapse-all",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M4.5 2.5 8 5.5l3.5-3M4.5 13.5 8 10.5l3.5 3"/>"#,
};
pub(crate) const EXPAND_ALL: rune::Icon = rune::Icon {
    name: "expand-all",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M4.5 5.5 8 2.5l3.5 3M4.5 10.5 8 13.5l3.5-3"/>"#,
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

/// Claude Code: Anthropic's Claude mark (Simple Icons).
pub(crate) const CLAUDE: rune::Icon = rune::Icon {
    name: "agent-claude",
    view: 24.0,
    body: r#"<path fill="currentColor" d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/>"#,
};

/// Codex: the OpenAI mark, which is also ChatGPT's (Simple Icons).
pub(crate) const CODEX: rune::Icon = rune::Icon {
    name: "agent-codex",
    view: 24.0,
    body: r#"<path fill="currentColor" d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z"/>"#,
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

/// The status banner: the headline, then what the tool said, both wrapping. An error reads red.
pub(crate) fn notice_view(k: &Kit, notice: &Notice) -> El {
    let c = &k.c;
    let (headline_ink, detail_ink) =
        if notice.error { (c.danger.fg, c.text_base) } else { (c.text_base, c.text_muted) };
    let mut body = col().grow(1.0).min_w(0.0).gap(4.0).child(
        text(notice.headline.clone())
            .wrap()
            .font(k.ty.ui_medium)
            .color(headline_ink),
    );
    if !notice.detail.is_empty() {
        body = body.child(text(notice.detail.clone()).wrap().font(k.ty.small).color(detail_ink));
    }
    body
}

/// The frame around [`notice_view`]: the danger tint for an error, the plain layer otherwise.
pub(crate) fn notice_frame(k: &Kit, notice: &Notice) -> El {
    let c = &k.c;
    let (fill, edge) =
        if notice.error { (c.danger.bg, c.danger.border) } else { (c.layer_01, c.border_weak) };
    row().w_full().px(12.0).py(8.0).gap(12.0).radius(6.0).fill(fill).border(1.0, edge)
}
