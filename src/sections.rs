//! The Inbox sections.
//!
//! A pull request goes to the first section whose conditions all hold, so the order is the
//! priority, and it is the order on screen until the user drags a section elsewhere. Most
//! conditions are plain facts (who asked, size, title, author). A condition can also be a
//! question a model answers; those sections are sorted by AI and carry a brain.
//!
//! The sections are built in. The condition language is small: `any`, `asked:me`, `asked:team`,
//! `asked:any`, `is:approved`, `is:changes-requested`, `is:unreviewed`, `is:stale-draft`, `mine`,
//! `theirs`, `external`, `bot`, `draft`, `lines>=N`, `repo:`, `author:`, `label:`, `title:`, or a
//! question id; `!` in front negates.

use crate::model::{PrAttention, PullRequest};
use serde::Deserialize;
use std::collections::HashMap;

const DEFAULT_JSON: &str = r#"{
  "questions": [
    { "id": "urgent", "at": 0.6, "ask": "Does the title or description itself state time pressure: a deadline, a customer or release waiting on it, an outage or production incident, or the words urgent, asap, hotfix or blocker? An ordinary bug fix, feature or improvement is NOT urgent." },
    { "id": "reading", "at": 0.67, "ask": "How much reading does a reviewer need before approving this pull request? Judge from the changed file paths and the description.",
      "scale": [
        "Approve on sight: mechanical or trivial (typos, docs, renames, generated code, dependency or version bumps, snapshots)",
        "Quick read: small, self-contained, low-risk change",
        "Careful read: changes logic or behaviour, touches several areas, or needs context to judge",
        "Deep review: security, auth, payments, data migrations, infrastructure, or a large or cross-cutting change"
      ] }
  ],
  "sections": [
    { "name": "Merge these", "when": ["is:approved"] },
    { "name": "Hotfixes", "when": ["asked:me", "title:hotfix"] },
    { "name": "Security", "when": ["asked:me", "title:security"] },
    { "name": "Do first", "when": ["asked:me", "urgent"] },
    { "name": "Read carefully", "when": ["asked:me", "reading>=0.67"] },
    { "name": "Approve at will", "when": ["asked:me", "reading<0.2", "lines<400"] },
    { "name": "Big reviews", "when": ["asked:me", "lines>=600"] },
    { "name": "Review these", "when": ["asked:me"] },
    { "name": "Address feedback", "when": ["is:changes-requested"] },
    { "name": "Team reviews", "when": ["asked:team"] },
    { "name": "Get a reviewer", "when": ["is:unreviewed"] },
    { "name": "Finish or close", "when": ["is:stale-draft"] }
  ]
}
"#;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Question {
    pub id: String,
    pub ask: String,
    /// The probability of yes from which the answer counts as yes.
    #[serde(default = "default_at")]
    pub at: f32,
    /// Ordered levels, lowest first. With levels it is a score question: the answer is the
    /// position between 0 (first level) and 1 (last), and `at` is where it counts as high.
    #[serde(default)]
    pub scale: Vec<String>,
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
    /// A score question's position compared with a number: `reading>=0.67`.
    Level(String, Cmp, f32),
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
            .any(|c| matches!(c.term, Term::Question(_) | Term::Level(..)))
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
    /// Changes whenever the sections do, so a saved screen order made for other ones is dropped.
    pub fingerprint: String,
}

#[derive(Deserialize)]
struct RawSection {
    name: String,
    when: Vec<String>,
}

#[derive(Deserialize)]
struct RawLayout {
    #[serde(default)]
    questions: Option<Vec<Question>>,
    #[serde(default)]
    sections: Option<Vec<RawSection>>,
}

impl Layout {
    pub fn default() -> Layout {
        Layout::parse(DEFAULT_JSON).expect("the built-in sections parse")
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
            let words = raw.when;
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
        Ok(Layout {
            questions,
            sections,
            fingerprint: format!("{:016x}", fnv(&key)),
        })
    }

    /// The first section the pull request belongs to: the order is the priority.
    pub fn section_of(&self, subject: &Subject) -> Option<usize> {
        self.sections
            .iter()
            .position(|s| s.when.iter().all(|c| self.holds(c, subject)))
    }

    /// The questions something in the layout depends on. Without any, no model is ever asked.
    pub fn needed_questions(&self) -> Vec<Question> {
        let mut used: Vec<&str> = Vec::new();
        for cond in self.sections.iter().flat_map(|s| &s.when) {
            if let Term::Question(id) | Term::Level(id, ..) = &cond.term
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
            Term::Level(id, cmp, n) => subject.answers.get(id).is_some_and(|p| match cmp {
                Cmp::Ge => p >= n,
                Cmp::Gt => p > n,
                Cmp::Le => p <= n,
                Cmp::Lt => p < n,
            }),
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
            } else if let Some((id, cmp, number)) = [">=", "<=", ">", "<"].iter().find_map(|op| {
                let (left, right) = rest.split_once(op)?;
                let cmp = match *op {
                    ">=" => Cmp::Ge,
                    "<=" => Cmp::Le,
                    ">" => Cmp::Gt,
                    _ => Cmp::Lt,
                };
                question_ids
                    .contains(&left.trim())
                    .then(|| {
                        (
                            left.trim().to_owned(),
                            cmp,
                            right.trim().parse::<f32>().ok(),
                        )
                    })
                    .and_then(|(id, cmp, n)| n.map(|n| (id, cmp, n)))
            }) {
                Term::Level(id, cmp, number)
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
    fn the_defaults_parse_and_ask_two_questions() {
        let layout = Layout::default();
        assert_eq!(layout.sections.len(), 12);
        assert_eq!(layout.sections[0].name, "Merge these");
        assert_eq!(layout.needed_questions().len(), 2);
        assert!(layout.sections[3].uses_model() && !layout.sections[0].uses_model());
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
        pr.additions = 900;
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &none)).as_deref(),
            Some("Big reviews")
        );
        // Big is never approved on sight, whatever the model says.
        let skim = HashMap::from([("reading".to_owned(), 0.05)]);
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &skim)).as_deref(),
            Some("Big reviews")
        );
        pr.additions = 40;
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &skim)).as_deref(),
            Some("Approve at will")
        );
        let deep = HashMap::from([("reading".to_owned(), 0.9)]);
        assert_eq!(
            name(&layout, &subject(&pr, assigned, &deep)).as_deref(),
            Some("Read carefully")
        );
        // Urgent beats both.
        let both = HashMap::from([("reading".to_owned(), 0.05), ("urgent".to_owned(), 0.8)]);
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
    fn conditions_can_name_questions_repos_and_negations() {
        let layout = Layout::parse(
            r#"{
              "questions": [{ "id": "payments", "ask": "Does it change how money moves?" }],
              "sections": [
                { "name": "Money", "when": ["asked:any", "payments"] },
                { "name": "Core", "when": ["asked:any", "repo:saleor/*", "!bot"] },
                { "name": "Everything else", "when": ["any"] }
              ]
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
    }

    #[test]
    fn mistakes_are_named_and_the_defaults_are_kept() {
        let bad = |json: &str| Layout::parse(json).unwrap_err();
        assert!(bad(r#"{"sections":[{"name":"A","when":["urgnt"]}]}"#).contains("urgnt"));
        assert!(bad(r#"{"sections":[{"name":"A","when":[]}]}"#).contains("no conditions"));
        assert!(
            bad(r#"{"sections":[{"name":"A","when":["any"]},{"name":"A","when":["any"]}]}"#)
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
