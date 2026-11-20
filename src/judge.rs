//! Judgments: yes/no questions a language model answers about a pull request, with probabilities.
//!
//! The model never writes text. It is asked the questions the sections need in one call and
//! answers `Y N N`; the probabilities come from the token log-probabilities at each answer, so
//! "92% trivial" is the model's own odds, not a guess dressed up as a number. Everything else
//! (merge readiness, who the author is, how long it has waited) stays in code: the model only
//! answers what needs reading. Which questions exist, and what a "yes" does, is in `sections`.
//!
//! Two kinds of endpoint can answer. A decision model (TypeSafe's Jev, `ggml-org/Kev-4B-GGUF` under
//! llama.cpp) takes a state and typed questions at `/v1/systemone` and returns probabilities
//! natively: `provider: "systemone"`. Any OpenAI-compatible chat model that returns token
//! log-probabilities can stand in for one: `provider: "chat"`, the default.
//!
//! A model costs money and time, so it is the last resort. A pull request goes to it only when
//! it asks something of the user, lives in a repository whose text may leave the machine, has
//! changed since it was last judged, and no cheap rule already knows the answer.

use crate::model::{PrAttention, PullRequest};
use crate::sections::Question;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

const DEFAULT_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";
const BODY_CHARS: usize = 1400;
const PARALLEL_CALLS: usize = 4;
/// Raise it when the questions or the rules change, so verdicts made under the old ones are redone.
const PROMPT_VERSION: &str = "2";

#[derive(Clone, Debug, PartialEq)]
pub struct Judgment {
    /// The probability of yes for each question, by question id.
    pub answers: HashMap<String, f32>,
    /// "rules" when a cheap rule decided, else the model's name.
    pub by: String,
    /// What the pull request and the questions looked like when it was judged; a change means
    /// judging again.
    pub fingerprint: String,
}

impl Judgment {
    pub fn summary(&self) -> String {
        let mut answers: Vec<_> = self.answers.iter().collect();
        answers.sort_by(|a, b| a.0.cmp(b.0));
        let odds: Vec<String> = answers
            .iter()
            .map(|(id, p)| format!("{id} {:.0}%", **p * 100.0))
            .collect();
        format!(
            "Alien intelligence says: {}  ·  {}",
            odds.join("  ·  "),
            self.by
        )
    }
}

/// Any OpenAI-compatible chat endpoint: OpenRouter, OpenAI, a local llama.cpp or Ollama server.
#[derive(Clone, Debug, Deserialize)]
pub struct JudgeConfig {
    #[serde(default = "default_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// "chat" (an OpenAI-compatible chat model read through its log-probabilities, the default) or
    /// "systemone" (a decision model's `/systemone` endpoint, which returns probabilities itself).
    #[serde(default = "default_provider")]
    pub provider: String,
    /// What a million input tokens cost, for the spending cap, when the endpoint does not say.
    #[serde(default)]
    pub input_usd_per_million: f64,
    /// Send text from private repositories too. Off: they are never sent.
    #[serde(default)]
    pub include_private: bool,
    /// The most pull requests one sync sends to the model.
    #[serde(default = "default_max_per_sync")]
    pub max_per_sync: usize,
    /// Stop asking once this much has been spent in one run of the app.
    #[serde(default = "default_max_spend")]
    pub max_spend_usd: f64,
}

fn default_url() -> String {
    DEFAULT_URL.into()
}
fn default_provider() -> String {
    "chat".into()
}
fn default_model() -> String {
    DEFAULT_MODEL.into()
}
fn default_max_per_sync() -> usize {
    30
}
fn default_max_spend() -> f64 {
    0.25
}

impl JudgeConfig {
    /// `~/.config/meat-proxy/judge.json`, overridden by `MEAT_PROXY_JUDGE_URL`, `_KEY` and `_MODEL`.
    /// Without a key there is nothing to ask, and the inbox runs on rules alone.
    pub fn load() -> Option<Self> {
        let from_file = std::env::var_os("HOME")
            .map(|home| std::path::PathBuf::from(home).join(".config/meat-proxy/judge.json"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<JudgeConfig>(&text).ok());
        let mut cfg = from_file.unwrap_or_else(|| JudgeConfig {
            base_url: default_url(),
            api_key: String::new(),
            model: default_model(),
            provider: default_provider(),
            input_usd_per_million: 0.0,
            include_private: false,
            max_per_sync: default_max_per_sync(),
            max_spend_usd: default_max_spend(),
        });
        if let Ok(url) = std::env::var("MEAT_PROXY_JUDGE_URL") {
            cfg.base_url = url;
        }
        if let Ok(key) = std::env::var("MEAT_PROXY_JUDGE_KEY") {
            cfg.api_key = key;
        }
        if let Ok(model) = std::env::var("MEAT_PROXY_JUDGE_MODEL") {
            cfg.model = model;
        }
        if let Ok(provider) = std::env::var("MEAT_PROXY_JUDGE_PROVIDER") {
            cfg.provider = provider;
        }
        // A local server needs no key; a hosted one without a key would only fail.
        let local = cfg.base_url.contains("localhost") || cfg.base_url.contains("127.0.0.1");
        (local || !cfg.api_key.trim().is_empty()).then_some(cfg)
    }
}

/// Whether a pull request is worth asking about at all. Mine, drafts, team requests and ones only
/// watched need no judgment about how to review them.
pub fn is_candidate(pr: &PullRequest, attention: PrAttention, cfg: &JudgeConfig) -> bool {
    // Only requests that name you: the Inbox is about those and your own pull requests.
    attention == PrAttention::IncomingAssigned
        && !pr.is_draft
        && (cfg.include_private || !pr.private_repo)
}

/// What cheap rules already know. A hit costs nothing and is final.
pub fn shortcut(pr: &PullRequest, questions_key: &str) -> Option<Judgment> {
    rule_judgment(pr).map(|j| Judgment {
        fingerprint: fingerprint(pr, questions_key),
        ..j
    })
}

/// The same verdict without the fingerprint, cheap enough to compute whenever a list is drawn:
/// it is what the inbox sorts by when no model is involved. Bots, routine titles and one-line
/// changes are trivial; a question nobody answered counts as no.
pub fn rule_judgment(pr: &PullRequest) -> Option<Judgment> {
    let title = pr.title.to_ascii_lowercase();
    let routine = title.starts_with("bump ")
        || title.starts_with("chore(deps")
        || title.starts_with("docs:")
        || title.starts_with("docs(")
        || title.starts_with("typo");
    let tiny = pr.additions + pr.deletions <= 2;
    let bot = crate::sections::is_bot(&pr.author);
    (bot || routine || tiny).then(|| Judgment {
        answers: HashMap::from([
            (
                "trivial".to_owned(),
                if bot || routine { 0.95 } else { 0.85 },
            ),
            ("urgent".to_owned(), 0.03),
            ("risky".to_owned(), 0.05),
        ]),
        by: "rules".into(),
        fingerprint: String::new(),
    })
}

/// A key for the questions in use: asking different ones makes old answers stale.
pub fn questions_key(questions: &[Question]) -> String {
    let mut text = String::from(PROMPT_VERSION);
    for q in questions {
        text.push_str(&format!("|{}:{}", q.id, q.ask));
    }
    format!("{:016x}", crate::sections::fnv(&text))
}

/// What a judgment depends on: the text, the size and the questions, not the comments or the clock.
pub fn fingerprint(pr: &PullRequest, questions_key: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes.iter().chain(&[0u8]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    feed(questions_key.as_bytes());
    feed(pr.repo.as_bytes());
    feed(pr.title.as_bytes());
    feed(pr.author.as_bytes());
    feed(clip(&pr.body, BODY_CHARS).as_bytes());
    feed(pr.labels.join(",").as_bytes());
    feed(&pr.additions.to_le_bytes());
    feed(&pr.deletions.to_le_bytes());
    format!("{hash:016x}")
}

fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

/// The description without the template noise: comments, images and runs of blank lines.
fn tidy(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
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
    let lines: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("!["))
        .collect();
    clip(&lines.join("\n"), BODY_CHARS)
}

/// What a model is told about the pull request, without the questions.
fn facts(pr: &PullRequest) -> String {
    let labels = if pr.labels.is_empty() {
        "none".to_owned()
    } else {
        pr.labels.join(", ")
    };
    let body = tidy(&pr.body);
    format!(
        "Pull request in {repo}\nTitle: {title}\nAuthor: {author}{external}\nLabels: {labels}\nSize: +{add} -{del}\nBranch: {branch}\nDescription:\n{body}",
        repo = pr.repo,
        title = pr.title,
        author = pr.author,
        external = if pr.external {
            " (outside the organization)"
        } else {
            ""
        },
        add = pr.additions,
        del = pr.deletions,
        branch = pr.head_ref,
        body = if body.is_empty() { "(none)" } else { &body },
    )
}

/// The chat stand-in's prompt: the facts, then the questions and how to answer them.
fn prompt(pr: &PullRequest, questions: &[Question]) -> String {
    let asked: String = questions
        .iter()
        .enumerate()
        .map(|(n, q)| format!("{}. {}: {}\n", n + 1, q.id, q.ask))
        .collect();
    let (n, s) = (questions.len(), if questions.len() == 1 { "" } else { "s" });
    format!(
        "{facts}\n\nAnswer {n} yes/no question{s} about this pull request, in this order, as {n} letter{s} Y or N separated by spaces. Nothing else. Most pull requests are routine, so answer N unless the text clearly says otherwise.\n{asked}",
        facts = facts(pr),
    )
}

/// A decision model's request: the state, and one yes/no (`noul`) question per id.
fn systemone_request(
    cfg: &JudgeConfig,
    pr: &PullRequest,
    questions: &[Question],
) -> serde_json::Value {
    let asked: serde_json::Map<String, serde_json::Value> = questions
        .iter()
        .map(|q| {
            (
                q.id.clone(),
                serde_json::json!({ "type": "noul", "instructions": q.ask }),
            )
        })
        .collect();
    serde_json::json!({ "model": cfg.model, "state": facts(pr), "questions": asked })
}

/// The probability of yes for each question, from a decision model's response.
pub fn parse_systemone(
    response: &serde_json::Value,
    questions: &[Question],
    input_usd_per_million: f64,
) -> Result<(Vec<f32>, Usage), String> {
    let mut odds = Vec::new();
    for q in questions {
        let p = response["answers"][q.id.as_str()]["noul"]
            .as_f64()
            .ok_or_else(|| format!("the decision model gave no answer for \"{}\"", q.id))?;
        odds.push(p.clamp(0.0, 1.0) as f32);
    }
    let input = response["usage"]["input_tokens"].as_u64().unwrap_or(0);
    Ok((
        odds,
        Usage {
            prompt_tokens: input,
            completion_tokens: response["usage"]["output_tokens"].as_u64().unwrap_or(0),
            cost_usd: input as f64 * input_usd_per_million / 1e6,
        },
    ))
}

/// What one call used, as the provider reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost_usd: f64,
}

/// The probability of Y at each of the first `count` answers, from the token log-probabilities.
/// A provider that sends none gets the letters themselves, held at 92% so they cannot claim
/// more certainty than a yes or a no deserves.
pub fn parse_answers(
    response: &serde_json::Value,
    count: usize,
) -> Result<(Vec<f32>, Usage), String> {
    let choice = &response["choices"][0];
    let usage = Usage {
        prompt_tokens: response["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: response["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        cost_usd: response["usage"]["cost"].as_f64().unwrap_or(0.0),
    };
    let letter = |token: &str| match token.trim().to_ascii_uppercase().as_str() {
        "Y" => Some(true),
        "N" => Some(false),
        _ => None,
    };
    let mut odds = Vec::<f32>::new();
    if let Some(tokens) = choice["logprobs"]["content"].as_array() {
        for entry in tokens {
            let Some(chosen) = entry["token"].as_str().and_then(letter) else {
                continue;
            };
            let (mut yes, mut no) = (0.0f32, 0.0f32);
            if let Some(top) = entry["top_logprobs"].as_array() {
                for alt in top {
                    let p = alt["logprob"].as_f64().unwrap_or(f64::NEG_INFINITY).exp() as f32;
                    match alt["token"].as_str().and_then(letter) {
                        Some(true) => yes += p,
                        Some(false) => no += p,
                        None => {}
                    }
                }
            }
            odds.push(if yes + no > 0.0 {
                yes / (yes + no)
            } else if chosen {
                0.92
            } else {
                0.08
            });
        }
    }
    if odds.len() < count {
        let content = choice["message"]["content"].as_str().unwrap_or_default();
        odds = content
            .chars()
            .filter_map(|c| match c.to_ascii_uppercase() {
                'Y' => Some(0.92),
                'N' => Some(0.08),
                _ => None,
            })
            .collect();
    }
    if odds.len() >= count {
        odds.truncate(count);
        Ok((odds, usage))
    } else {
        Err(format!(
            "the model did not answer Y or N {count} times: {}",
            choice["message"]["content"]
        ))
    }
}

fn post_json(
    cfg: &JudgeConfig,
    endpoint: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    // The body goes through a private temp file and the key through curl's stdin, so neither shows in `ps`.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "meat-proxy-judge-{}-{}.json",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(request.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let config = format!(
        "url = \"{}{endpoint}\"\nheader = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
        escape(cfg.base_url.trim_end_matches('/')),
        escape(&cfg.api_key)
    );
    let spawned = Command::new("curl")
        .args(["-sS", "--max-time", "40", "-X", "POST", "--config", "-"])
        .arg("--data-binary")
        .arg(format!("@{}", path.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let result = spawned
        .map_err(|e| format!("could not run curl: {e}"))
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(config.as_bytes());
            }
            child.wait_with_output().map_err(|e| e.to_string())
        });
    let _ = std::fs::remove_file(&path);
    let output = result?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("not JSON from the model API: {e}"))?;
    if let Some(error) = json.get("error") {
        let message = error["message"].as_str().unwrap_or("error");
        let code = error["code"].as_i64().unwrap_or(0);
        return Err(format!("{code} {message}"));
    }
    Ok(json)
}

/// Ask the model the questions about one pull request.
pub fn ask(
    cfg: &JudgeConfig,
    questions: &[Question],
    pr: &PullRequest,
) -> Result<(Judgment, Usage), String> {
    let (odds, usage) = if cfg.provider == "systemone" {
        let response = post_json(cfg, "/systemone", &systemone_request(cfg, pr, questions))?;
        parse_systemone(&response, questions, cfg.input_usd_per_million)?
    } else {
        let request = serde_json::json!({
            "model": cfg.model,
            "temperature": 0,
            "max_tokens": 4 * questions.len().max(1) + 4,
            "logprobs": true,
            "top_logprobs": 8,
            // OpenRouter would otherwise route to a provider that drops the log-probabilities.
            "provider": { "require_parameters": true },
            "messages": [
                { "role": "system", "content": "You triage pull requests for a busy reviewer. Reply with only the letters Y or N, separated by spaces, one per question." },
                { "role": "user", "content": prompt(pr, questions) },
            ],
        });
        parse_answers(
            &post_json(cfg, "/chat/completions", &request)?,
            questions.len(),
        )?
    };
    Ok((
        Judgment {
            answers: questions.iter().map(|q| q.id.clone()).zip(odds).collect(),
            by: cfg.model.clone(),
            fingerprint: fingerprint(pr, &questions_key(questions)),
        },
        usage,
    ))
}

/// What a batch sends back: a judgment per pull request, or why it failed.
#[derive(Debug, Default)]
pub struct Batch {
    pub judged: Vec<(String, Judgment)>,
    pub errors: Vec<String>,
    pub usage: Usage,
    /// An error no retry fixes (bad key, no credit, rate limit, a model no provider serves with
    /// log-probabilities): stop asking.
    pub fatal: bool,
    pub calls: usize,
}

/// Judge pull requests a few at a time, stopping early on an error that would repeat or when the
/// spending cap is reached.
pub fn run_batch(
    cfg: &JudgeConfig,
    questions: &[Question],
    prs: Vec<PullRequest>,
    spent_before: f64,
) -> Batch {
    let mut batch = Batch::default();
    for chunk in prs.chunks(PARALLEL_CALLS) {
        if batch.fatal || spent_before + batch.usage.cost_usd >= cfg.max_spend_usd {
            break;
        }
        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|pr| scope.spawn(move || (pr.id.clone(), ask(cfg, questions, pr))))
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (id, result) in results {
            batch.calls += 1;
            match result {
                Ok((judgment, usage)) => {
                    batch.usage.prompt_tokens += usage.prompt_tokens;
                    batch.usage.completion_tokens += usage.completion_tokens;
                    batch.usage.cost_usd += usage.cost_usd;
                    batch.judged.push((id, judgment));
                }
                Err(error) => {
                    let code = error.split_whitespace().next().unwrap_or("");
                    batch.fatal |= matches!(code, "401" | "402" | "403" | "404" | "429");
                    batch.errors.push(format!("{id}: {error}"));
                }
            }
        }
    }
    batch
}

/// What to send: the candidates the cache and the rules do not already answer, newest first,
/// at most `cfg.max_per_sync`. The rules' answers come back at once in `settled`.
pub fn plan(
    prs: &[PullRequest],
    attention: impl Fn(&PullRequest) -> PrAttention,
    cache: &HashMap<String, Judgment>,
    cfg: &JudgeConfig,
    questions: &[Question],
) -> Plan {
    let mut plan = Plan::default();
    // No section depends on a question: there is nothing to ask.
    if questions.is_empty() {
        return plan;
    }
    let key = questions_key(questions);
    for pr in prs {
        if !is_candidate(pr, attention(pr), cfg) {
            continue;
        }
        let fp = fingerprint(pr, &key);
        // A verdict stands while the text and the questions are the same and it came from a rule
        // or from the model in use with an answer to every question.
        if cache.get(&pr.id).is_some_and(|j| {
            j.fingerprint == fp
                && (j.by == "rules"
                    || (j.by == cfg.model
                        && questions.iter().all(|q| j.answers.contains_key(&q.id))))
        }) {
            plan.cached += 1;
        } else if let Some(judgment) = shortcut(pr, &key) {
            plan.settled.push((pr.id.clone(), judgment));
        } else {
            plan.to_ask.push(pr.clone());
        }
    }
    plan.to_ask.sort_by_key(|pr| pr.idle_hours);
    plan.deferred = plan.to_ask.len().saturating_sub(cfg.max_per_sync);
    plan.to_ask.truncate(cfg.max_per_sync);
    plan
}

#[derive(Debug, Default)]
pub struct Plan {
    pub to_ask: Vec<PullRequest>,
    pub settled: Vec<(String, Judgment)>,
    pub cached: usize,
    /// Left for the next sync because this one hit its cap.
    pub deferred: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::Layout;
    use serde_json::json;

    fn pr(title: &str) -> PullRequest {
        let mut pr = crate::github::demo_prs().remove(0);
        pr.title = title.into();
        pr.body = "Fixes the thing.".into();
        pr.additions = 40;
        pr.deletions = 10;
        pr.is_draft = false;
        pr.private_repo = false;
        pr
    }

    fn defaults() -> Vec<Question> {
        Layout::default().needed_questions()
    }

    fn token(letter: &str, yes: f64, no: f64) -> serde_json::Value {
        json!({ "token": letter, "logprob": 0.0, "top_logprobs": [
            { "token": "Y", "logprob": yes.ln() },
            { "token": " N", "logprob": no.ln() },
            { "token": "maybe", "logprob": -9.0 },
        ]})
    }

    #[test]
    fn answers_are_the_models_own_odds() {
        let response = json!({
            "choices": [{ "message": { "content": "Y N N" }, "logprobs": { "content": [
                token("Y", 0.9, 0.1), token(" N", 0.2, 0.6), token(" N", 0.01, 0.97),
            ]}}],
            "usage": { "prompt_tokens": 300, "completion_tokens": 5, "cost": 0.0001 },
        });
        let (odds, usage) = parse_answers(&response, 3).unwrap();
        assert!((odds[0] - 0.9).abs() < 1e-3, "{odds:?}");
        assert!((odds[1] - 0.25).abs() < 1e-3);
        assert!(odds[2] < 0.02);
        assert_eq!(usage.prompt_tokens, 300);
        // Two questions ask for two answers, whatever else the model wrote.
        assert_eq!(parse_answers(&response, 2).unwrap().0.len(), 2);
        assert!(parse_answers(&response, 4).is_err());
    }

    #[test]
    fn without_logprobs_the_letters_are_held_back_from_certainty() {
        let response = json!({ "choices": [{ "message": { "content": "Y N Y" } }] });
        let (odds, _) = parse_answers(&response, 3).unwrap();
        assert_eq!(odds, [0.92, 0.08, 0.92]);
        let junk = json!({ "choices": [{ "message": { "content": "I think so" } }] });
        assert!(parse_answers(&junk, 3).is_err());
    }

    #[test]
    fn the_prompt_lists_exactly_the_questions_asked() {
        let questions = vec![Question {
            id: "payments".into(),
            ask: "Does it change how money moves?".into(),
            at: 0.6,
        }];
        let text = prompt(&pr("Refund flow"), &questions);
        assert!(text.contains("1. payments: Does it change how money moves?"));
        assert!(text.contains("Answer 1 yes/no question "));
        assert!(!text.contains("urgent"));
    }

    #[test]
    fn a_decision_model_is_asked_with_a_state_and_noul_questions() {
        let questions = defaults();
        let cfg = JudgeConfig {
            provider: "systemone".into(),
            model: "jev-latest".into(),
            input_usd_per_million: 0.042,
            ..cfg(30)
        };
        let request = systemone_request(&cfg, &pr("Add webhook retries"), &questions);
        assert_eq!(request["model"], "jev-latest");
        assert!(
            request["state"]
                .as_str()
                .unwrap()
                .contains("Title: Add webhook retries")
        );
        assert_eq!(request["questions"]["urgent"]["type"], "noul");
        assert!(
            request["questions"]["risky"]["instructions"]
                .as_str()
                .unwrap()
                .contains("authentication")
        );
        assert_eq!(
            request["questions"].as_object().unwrap().len(),
            questions.len()
        );
        // The answers come back as probabilities keyed by question id, and the input is priced.
        let response = json!({
            "model": "jev-1.13.0",
            "answers": {
                "trivial": { "type": "noul", "noul": 0.04 },
                "urgent": { "type": "noul", "noul": 0.91 },
                "risky": { "type": "noul", "noul": 0.5 },
            },
            "usage": { "input_tokens": 1000, "output_tokens": 0 },
        });
        let (odds, usage) = parse_systemone(&response, &questions, 0.042).unwrap();
        assert_eq!(odds, [0.04, 0.91, 0.5]);
        assert_eq!(usage.prompt_tokens, 1000);
        assert!((usage.cost_usd - 0.000042).abs() < 1e-9);
        let missing = json!({ "answers": { "trivial": { "noul": 0.1 } } });
        assert!(parse_systemone(&missing, &questions, 0.0).is_err());
    }

    #[test]
    fn rules_answer_bots_and_tiny_changes_without_a_call() {
        let mut bot = pr("Update lockfile");
        bot.author = "dependabot".into();
        let j = shortcut(&bot, "k").unwrap();
        assert!(j.answers["trivial"] > 0.9 && j.by == "rules");
        let mut tiny = pr("Fix label");
        tiny.additions = 1;
        tiny.deletions = 1;
        assert!(shortcut(&tiny, "k").is_some());
        assert!(shortcut(&pr("Rework checkout payment flow"), "k").is_none());
    }

    #[test]
    fn the_fingerprint_tracks_the_text_and_the_questions_and_not_the_clock() {
        let a = pr("Add webhook retries");
        let mut later = a.clone();
        later.idle_hours += 50;
        later.reviewers.push("someone".into());
        assert_eq!(fingerprint(&a, "k"), fingerprint(&later, "k"));
        let mut edited = a.clone();
        edited.body.push_str(" Urgent: blocks the release.");
        assert_ne!(fingerprint(&a, "k"), fingerprint(&edited, "k"));
        assert_ne!(
            fingerprint(&a, "k"),
            fingerprint(&a, "another set of questions")
        );
    }

    fn cfg(max: usize) -> JudgeConfig {
        JudgeConfig {
            base_url: String::new(),
            api_key: "k".into(),
            model: "m".into(),
            provider: "chat".into(),
            input_usd_per_million: 0.0,
            include_private: false,
            max_per_sync: max,
            max_spend_usd: 1.0,
        }
    }

    #[test]
    fn only_what_needs_a_model_is_sent() {
        let questions = defaults();
        let key = questions_key(&questions);
        let cfg = cfg(2);
        let mut prs = Vec::new();
        for n in 0..5u64 {
            let mut p = pr(&format!("Change number {n}"));
            p.id = format!("o/r#{n}");
            p.idle_hours = n;
            prs.push(p);
        }
        prs[3].private_repo = true; // never leaves the machine
        prs[4].author = "renovate".into(); // a rule answers
        let mut cache = HashMap::new();
        cache.insert(
            prs[0].id.clone(),
            Judgment {
                answers: questions.iter().map(|q| (q.id.clone(), 0.1)).collect(),
                by: "m".into(),
                fingerprint: fingerprint(&prs[0], &key),
            },
        );
        let attention = |_: &PullRequest| PrAttention::IncomingAssigned;
        let plan = plan(&prs, attention, &cache, &cfg, &questions);
        assert_eq!(plan.cached, 1);
        assert_eq!(plan.settled.len(), 1);
        let asked: Vec<_> = plan.to_ask.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(asked, ["o/r#1", "o/r#2"], "newest first, capped");
        // A new model, or a question added since, redoes the verdict.
        let other = JudgeConfig {
            model: "another/model".into(),
            ..cfg.clone()
        };
        assert_eq!(
            super::plan(&prs, attention, &cache, &other, &questions).cached,
            0
        );
        let mut more = questions.clone();
        more.push(Question {
            id: "payments".into(),
            ask: "Money?".into(),
            at: 0.6,
        });
        assert_eq!(super::plan(&prs, attention, &cache, &cfg, &more).cached, 0);
        // Pull requests that are only watched, and layouts that ask nothing, send nothing.
        let watched = super::plan(
            &prs,
            |_| PrAttention::IncomingWatched,
            &HashMap::new(),
            &cfg,
            &questions,
        );
        assert!(watched.to_ask.is_empty() && watched.settled.is_empty());
        assert!(
            super::plan(&prs, attention, &HashMap::new(), &cfg, &[])
                .to_ask
                .is_empty()
        );
    }

    #[test]
    fn templates_noise_is_dropped_from_the_description() {
        let body =
            "<!-- Describe your change -->\n![shot](x.png)\n\nReal text here.\n\n\n<!-- end -->";
        assert_eq!(tidy(body), "Real text here.");
    }
}
