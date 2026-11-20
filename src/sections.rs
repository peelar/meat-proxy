//! The Inbox sections, as configuration.
//!
//! `~/.config/meat-proxy/sections.json` lists the sections top to bottom. A pull request goes to
//! the first section whose conditions all hold, so the order is the priority, and it is the
//! default order on screen too (dragging changes only the screen). `questions` are yes/no
//! questions a model answers; a section asks for one by naming it. `hide` removes pull requests
//! from the Inbox tab (they stay under Theirs and All).
//!
//! A condition is a word, `!` in front of it negates it:
//!
//! | condition              | holds when                                                          |
//! |------------------------|----------------------------------------------------------------------|
//! | `any`                  | always                                                               |
//! | `asked:me`             | your review was requested by name                                    |
//! | `asked:team`           | your review was requested through a team                             |
//! | `asked:any`            | either                                                               |
//! | `is:approved`          | your PR is approved and ready to merge                               |
//! | `is:changes-requested` | your PR has changes requested                                        |
//! | `is:unreviewed`        | your PR waited for a reviewer longer than your reminder              |
//! | `is:stale-draft`       | your draft sat untouched longer than your reminder                   |
//! | `mine` / `theirs`      | who wrote it                                                         |
//! | `external` / `bot`     | an outside contributor / an automation                               |
//! | `draft`                | it is a draft                                                        |
//! | `lines>=600`           | changed lines (`>=`, `>`, `<=`, `<`)                                 |
//! | `repo:org/*`           | the repository, with `*` as a wildcard                               |
//! | `author:name*`         | the author, with `*` as a wildcard                                   |
//! | `label:security`       | it carries that label                                                |
//! | `title:hotfix`         | the title contains the text                                          |
//! | a question id          | the model answers yes (at or above the question's `at`)              |

use crate::model::{PrAttention, PullRequest};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

const DEFAULT_JSON: &str = r#"{
  "questions": [
    { "id": "trivial", "at": 0.75, "ask": "Is it a typo, docs, formatting, a rename, a dependency bump, a translation or generated code, safe to skim without reading the diff?" },
    { "id": "urgent", "at": 0.6, "ask": "Does the title or description itself state time pressure: a deadline, a customer or release waiting on it, an outage or production incident, or the words urgent, asap, hotfix or blocker? An ordinary bug fix, feature or improvement is NOT urgent." },
    { "id": "risky", "at": 0.6, "ask": "Is the main change itself in authentication, authorization, payments or money, data migrations, security, or deploy and infrastructure config? Judge the change, not words mentioned in passing." }
  ],
  "sections": [
    { "name": "Merge these", "when": ["is:approved"] },
    { "name": "Do first", "when": ["asked:any", "urgent"] },
    { "name": "Read carefully", "when": ["asked:any", "risky"] },
    { "name": "Approve at will", "when": ["asked:any", "trivial"] },
    { "name": "Big reviews", "when": ["asked:any", "lines>=600"] },
    { "name": "Review these", "when": ["asked:me"] },
    { "name": "Address feedback", "when": ["is:changes-requested"] },
    { "name": "Team reviews", "when": ["asked:team"] },
    { "name": "Get a reviewer", "when": ["is:unreviewed"] },
    { "name": "Finish or close", "when": ["is:stale-draft"] }
  ],
  "hide": []
}
"#;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Question {
    pub id: String,
    pub ask: String,
    /// The probability of yes from which the answer counts as yes.
    #[serde(default = "default_at")]
    pub at: f32,
}

fn default_at() -> f32 {
    0.6
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmp {
    Ge,
    Gt,
    Le,
    Lt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Is {
    Approved,
    ChangesRequested,
    Unreviewed,
    StaleDraft,
}

#[derive(Clone, Debug, PartialEq)]
enum Term {
    Any,
    Mine,
    Theirs,
    AskedMe,
    AskedTeam,
    AskedAny,
    Is(Is),
    External,
    Bot,
    Draft,
    Lines(Cmp, u32),
    Repo(String),
    Author(String),
    Label(String),
    Title(String),
    Question(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cond {
    not: bool,
    term: Term,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    when: Vec<Cond>,
}

impl Section {
    /// Whether the model has a say in who belongs here.
    pub fn uses_model(&self) -> bool {
        self.when
            .iter()
            .any(|c| matches!(c.term, Term::Question(_)))
    }
}

/// What a condition is checked against.
pub struct Subject<'a> {
    pub pr: &'a PullRequest,
    pub attention: PrAttention,
    /// What the model (or a rule) answered, by question id; empty when sorting by rules alone.
    pub answers: &'a HashMap<String, f32>,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub questions: Vec<Question>,
    pub sections: Vec<Section>,
    hide: Vec<Vec<Cond>>,
    /// Changes whenever the sections do, so a saved screen order made for other ones is dropped.
    pub fingerprint: String,
    /// The file the layout came from, or None for the defaults.
    pub custom: bool,
    /// Why the file was not used, if it was not.
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Terms {
    One(String),
    Many(Vec<String>),
}

impl Terms {
    fn into_vec(self) -> Vec<String> {
        match self {
            Terms::One(one) => vec![one],
            Terms::Many(many) => many,
        }
    }
}

#[derive(Deserialize)]
struct RawSection {
    name: String,
    when: Terms,
}

#[derive(Deserialize)]
struct RawLayout {
    #[serde(default)]
    questions: Option<Vec<Question>>,
    #[serde(default)]
    sections: Option<Vec<RawSection>>,
    #[serde(default)]
    hide: Vec<Terms>,
}

/// `~/.config/meat-proxy/sections.json`, or the file `MEAT_PROXY_SECTIONS` names.
pub fn path() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("MEAT_PROXY_SECTIONS") {
        return Some(PathBuf::from(custom));
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/meat-proxy/sections.json"))
}

pub fn default_json() -> &'static str {
    DEFAULT_JSON
}

impl Layout {
    pub fn default() -> Layout {
        Layout::parse(DEFAULT_JSON).expect("the built-in sections parse")
    }

    /// The user's file, else the defaults. A file that does not parse is reported, not obeyed.
    pub fn load() -> Layout {
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return Layout::default();
        };
        match Layout::parse(&text) {
            Ok(mut layout) => {
                layout.custom = true;
                layout
            }
            Err(error) => Layout {
                error: Some(error),
                ..Layout::default()
            },
        }
    }

    pub fn parse(text: &str) -> Result<Layout, String> {
        let raw: RawLayout =
            serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
        let defaults: RawLayout = serde_json::from_str(DEFAULT_JSON).expect("built-in JSON");
        // A file may leave out `questions` (the defaults stay) but not describe sections it can't place.
        let questions = raw.questions.or(defaults.questions).unwrap_or_default();
        let raw_sections = raw.sections.or(defaults.sections).unwrap_or_default();
        for (n, q) in questions.iter().enumerate() {
            if q.id.trim().is_empty() || q.ask.trim().is_empty() {
                return Err(format!("question {} needs an id and an ask", n + 1));
            }
            if is_reserved(&q.id) || questions[..n].iter().any(|o| o.id == q.id) {
                return Err(format!(
                    "question id \"{}\" is reserved or used twice",
                    q.id
                ));
            }
        }
        let ids: Vec<&str> = questions.iter().map(|q| q.id.as_str()).collect();
        let mut sections = Vec::new();
        let mut key = String::new();
        for raw in raw_sections {
            let words = raw.when.into_vec();
            if words.is_empty() {
                return Err(format!(
                    "section \"{}\" has no conditions; use \"any\"",
                    raw.name
                ));
            }
            let when = words
                .iter()
                .map(|w| parse_cond(w, &ids))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("section \"{}\": {e}", raw.name))?;
            if sections.iter().any(|s: &Section| s.name == raw.name) {
                return Err(format!("section \"{}\" appears twice", raw.name));
            }
            key.push_str(&format!("{}={};", raw.name, words.join("&")));
            sections.push(Section {
                name: raw.name,
                when,
            });
        }
        if sections.len() > 200 {
            return Err("at most 200 sections".into());
        }
        let hide = raw
            .hide
            .into_iter()
            .map(|terms| {
                terms
                    .into_vec()
                    .iter()
                    .map(|w| parse_cond(w, &ids))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("hide: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Layout {
            questions,
            sections,
            hide,
            fingerprint: format!("{:016x}", fnv(&key)),
            custom: false,
            error: None,
        })
    }

    /// The first section the pull request belongs to: the order is the priority.
    pub fn section_of(&self, subject: &Subject) -> Option<usize> {
        self.sections
            .iter()
            .position(|s| s.when.iter().all(|c| self.holds(c, subject)))
    }

    /// Whether a `hide` entry takes it out of the Inbox.
    pub fn hides(&self, subject: &Subject) -> bool {
        self.hide
            .iter()
            .any(|all| !all.is_empty() && all.iter().all(|c| self.holds(c, subject)))
    }

    /// The questions something in the layout depends on. Without any, no model is ever asked.
    pub fn needed_questions(&self) -> Vec<Question> {
        let mut used: Vec<&str> = Vec::new();
        for cond in self
            .sections
            .iter()
            .flat_map(|s| &s.when)
            .chain(self.hide.iter().flatten())
        {
            if let Term::Question(id) = &cond.term
                && !used.contains(&id.as_str())
            {
                used.push(id);
            }
        }
        self.questions
            .iter()
            .filter(|q| used.contains(&q.id.as_str()))
            .cloned()
            .collect()
    }

    /// The questions of `subject.answers` that came out yes, for a short label.
    pub fn fired<'a>(&'a self, answers: &HashMap<String, f32>) -> Vec<&'a str> {
        self.questions
            .iter()
            .filter(|q| answers.get(&q.id).is_some_and(|p| *p >= q.at))
            .map(|q| q.id.as_str())
            .collect()
    }

    fn holds(&self, cond: &Cond, subject: &Subject) -> bool {
        let pr = subject.pr;
        let yes = match &cond.term {
            Term::Any => true,
            Term::Mine => pr.ownership == crate::model::Ownership::Mine,
            Term::Theirs => pr.ownership == crate::model::Ownership::Theirs,
            Term::AskedMe => subject.attention == PrAttention::IncomingAssigned,
            Term::AskedTeam => subject.attention == PrAttention::IncomingViaTeam,
            Term::AskedAny => matches!(
                subject.attention,
                PrAttention::IncomingAssigned | PrAttention::IncomingViaTeam
            ),
            Term::Is(Is::Approved) => subject.attention == PrAttention::MyOpenApproved,
            Term::Is(Is::ChangesRequested) => {
                subject.attention == PrAttention::MyOpenChangesRequested
            }
            Term::Is(Is::Unreviewed) => subject.attention == PrAttention::MyOpenNeedsNudge,
            Term::Is(Is::StaleDraft) => subject.attention == PrAttention::MyDraftNeedsNudge,
            Term::External => pr.external,
            Term::Bot => is_bot(&pr.author),
            Term::Draft => pr.is_draft,
            Term::Lines(cmp, n) => {
                let lines = pr.additions + pr.deletions;
                match cmp {
                    Cmp::Ge => lines >= *n,
                    Cmp::Gt => lines > *n,
                    Cmp::Le => lines <= *n,
                    Cmp::Lt => lines < *n,
                }
            }
            Term::Repo(glob) => wildcard(glob, &pr.repo),
            Term::Author(glob) => wildcard(glob, &pr.author),
            Term::Label(label) => pr.labels.iter().any(|l| l.eq_ignore_ascii_case(label)),
            Term::Title(text) => pr.title.to_lowercase().contains(&text.to_lowercase()),
            Term::Question(id) => {
                let at = self
                    .questions
                    .iter()
                    .find(|q| q.id == *id)
                    .map_or(0.6, |q| q.at);
                subject.answers.get(id).is_some_and(|p| *p >= at)
            }
        };
        yes != cond.not
    }
}

pub fn is_bot(author: &str) -> bool {
    let author = author.to_ascii_lowercase();
    author.ends_with("[bot]")
        || author.contains("dependabot")
        || author.contains("renovate")
        || author == "github-actions"
}

fn is_reserved(id: &str) -> bool {
    matches!(id, "any" | "mine" | "theirs" | "external" | "bot" | "draft")
        || id.contains(':')
        || id.contains('>')
        || id.contains('<')
        || id.starts_with('!')
}

fn parse_cond(word: &str, question_ids: &[&str]) -> Result<Cond, String> {
    let word = word.trim();
    let (not, rest) = match word.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, word),
    };
    let lower = rest.to_ascii_lowercase();
    let term = match lower.as_str() {
        "any" => Term::Any,
        "mine" => Term::Mine,
        "theirs" => Term::Theirs,
        "external" => Term::External,
        "bot" => Term::Bot,
        "draft" => Term::Draft,
        "asked:me" => Term::AskedMe,
        "asked:team" => Term::AskedTeam,
        "asked:any" => Term::AskedAny,
        "is:approved" => Term::Is(Is::Approved),
        "is:changes-requested" => Term::Is(Is::ChangesRequested),
        "is:unreviewed" => Term::Is(Is::Unreviewed),
        "is:stale-draft" => Term::Is(Is::StaleDraft),
        _ => {
            if let Some(size) = lower.strip_prefix("lines") {
                let (cmp, number) = [
                    (">=", Cmp::Ge),
                    ("<=", Cmp::Le),
                    (">", Cmp::Gt),
                    ("<", Cmp::Lt),
                ]
                .iter()
                .find_map(|(op, cmp)| size.strip_prefix(op).map(|n| (*cmp, n)))
                .ok_or_else(|| format!("\"{word}\": write lines>=600, lines<50 and so on"))?;
                Term::Lines(
                    cmp,
                    number
                        .trim()
                        .parse()
                        .map_err(|_| format!("\"{word}\": the number of lines is not a number"))?,
                )
            } else if let Some(value) = rest.split_once(':').map(|(_, v)| v.trim().to_owned()) {
                match lower.split(':').next().unwrap_or_default() {
                    "repo" => Term::Repo(value),
                    "author" => Term::Author(value),
                    "label" => Term::Label(value),
                    "title" => Term::Title(value),
                    _ => return Err(format!("\"{word}\" is not a condition")),
                }
            } else if question_ids.contains(&rest) {
                Term::Question(rest.to_owned())
            } else {
                return Err(format!(
                    "\"{word}\" is not a condition or the id of a question"
                ));
            }
        }
    };
    Ok(Cond { not, term })
}

/// `*` matches any run of characters; the rest compares without regard to case.
fn wildcard(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.to_lowercase(), text.to_lowercase());
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text.as_str();
    for (n, part) in parts.iter().enumerate() {
        if n == 0 {
            match rest.strip_prefix(part) {
                Some(after) => rest = after,
                None => return false,
            }
        } else if n == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

pub fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject<'a>(
        pr: &'a PullRequest,
        attention: PrAttention,
        answers: &'a HashMap<String, f32>,
    ) -> Subject<'a> {
        Subject {
            pr,
            attention,
            answers,
        }
    }

    fn review() -> PullRequest {
        let mut pr = crate::github::demo_prs().remove(0);
        pr.additions = 40;
        pr.deletions = 10;
        pr.external = false;
        pr.is_draft = false;
        pr
    }

    #[test]
    fn the_defaults_parse_and_ask_three_questions() {
        let layout = Layout::default();
        assert_eq!(layout.sections.len(), 10);
        assert_eq!(layout.sections[0].name, "Merge these");
        assert_eq!(layout.needed_questions().len(), 3);
        assert!(layout.sections[1].uses_model() && !layout.sections[0].uses_model());
    }

    #[test]
    fn the_first_matching_section_wins() {
        let layout = Layout::default();
        let none = HashMap::new();
        let mut pr = review();
        let assigned = PrAttention::IncomingAssigned;
        let name = |layout: &Layout, s: &Subject| {
            layout
                .section_of(s)
                .map(|i| layout.sections[i].name.clone())
        };
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &none)).as_deref(),
            Some("Review these")
        );
        // Big and trivial: trivial is listed first, so it wins.
        pr.additions = 900;
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &none)).as_deref(),
            Some("Big reviews")
        );
        let trivial = HashMap::from([("trivial".to_owned(), 0.9)]);
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &trivial)).as_deref(),
            Some("Approve at will")
        );
        // Urgent beats trivial.
        let both = HashMap::from([("trivial".to_owned(), 0.9), ("urgent".to_owned(), 0.8)]);
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &both)).as_deref(),
            Some("Do first")
        );
        // Team requests land in their own section.
        assert_eq!(
            name(
                &layout,
                &subject(&review(), PrAttention::IncomingViaTeam, &none)
            )
            .as_deref(),
            Some("Team reviews")
        );
    }

    #[test]
    fn a_file_can_add_questions_sections_and_hide_rules() {
        let layout = Layout::parse(
            r#"{
              "questions": [{ "id": "payments", "ask": "Does it change how money moves?" }],
              "sections": [
                { "name": "Money", "when": ["asked:any", "payments"] },
                { "name": "Saleor core", "when": ["asked:any", "repo:saleor/*", "!bot"] },
                { "name": "Everything else", "when": "any" }
              ],
              "hide": ["external", ["bot", "lines<10"]]
            }"#,
        )
        .unwrap();
        assert_eq!(layout.needed_questions().len(), 1);
        let none = HashMap::new();
        let mut pr = review();
        pr.repo = "saleor/saleor".into();
        let att = PrAttention::IncomingAssigned;
        assert_eq!(layout.section_of(&subject(&pr, att, &none)), Some(1));
        let yes = HashMap::from([("payments".to_owned(), 0.7)]);
        assert_eq!(layout.section_of(&subject(&pr, att, &yes)), Some(0));
        pr.repo = "other/repo".into();
        assert_eq!(layout.section_of(&subject(&pr, att, &none)), Some(2));
        // Hide: anyone outside, or a bot with a tiny change.
        assert!(!layout.hides(&subject(&pr, att, &none)));
        pr.external = true;
        assert!(layout.hides(&subject(&pr, att, &none)));
        pr.external = false;
        pr.author = "dependabot".into();
        pr.additions = 2;
        pr.deletions = 1;
        assert!(layout.hides(&subject(&pr, att, &none)));
    }

    #[test]
    fn mistakes_are_named_and_the_defaults_are_kept() {
        let bad = |json: &str| Layout::parse(json).unwrap_err();
        assert!(bad(r#"{"sections":[{"name":"A","when":["urgnt"]}]}"#).contains("urgnt"));
        assert!(bad(r#"{"sections":[{"name":"A","when":[]}]}"#).contains("no conditions"));
        assert!(
            bad(r#"{"sections":[{"name":"A","when":"any"},{"name":"A","when":"any"}]}"#)
                .contains("twice")
        );
        assert!(bad(r#"{"sections":[{"name":"A","when":["lines>>5"]}]}"#).contains("lines"));
        assert!(bad("{nope").contains("JSON"));
    }

    #[test]
    fn wildcards_and_the_fingerprint() {
        assert!(wildcard("saleor/*", "Saleor/saleor-dashboard"));
        assert!(wildcard("*-dashboard", "saleor/saleor-dashboard"));
        assert!(wildcard("a*c*e", "abcde") && !wildcard("a*c*e", "abcd"));
        assert!(!wildcard("saleor", "saleor/x"));
        let a = Layout::default();
        let b = Layout::parse(&DEFAULT_JSON.replace("Do first", "Now")).unwrap();
        assert_ne!(a.fingerprint, b.fingerprint);
        assert_eq!(a.fingerprint, Layout::default().fingerprint);
    }
}
