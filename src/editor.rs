//! The editor for the Inbox sections: what Settings opens instead of a text file.
//!
//! It works on a draft. Nothing changes in the Inbox until Done, and Done is off while the draft
//! would not make a valid layout (the reason is shown). The result is stored as `sections.json`,
//! the same format the file always had.

use crate::sections::{self, Layout};
use crate::{App, BRAIN, PLUS, TRASH, icon_action, model_ink, panel};
use rune::{El, ScrollSpec, TextEdit, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, SegmentedControl, TextInput};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Selected {
    Nothing,
    Section(usize),
    Hide(usize),
}

pub struct EditSection {
    pub name: TextEdit,
    pub when: Vec<String>,
}

pub struct EditQuestion {
    pub id: TextEdit,
    pub ask: TextEdit,
    pub at: f32,
}

pub struct SectionsEditor {
    pub sections: Vec<EditSection>,
    pub questions: Vec<EditQuestion>,
    pub hide: Vec<Vec<String>>,
    pub selected: Selected,
    /// The next condition added is negated.
    pub negate: bool,
    /// Which kind of condition with a value is picked, and its value.
    pub kind: usize,
    pub value: TextEdit,
}

/// Conditions with a value, as the word starts and what they are called.
const KINDS: [(&str, &str); 6] = [
    ("lines>=", "lines ≥"),
    ("lines<", "lines <"),
    ("repo:", "repo"),
    ("author:", "author"),
    ("label:", "label"),
    ("title:", "title has"),
];

/// Conditions that need no value, grouped.
const WHO: [&str; 8] = [
    "asked:any",
    "asked:me",
    "asked:team",
    "external",
    "bot",
    "draft",
    "mine",
    "theirs",
];
const MINE: [&str; 4] = [
    "is:approved",
    "is:changes-requested",
    "is:unreviewed",
    "is:stale-draft",
];

const UP: rune::Icon = rune::Icon {
    name: "move-up",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M4 10l4-4 4 4"/>"#,
};
const DOWN: rune::Icon = rune::Icon {
    name: "move-down",
    view: 16.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M4 6l4 4 4-4"/>"#,
};

impl SectionsEditor {
    pub fn from_layout(layout: &Layout) -> Self {
        SectionsEditor {
            sections: layout
                .sections
                .iter()
                .map(|s| EditSection {
                    name: TextEdit::new(&s.name),
                    when: s.words.clone(),
                })
                .collect(),
            questions: layout
                .questions
                .iter()
                .map(|q| EditQuestion {
                    id: TextEdit::new(&q.id),
                    ask: TextEdit::new(&q.ask),
                    at: q.at,
                })
                .collect(),
            hide: layout.hide_words.clone(),
            selected: Selected::Nothing,
            negate: false,
            kind: 0,
            value: TextEdit::new(""),
        }
    }

    fn json(&self) -> String {
        let questions: Vec<_> = self
            .questions
            .iter()
            .map(|q| {
                serde_json::json!({ "id": q.id.text.trim(), "at": q.at, "ask": q.ask.text.trim() })
            })
            .collect();
        let sections: Vec<_> = self
            .sections
            .iter()
            .map(|s| serde_json::json!({ "name": s.name.text.trim(), "when": s.when }))
            .collect();
        serde_json::json!({ "questions": questions, "sections": sections, "hide": self.hide })
            .to_string()
    }

    /// What the draft makes, or why it cannot be used.
    pub fn check(&self) -> Result<Layout, String> {
        if self.sections.iter().any(|s| s.name.text.trim().is_empty()) {
            return Err("Every section needs a name.".into());
        }
        Layout::parse(&self.json())
    }

    fn question_ids(&self) -> Vec<String> {
        self.questions
            .iter()
            .map(|q| q.id.text.trim().to_owned())
            .filter(|id| !id.is_empty())
            .collect()
    }

    fn conditions_mut(&mut self) -> Option<&mut Vec<String>> {
        match self.selected {
            Selected::Section(i) => self.sections.get_mut(i).map(|s| &mut s.when),
            Selected::Hide(i) => self.hide.get_mut(i),
            Selected::Nothing => None,
        }
    }

    fn add_condition(&mut self, word: String) {
        let word = if self.negate {
            format!("!{word}")
        } else {
            word
        };
        self.negate = false;
        if let Some(list) = self.conditions_mut()
            && !list.contains(&word)
        {
            list.push(word);
        }
    }

    #[cfg(test)]
    pub fn add_valued_condition_for_test(&mut self) {
        self.add_valued_condition();
    }

    fn add_valued_condition(&mut self) {
        let value = self.value.text.trim().to_owned();
        if value.is_empty() {
            return;
        }
        let word = format!("{}{value}", KINDS[self.kind].0);
        self.value = TextEdit::new("");
        self.add_condition(word);
    }
}

fn ed(s: &mut App) -> &mut SectionsEditor {
    s.sections_editor
        .as_mut()
        .expect("the sections editor is open")
}

impl App {
    pub fn open_sections_editor(&mut self) {
        self.sections_editor = Some(SectionsEditor::from_layout(&self.layout));
    }

    /// Done: keep the draft as the sections when it is valid. Returns whether the editor closed.
    pub fn close_sections_editor(&mut self) -> bool {
        let Some(editor) = &self.sections_editor else {
            return true;
        };
        let Ok(layout) = editor.check() else {
            return false;
        };
        self.apply_layout(layout);
        self.sections_editor = None;
        true
    }

    /// Make a layout the sections, and keep it: as the file, or as no file when it is the defaults.
    /// A preview (and so every test) never touches the user's files.
    fn apply_layout(&mut self, layout: Layout) {
        let changed = layout.fingerprint != self.layout.fingerprint;
        if !self.preview
            && let Some(path) = sections::path()
        {
            if layout.fingerprint == Layout::default().fingerprint
                && layout.questions == Layout::default().questions
            {
                let _ = std::fs::remove_file(path);
            } else {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, layout.to_json());
            }
        }
        self.layout = layout;
        if changed {
            self.group_order = self.default_group_order();
            self.collapsed_groups.clear();
            if !self.preview
                && let Ok(db) = crate::Database::open_default()
            {
                let _ = db.set_setting("inbox_layout", &self.layout.fingerprint);
            }
            self.persist_groups();
        }
    }
}

fn chip(k: &Kit, label: String, model: bool) -> El {
    let c = &k.c;
    let mut inner = row().center_y().gap(4.0);
    if model {
        inner = inner.child(rune::icon(&BRAIN, 12.0).color(model_ink(k)));
    }
    inner.child(text(label).font(k.ty.small).color(c.text_base))
}

fn word_chips(
    k: &Kit,
    words: &[String],
    ids: &[String],
    remove: impl Fn(usize) -> Box<dyn Fn(&mut App)> + 'static,
) -> El {
    let c = &k.c;
    let mut chips = row().w_full().gap(6.0).flex_wrap();
    for (n, word) in words.iter().enumerate() {
        let is_question = ids.contains(&word.trim_start_matches('!').to_owned());
        let label = sections::describe(word);
        let on_press = remove(n);
        chips = chips.child(
            row()
                .key(format!("chip-{n}-{word}"))
                .role("button")
                .label(format!("Remove: {label}"))
                .help("Click to remove")
                .center_y()
                .gap(6.0)
                .pl(8.0)
                .pr(6.0)
                .py(3.0)
                .radius(10.0)
                .fill(c.layer_02)
                .cursor(rune::Cursor::Pointer)
                .hover(c.hover)
                .on_press(move |s: &mut App| on_press(s))
                .child(chip(k, label, is_question))
                .child(text("×").font(k.ty.small).color(c.text_faint)),
        );
    }
    chips
}

fn palette(editor: &SectionsEditor, k: &Kit) -> El {
    let c = &k.c;
    let add = |word: &str, model: bool| {
        let w = word.to_owned();
        let label = sections::describe(word);
        let mut b = row()
            .key(format!("add-{word}"))
            .role("button")
            .label(format!("Add: {label}"))
            .center_y()
            .gap(4.0)
            .px(8.0)
            .py(3.0)
            .radius(10.0)
            .border(1.0, c.border_weak)
            .cursor(rune::Cursor::Pointer)
            .hover(c.hover)
            .on_press(move |s: &mut App| ed(s).add_condition(w.clone()));
        if model {
            b = b.child(rune::icon(&BRAIN, 12.0).color(model_ink(k)));
        }
        b.child(text(label).font(k.ty.small).color(c.text_muted))
    };
    let group = |words: &[&str]| {
        let mut r = row().w_full().gap(6.0).flex_wrap();
        for w in words {
            r = r.child(add(w, false));
        }
        r
    };
    let mut questions = row().w_full().gap(6.0).flex_wrap();
    for id in editor.question_ids() {
        questions = questions.child(add(&id, true));
    }
    let kinds: Vec<&str> = KINDS.iter().map(|(_, label)| *label).collect();
    col()
        .w_full()
        .gap(8.0)
        .pt(4.0)
        .child(
            row().center_y().gap(8.0).child(
                Button::new("Not")
                    .variant(if editor.negate {
                        ButtonVariant::Contrast
                    } else {
                        ButtonVariant::Outline
                    })
                    .control_size(ControlSize::Small)
                    .help("The next condition you add is turned around")
                    .on_press(|s: &mut App| {
                        let e = ed(s);
                        e.negate = !e.negate;
                    })
                    .build(k)
                    .key("editor-negate"),
            ),
        )
        .child(group(&WHO))
        .child(group(&MINE))
        .child(questions)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(
                    SegmentedControl::new(&kinds, Some(editor.kind))
                        .label("Kind of condition")
                        .width(Some(430.0))
                        .on_pick(|s: &mut App, idx, _| ed(s).kind = idx)
                        .build(k)
                        .key("editor-kind"),
                )
                .child(
                    TextInput::new(&editor.value)
                        .placeholder("value")
                        .width(Some(150.0))
                        .on_edit(|s: &mut App| &mut ed(s).value)
                        .build(k)
                        .key("editor-value"),
                )
                .child(
                    Button::new("Add")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut App| ed(s).add_valued_condition())
                        .build(k)
                        .key("editor-add-valued"),
                ),
        )
}

fn section_row(editor: &SectionsEditor, k: &Kit, i: usize) -> El {
    let c = &k.c;
    let selected = editor.selected == Selected::Section(i);
    let last = editor.sections.len() - 1;
    let ids = editor.question_ids();
    let section = &editor.sections[i];
    let mut block = col()
        .key(format!("editor-section-{i}"))
        .w_full()
        .gap(8.0)
        .px(10.0)
        .py(8.0)
        .radius(8.0)
        .border(
            1.0,
            if selected {
                c.border_base
            } else {
                c.border_weak
            },
        )
        .fill(if selected { c.layer_01 } else { c.bg_base })
        .on_press(move |s: &mut App| ed(s).selected = Selected::Section(i))
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(
                    icon_action(&UP, "Move up")
                        .disabled(i == 0)
                        .on_press(move |s: &mut App| {
                            let e = ed(s);
                            e.sections.swap(i, i - 1);
                            e.selected = Selected::Section(i - 1);
                        })
                        .build(k)
                        .key(format!("section-up-{i}")),
                )
                .child(
                    icon_action(&DOWN, "Move down")
                        .disabled(i == last)
                        .on_press(move |s: &mut App| {
                            let e = ed(s);
                            e.sections.swap(i, i + 1);
                            e.selected = Selected::Section(i + 1);
                        })
                        .build(k)
                        .key(format!("section-down-{i}")),
                )
                .child(
                    TextInput::new(&section.name)
                        .placeholder("Name")
                        .label("Section name")
                        .width(Some(230.0))
                        .on_edit(move |s: &mut App| &mut ed(s).sections[i].name)
                        .build(k)
                        .key(format!("section-name-{i}")),
                )
                .child(el().grow(1.0))
                .child(
                    icon_action(&TRASH, "Remove section")
                        .on_press(move |s: &mut App| {
                            let e = ed(s);
                            e.sections.remove(i);
                            e.selected = Selected::Nothing;
                        })
                        .build(k)
                        .key(format!("section-remove-{i}")),
                ),
        )
        .child(word_chips(k, &section.when, &ids, move |n| {
            Box::new(move |s: &mut App| {
                ed(s).sections[i].when.remove(n);
            })
        }));
    if section.when.is_empty() {
        block = block.child(
            text("No conditions: nothing can match. Pick some below.")
                .font(k.ty.small)
                .color(c.text_faint),
        );
    }
    if selected {
        block = block.child(palette(editor, k));
    }
    block
}

fn hide_row(editor: &SectionsEditor, k: &Kit, i: usize) -> El {
    let c = &k.c;
    let selected = editor.selected == Selected::Hide(i);
    let ids = editor.question_ids();
    let mut block = col()
        .key(format!("editor-hide-{i}"))
        .w_full()
        .gap(8.0)
        .px(10.0)
        .py(8.0)
        .radius(8.0)
        .border(
            1.0,
            if selected {
                c.border_base
            } else {
                c.border_weak
            },
        )
        .fill(if selected { c.layer_01 } else { c.bg_base })
        .on_press(move |s: &mut App| ed(s).selected = Selected::Hide(i))
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(
                    text("Hide when all of these hold")
                        .font(k.ty.small)
                        .color(c.text_muted),
                )
                .child(el().grow(1.0))
                .child(
                    icon_action(&TRASH, "Remove this rule")
                        .on_press(move |s: &mut App| {
                            let e = ed(s);
                            e.hide.remove(i);
                            e.selected = Selected::Nothing;
                        })
                        .build(k)
                        .key(format!("hide-remove-{i}")),
                ),
        )
        .child(word_chips(k, &editor.hide[i], &ids, move |n| {
            Box::new(move |s: &mut App| {
                ed(s).hide[i].remove(n);
            })
        }));
    if selected {
        block = block.child(palette(editor, k));
    }
    block
}

fn question_row(editor: &SectionsEditor, k: &Kit, i: usize) -> El {
    let q = &editor.questions[i];
    row()
        .key(format!("editor-question-{i}"))
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            TextInput::new(&q.id)
                .placeholder("id")
                .label("Question id")
                .width(Some(120.0))
                .on_edit(move |s: &mut App| &mut ed(s).questions[i].id)
                .build(k)
                .key(format!("question-id-{i}")),
        )
        .child(
            TextInput::new(&q.ask)
                .placeholder("A yes/no question about a pull request")
                .label("Question")
                .width(Some(420.0))
                .on_edit(move |s: &mut App| &mut ed(s).questions[i].ask)
                .build(k)
                .key(format!("question-ask-{i}")),
        )
        .child(
            icon_action(&TRASH, "Remove question")
                .on_press(move |s: &mut App| {
                    ed(s).questions.remove(i);
                })
                .build(k)
                .key(format!("question-remove-{i}")),
        )
}

pub fn sections_editor_modal(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let Some(editor) = &s.sections_editor else {
        return el();
    };
    let verdict = editor.check();
    let close = rune_kit::IconButton::new(k.glyph.close)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label("Done")
        .help("Done (Esc)")
        .on_press(|s: &mut App| {
            s.close_sections_editor();
        })
        .build(k)
        .key("close-sections-editor");

    let mut list = col().w_full().gap(8.0);
    for i in 0..editor.sections.len() {
        list = list.child(section_row(editor, k, i));
    }
    list = list.child(
        row().child(
            Button::new("Add section")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .icon(&PLUS)
                .on_press(|s: &mut App| {
                    let e = ed(s);
                    e.sections.push(EditSection {
                        name: TextEdit::new("New section"),
                        when: vec!["asked:any".into()],
                    });
                    e.selected = Selected::Section(e.sections.len() - 1);
                })
                .build(k)
                .key("editor-add-section"),
        ),
    );

    let mut hidden = col().w_full().gap(8.0).child(
        text("Hidden from the Inbox")
            .font(k.ty.ui_medium)
            .color(c.text_base),
    );
    for i in 0..editor.hide.len() {
        hidden = hidden.child(hide_row(editor, k, i));
    }
    hidden = hidden.child(
        row().child(
            Button::new("Add a rule")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .icon(&PLUS)
                .on_press(|s: &mut App| {
                    let e = ed(s);
                    e.hide.push(vec!["external".into()]);
                    e.selected = Selected::Hide(e.hide.len() - 1);
                })
                .build(k)
                .key("editor-add-hide"),
        ),
    );

    let mut questions = col()
        .w_full()
        .gap(8.0)
        .child(
            text("Questions the model answers")
                .font(k.ty.ui_medium)
                .color(c.text_base),
        )
        .child(
            text("A section can use a question as a condition. Without any in use, no model is asked.")
                .font(k.ty.small)
                .color(c.text_faint),
        );
    for i in 0..editor.questions.len() {
        questions = questions.child(question_row(editor, k, i));
    }
    questions = questions.child(
        row().child(
            Button::new("Add a question")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .icon(&PLUS)
                .on_press(|s: &mut App| {
                    ed(s).questions.push(EditQuestion {
                        id: TextEdit::new(""),
                        ask: TextEdit::new(""),
                        at: 0.6,
                    });
                })
                .build(k)
                .key("editor-add-question"),
        ),
    );

    let content = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(18.0)
        .child(list)
        .child(panel(k).child(hidden))
        .child(panel(k).child(questions));

    let mut footer = row().w_full().px(16.0).py(12.0).center_y().gap(10.0).child(
        Button::new("Reset to defaults")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut App| {
                s.sections_editor = Some(SectionsEditor::from_layout(&Layout::default()));
            })
            .build(k)
            .key("editor-reset"),
    );
    footer = footer.child(
        match &verdict {
            Err(error) => text(error.clone())
                .wrap()
                .font(k.ty.small)
                .color(c.danger.fg),
            Ok(_) => text("").font(k.ty.small),
        }
        .key("editor-error"),
    );
    footer = footer.child(el().grow(1.0)).child(
        Button::new("Done")
            .variant(ButtonVariant::Contrast)
            .disabled(verdict.is_err())
            .on_press(|s: &mut App| {
                s.close_sections_editor();
            })
            .build(k)
            .key("editor-done"),
    );

    let dialog = col()
        .key("sections-editor-dialog")
        .role("dialog")
        .label("Inbox sections")
        .w_full()
        .max_w(820.0)
        .h_full()
        .max_h(720.0)
        .min_h(0.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(c.bg_base)
        .border(1.0, c.border_base)
        .child(
            row()
                .w_full()
                .padding(16.0)
                .center_y()
                .justify_between()
                .child(
                    col()
                        .gap(2.0)
                        .child(
                            text("Inbox sections")
                                .font(TextStyle::new(22.0, 650.0))
                                .color(c.text_base),
                        )
                        .child(
                            text("A pull request goes to the first section whose conditions all hold, so the order is the priority.")
                                .font(k.ty.small)
                                .color(c.text_muted),
                        ),
                )
                .child(close),
        )
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(content),
        )
        .child(el().w_full().h(1.0).fill(c.border_weak))
        .child(footer)
        .modal(true, |s: &mut App| {
            s.close_sections_editor();
        })
        .modal_click_outside(false);
    el().key("sections-editor-overlay")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .padding(24.0)
        .fill(c.scrim)
        .center()
        .child(dialog)
}
