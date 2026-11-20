//! Judgments: yes/no questions a language model answers about a pull request, with probabilities.
//!
//! The model never writes text. It is asked three questions in one call and answers `Y N N`; the
//! probabilities come from the token log-probabilities at each answer, so "92% trivial" is the
//! model's own odds, not a guess dressed up as a number. Everything else (merge readiness, who
//! the author is, how long it has waited) stays in code: the model only answers what needs reading.
//!
//! A model costs money and time, so it is the last resort. A pull request goes to it only when
//! it asks something of the user, lives in a repository whose text may leave the machine, has
//! changed since it was last judged, and no cheap rule already knows the answer.

use crate::model::{PrAttention, PullRequest};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

/// A trivial change: a typo, docs, a dependency bump. Worth a skim, not a read.
pub const TRIVIAL_AT: f32 = 0.75;
/// Time pressure or something blocked on it.
pub const URGENT_AT: f32 = 0.6;
/// Touches auth, payments, migrations, security or infrastructure.
pub const RISKY_AT: f32 = 0.6;

const DEFAULT_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";
const BODY_CHARS: usize = 1400;
const PARALLEL_CALLS: usize = 4;
/// Raise it when the questions or the rules change, so verdicts made under the old ones are redone.
const PROMPT_VERSION: &str = "2";

#[derive(Clone, Debug, PartialEq)]
pub struct Judgment {
    pub trivial: f32,
    pub urgent: f32,
    pub risky: f32,
    /// "rules" when a cheap rule decided, else the model's name.
    pub by: String,
    /// What the pull request looked like when it was judged; a change means judging again.
    pub fingerprint: String,
}

impl Judgment {
    pub fn is_urgent(&self) -> bool {
        self.urgent >= URGENT_AT
    }
    pub fn is_risky(&self) -> bool {
        self.risky >= RISKY_AT
    }
    /// Worth skimming instead of reading: trivial, and nothing says to slow down.
    pub fn is_skim(&self) -> bool {
        self.trivial >= TRIVIAL_AT && !self.is_urgent() && !self.is_risky()
    }
    pub fn summary(&self) -> String {
        format!(
            "Alien intelligence says: urgent {:.0}%  ·  risky {:.0}%  ·  trivial {:.0}%  ·  {}",
            self.urgent * 100.0,
            self.risky * 100.0,
            self.trivial * 100.0,
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
        // A local server needs no key; a hosted one without a key would only fail.
        let local = cfg.base_url.contains("localhost") || cfg.base_url.contains("127.0.0.1");
        (local || !cfg.api_key.trim().is_empty()).then_some(cfg)
    }
}

/// Whether a pull request is worth asking about at all. Mine, drafts, and ones only watched need
/// no judgment about how to review them.
pub fn is_candidate(pr: &PullRequest, attention: PrAttention, cfg: &JudgeConfig) -> bool {
    matches!(
        attention,
        PrAttention::IncomingAssigned | PrAttention::IncomingViaTeam
    ) && !pr.is_draft
        && (cfg.include_private || !pr.private_repo)
}

/// What cheap rules already know. A hit costs nothing and is final.
pub fn shortcut(pr: &PullRequest) -> Option<Judgment> {
    let author = pr.author.to_ascii_lowercase();
    let bot = author.ends_with("[bot]")
        || author.contains("dependabot")
        || author.contains("renovate")
        || author == "github-actions";
    let title = pr.title.to_ascii_lowercase();
    let routine = title.starts_with("bump ")
        || title.starts_with("chore(deps")
        || title.starts_with("docs:")
        || title.starts_with("docs(")
        || title.starts_with("typo");
    let tiny = pr.additions + pr.deletions <= 2;
    (bot || routine || tiny).then(|| Judgment {
        trivial: if bot || routine { 0.95 } else { 0.85 },
        urgent: 0.03,
        risky: 0.05,
        by: "rules".into(),
        fingerprint: fingerprint(pr),
    })
}

/// What a judgment depends on: the text and the size, not the comments or the clock.
pub fn fingerprint(pr: &PullRequest) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes.iter().chain(&[0u8]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    feed(PROMPT_VERSION.as_bytes());
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

fn prompt(pr: &PullRequest) -> String {
    let labels = if pr.labels.is_empty() {
        "none".to_owned()
    } else {
        pr.labels.join(", ")
    };
    let body = tidy(&pr.body);
    format!(
        "Pull request in {repo}\nTitle: {title}\nAuthor: {author}{external}\nLabels: {labels}\nSize: +{add} -{del}\nBranch: {branch}\nDescription:\n{body}\n\n\
         Answer three yes/no questions about this pull request, in this order, as three letters Y or N separated by spaces. Nothing else. Most pull requests are routine, so answer N unless the text clearly says otherwise.\n\
         1. Trivial: is it a typo, docs, formatting, a rename, a dependency bump, a translation or generated code, safe to skim without reading the diff?\n\
         2. Urgent: does the title or description itself state time pressure: a deadline, a customer or release waiting on it, an outage or production incident, or the words urgent, asap, hotfix or blocker? An ordinary bug fix, feature or improvement is NOT urgent.\n\
         3. Risky: is the main change itself in authentication, authorization, payments or money, data migrations, security, or deploy and infrastructure config? Judge the change, not words mentioned in passing.\n\n\
         Examples. \"bump lodash from 4.17.20 to 4.17.21\" is Y N N. \"Hotfix: checkout crashes for all customers, blocks the release\" is N Y N. \"Add refund idempotency keys to the payment flow\" is N N Y. \"Add retry to the webhook sender\" is N N N.",
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

/// What one call used, as the provider reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost_usd: f64,
}

/// The probability of Y at each of the first three answers, from the token log-probabilities.
/// A provider that sends none gets the letters themselves, held at 92% so they cannot claim
/// more certainty than a yes or a no deserves.
pub fn parse_answers(response: &serde_json::Value) -> Result<([f32; 3], Usage), String> {
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
    if odds.len() < 3 {
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
    match odds.as_slice() {
        [a, b, c, ..] => Ok(([*a, *b, *c], usage)),
        _ => Err(format!(
            "the model did not answer Y or N three times: {}",
            choice["message"]["content"]
        )),
    }
}

fn post_chat(cfg: &JudgeConfig, request: &serde_json::Value) -> Result<serde_json::Value, String> {
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
        "url = \"{}/chat/completions\"\nheader = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
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

/// Ask the model about one pull request.
pub fn ask(cfg: &JudgeConfig, pr: &PullRequest) -> Result<(Judgment, Usage), String> {
    let request = serde_json::json!({
        "model": cfg.model,
        "temperature": 0,
        "max_tokens": 8,
        "logprobs": true,
        "top_logprobs": 8,
        // OpenRouter would otherwise route to a provider that drops the log-probabilities.
        "provider": { "require_parameters": true },
        "messages": [
            { "role": "system", "content": "You triage pull requests for a busy reviewer. Reply with exactly three letters, Y or N, separated by spaces." },
            { "role": "user", "content": prompt(pr) },
        ],
    });
    let response = post_chat(cfg, &request)?;
    let (odds, usage) = parse_answers(&response)?;
    Ok((
        Judgment {
            trivial: odds[0],
            urgent: odds[1],
            risky: odds[2],
            by: cfg.model.clone(),
            fingerprint: fingerprint(pr),
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
pub fn run_batch(cfg: &JudgeConfig, prs: Vec<PullRequest>, spent_before: f64) -> Batch {
    let mut batch = Batch::default();
    for chunk in prs.chunks(PARALLEL_CALLS) {
        if batch.fatal || spent_before + batch.usage.cost_usd >= cfg.max_spend_usd {
            break;
        }
        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|pr| scope.spawn(move || (pr.id.clone(), ask(cfg, pr))))
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
) -> Plan {
    let mut plan = Plan::default();
    for pr in prs {
        if !is_candidate(pr, attention(pr), cfg) {
            continue;
        }
        let fp = fingerprint(pr);
        // A verdict stands while the text is the same and it came from the model in use (or a rule).
        if cache
            .get(&pr.id)
            .is_some_and(|j| j.fingerprint == fp && (j.by == "rules" || j.by == cfg.model))
        {
            plan.cached += 1;
        } else if let Some(judgment) = shortcut(pr) {
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
        let ([trivial, urgent, risky], usage) = parse_answers(&response).unwrap();
        assert!((trivial - 0.9).abs() < 1e-3, "{trivial}");
        assert!((urgent - 0.25).abs() < 1e-3, "{urgent}");
        assert!(risky < 0.02, "{risky}");
        assert_eq!(usage.prompt_tokens, 300);
    }

    #[test]
    fn without_logprobs_the_letters_are_held_back_from_certainty() {
        let response = json!({ "choices": [{ "message": { "content": "Y N Y" } }] });
        let ([a, b, c], _) = parse_answers(&response).unwrap();
        assert_eq!((a, b, c), (0.92, 0.08, 0.92));
        let junk = json!({ "choices": [{ "message": { "content": "I think so" } }] });
        assert!(parse_answers(&junk).is_err());
    }

    #[test]
    fn rules_answer_bots_and_tiny_changes_without_a_call() {
        let mut bot = pr("Update lockfile");
        bot.author = "dependabot".into();
        assert!(shortcut(&bot).is_some_and(|j| j.is_skim() && j.by == "rules"));
        let mut tiny = pr("Fix label");
        tiny.additions = 1;
        tiny.deletions = 1;
        assert!(shortcut(&tiny).is_some());
        assert!(shortcut(&pr("Rework checkout payment flow")).is_none());
    }

    #[test]
    fn the_fingerprint_tracks_the_text_and_not_the_clock() {
        let a = pr("Add webhook retries");
        let mut later = a.clone();
        later.idle_hours += 50;
        later.reviewers.push("someone".into());
        assert_eq!(fingerprint(&a), fingerprint(&later));
        let mut edited = a.clone();
        edited.body.push_str(" Urgent: blocks the release.");
        assert_ne!(fingerprint(&a), fingerprint(&edited));
    }

    #[test]
    fn only_what_needs_a_model_is_sent() {
        let cfg = JudgeConfig {
            base_url: String::new(),
            api_key: "k".into(),
            model: "m".into(),
            include_private: false,
            max_per_sync: 2,
            max_spend_usd: 1.0,
        };
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
                trivial: 0.1,
                urgent: 0.1,
                risky: 0.1,
                by: "m".into(),
                fingerprint: fingerprint(&prs[0]),
            },
        );
        let plan = plan(&prs, |_| PrAttention::IncomingAssigned, &cache, &cfg);
        assert_eq!(plan.cached, 1);
        let other = JudgeConfig {
            model: "another/model".into(),
            ..cfg.clone()
        };
        assert_eq!(
            super::plan(&prs, |_| PrAttention::IncomingAssigned, &cache, &other).cached,
            0,
            "a new model redoes the verdicts"
        );
        assert_eq!(plan.settled.len(), 1);
        let asked: Vec<_> = plan.to_ask.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(asked, ["o/r#1", "o/r#2"], "newest first, capped");
        assert_eq!(plan.deferred, 0);
        let none = plan_for_watched(&prs, &cfg);
        assert!(none.to_ask.is_empty() && none.settled.is_empty());
    }

    fn plan_for_watched(prs: &[PullRequest], cfg: &JudgeConfig) -> Plan {
        plan(prs, |_| PrAttention::IncomingWatched, &HashMap::new(), cfg)
    }

    #[test]
    fn templates_noise_is_dropped_from_the_description() {
        let body =
            "<!-- Describe your change -->\n![shot](x.png)\n\nReal text here.\n\n\n<!-- end -->";
        assert_eq!(tidy(body), "Real text here.");
    }
}
