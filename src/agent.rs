//! Detached coding agents: what the Fix button runs once the user turns it on in Settings.
//!
//! The button's alternative is Copy prompt, which hands the same prompt to an agent the user runs
//! themselves. Here meat proxy runs it: it clones the repository into its own folder, checks the
//! pull request out, starts Claude Code or Codex there without a terminal and waits for it. It
//! then asks GitHub whether the branch moved, which is the only thing it can know right away; the
//! next sync says whether the checks and the merge came out right.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::model::PullRequest;

/// An agent that has not finished by now is stopped.
const TIME_LIMIT: Duration = Duration::from_secs(45 * 60);
/// How much of an agent's last words an error keeps.
const TAIL_CHARS: usize = 400;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Claude,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];

    pub fn id(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "codex" => Provider::Codex,
            _ => Provider::Claude,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// The label of the choice that leaves the model to the agent's own configuration.
pub const DEFAULT_MODEL: &str = "Agent default";

/// The models Settings offers for `provider`, as (value for `--model`, name shown). Claude Code has
/// no command that lists them, so these are the current ones. Codex keeps its own list in
/// `~/.codex/models_cache.json`.
pub fn models(provider: Provider) -> Vec<(String, String)> {
    match provider {
        Provider::Claude => [
            ("claude-fable-5-2", "Fable 5.2"),
            ("claude-opus-5-5", "Opus 5.5"),
            ("claude-sonnet-5-5", "Sonnet 5.5"),
            ("claude-haiku-5-5", "Haiku 5.5"),
        ]
        .map(|(value, name)| (value.to_owned(), name.to_owned()))
        .to_vec(),
        Provider::Codex => {
            static LISTED: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
            LISTED.get_or_init(codex_models).clone()
        }
    }
}

fn codex_models() -> Vec<(String, String)> {
    let listed = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".codex/models_cache.json"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| {
            let models = json.get("models")?.as_array()?;
            Some(
                models
                    .iter()
                    .filter(|m| m.get("visibility").and_then(|v| v.as_str()) == Some("list"))
                    .filter_map(|m| {
                        let slug = m.get("slug")?.as_str()?.to_owned();
                        let name = m.get("display_name").and_then(|n| n.as_str());
                        Some((slug.clone(), name.unwrap_or(&slug).to_owned()))
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    if listed.is_empty() {
        [("gpt-5-codex", "GPT-5 Codex"), ("gpt-5", "GPT-5")]
            .map(|(value, name)| (value.to_owned(), name.to_owned()))
            .to_vec()
    } else {
        listed
    }
}

/// The name Settings shows for a saved model: the listed name, else the value itself.
pub fn model_name(provider: Provider, value: &str) -> String {
    if value.is_empty() {
        return DEFAULT_MODEL.to_owned();
    }
    models(provider)
        .into_iter()
        .find(|(v, _)| v == value)
        .map_or_else(|| value.to_owned(), |(_, name)| name)
}

/// The value to save for a name chosen in Settings: empty for the agent's default.
pub fn model_value(provider: Provider, name: &str) -> String {
    models(provider)
        .into_iter()
        .find(|(_, n)| n == name)
        .map_or_else(
            || if name == DEFAULT_MODEL { String::new() } else { name.to_owned() },
            |(value, _)| value,
        )
}

/// Settings › Fix with an agent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub provider: Provider,
    /// Passed as `--model`; empty leaves the choice to the agent's own configuration.
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The agent finished and the pull request has a new head commit.
    Pushed,
    /// The agent finished and pushed nothing; its last words, and where the clone stayed.
    NothingPushed { said: String, folder: PathBuf },
}

impl Outcome {
    pub fn announce(&self, pr: &PullRequest) -> String {
        let name = format!("{}#{}", pr.repo, pr.number);
        match self {
            Outcome::Pushed => format!("Pushed a fix to {name}. Checks are running again."),
            Outcome::NothingPushed { .. } => {
                format!("The agent finished on {name} but pushed nothing")
            }
        }
    }
}

fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&inherited).collect();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        paths.push(home.join(".local/bin"));
        paths.push(home.join(".local/share/mise/shims"));
        paths.push(home.join(".cargo/bin"));
    }
    paths.extend(["/opt/homebrew/bin".into(), "/usr/local/bin".into()]);
    if let Ok(path) = std::env::join_paths(paths) {
        cmd.env("PATH", path);
    }
    cmd
}

/// Where agents work, one folder per pull request.
fn workspace(pr: &PullRequest) -> PathBuf {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".config")
        .join(crate::profile::dir_name())
        .join("agents");
    base.join(format!("{}-{}", pr.repo.replace('/', "-"), pr.number))
}

/// What the agent is told: the prompt Copy prompt would have copied, said to an agent that
/// nobody will answer.
pub fn instructions(pr: &PullRequest, prompt: &str) -> String {
    format!(
        "You are working unattended: nobody will answer questions, so decide and carry on.\n\
         The pull request {} is already checked out in the current directory, on its own branch.\n\n\
         {prompt}\n\n\
         When the work is done, push to the pull request's branch. Do not open a new pull request. \
         If you cannot fix it, push nothing and say why in your last message.",
        pr.url
    )
}

fn run_checked(mut cmd: Command, what: &str) -> Result<String, String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(format!(
            "{what}: {}",
            tail(&String::from_utf8_lossy(&out.stderr))
        ))
    }
}

fn tail(text: &str) -> String {
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(TAIL_CHARS);
    text.chars().skip(skip).collect()
}

/// The arguments that start `provider` on `prompt` in `dir`, without a terminal and without
/// asking: the agent has to run tests and push, which nothing else could approve.
fn agent_command(cfg: &Config, dir: &Path, prompt: &str) -> Command {
    match cfg.provider {
        Provider::Claude => {
            let mut cmd = command("claude");
            cmd.args(["-p", prompt, "--dangerously-skip-permissions"]);
            if !cfg.model.is_empty() {
                cmd.args(["--model", &cfg.model]);
            }
            cmd
        }
        Provider::Codex => {
            let mut cmd = command("codex");
            cmd.args(["exec", "--dangerously-bypass-approvals-and-sandbox", "-C"])
                .arg(dir);
            if !cfg.model.is_empty() {
                cmd.args(["-m", &cfg.model]);
            }
            cmd.arg(prompt);
            cmd
        }
    }
}

/// Clone, check out, run the agent, and see whether the branch moved. Blocks until the agent is
/// done: call it from a worker.
pub fn run(pr: &PullRequest, prompt: &str, cfg: &Config) -> Result<Outcome, String> {
    let dir = workspace(pr);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;

    let mut clone = crate::github::gh_command();
    clone
        .args(["repo", "clone", &pr.repo])
        .arg(&dir)
        .args(["--", "--filter=blob:none"]);
    run_checked(clone, "Could not clone the repository")?;

    let mut checkout = crate::github::gh_command();
    checkout
        .args(["pr", "checkout", &pr.number.to_string()])
        .current_dir(&dir);
    run_checked(checkout, "Could not check out the pull request")?;

    let mut cmd = agent_command(cfg, &dir, &instructions(pr, prompt));
    let mut child = cmd
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", cfg.provider.label()))?;
    // Both pipes are drained on threads, so an agent that talks a lot never blocks on a full one.
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
            }
            text
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if started.elapsed() > TIME_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} ran for over {} minutes and was stopped",
                    cfg.provider.label(),
                    TIME_LIMIT.as_secs() / 60
                ));
            }
            None => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    let (said, errors) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if !status.success() {
        return Err(format!(
            "{} failed: {}",
            cfg.provider.label(),
            tail(if errors.trim().is_empty() { &said } else { &errors })
        ));
    }

    let mut head = crate::github::gh_command();
    head.args([
        "pr",
        "view",
        &pr.number.to_string(),
        "-R",
        &pr.repo,
        "--json",
        "headRefOid",
        "-q",
        ".headRefOid",
    ]);
    let now = run_checked(head, "Could not read the pull request")?;
    if !now.is_empty() && now != pr.head_oid {
        let _ = std::fs::remove_dir_all(&dir);
        Ok(Outcome::Pushed)
    } else {
        Ok(Outcome::NothingPushed {
            said: tail(&said),
            folder: dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_round_trip_through_their_saved_names() {
        for p in Provider::ALL {
            assert_eq!(Provider::from_id(p.id()), p);
        }
        assert_eq!(Provider::from_id("anything else"), Provider::Claude);
    }

    #[test]
    fn the_model_is_passed_only_when_chosen() {
        let dir = Path::new("/tmp/x");
        let args = |cfg: &Config| -> Vec<String> {
            agent_command(cfg, dir, "go")
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let mut cfg = Config::default();
        assert!(!args(&cfg).contains(&"--model".to_owned()));
        cfg.model = "opus".into();
        assert!(args(&cfg).windows(2).any(|w| w == ["--model", "opus"]));
        cfg.provider = Provider::Codex;
        assert!(args(&cfg).windows(2).any(|w| w == ["-m", "opus"]));
        assert_eq!(args(&cfg).last().unwrap(), "go");
    }
}
