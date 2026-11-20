<p align="center"><img src="assets/logo.png" width="96" alt="A chewed thumb resting on a green approve check"></p>

# meat proxy

Keep up with the pace of LLM-generated code.

A native macOS inbox for GitHub pull requests, built with Rune. It puts what needs your thumb first, grouped by the action each pull request needs, and merges your own once GitHub allows it. It reads GitHub through the GitHub CLI, as you are signed in to it.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="snapshots/inbox-dark.png">
  <img alt="The pull request inbox, grouped by the action each one needs" src="snapshots/inbox-light.png">
</picture>

## Install

Install and sign in to the [GitHub CLI](https://cli.github.com) (`brew install gh && gh auth login`), then:

```sh
curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-mac.sh | sh
```

It puts meat-proxy in `~/Applications` and keeps it at the latest release: a launchd agent checks every hour and
replaces the app while it is closed. `sh -s -- --uninstall` after the same command removes it. On first launch,
connect your GitHub CLI account; the inbox opens with sensible defaults, which Settings changes.

Every push to `main` publishes a release (`.github/workflows/release.yml`): the next patch, unless `Cargo.toml` names a
higher version.

## Judgments (experimental)

meat proxy can ask a language model three yes/no questions about the review requests sent to you: is it trivial, is it urgent, is it risky. The answers are probabilities read from the model's token log-probabilities. Trivial ones move to a "Skim these" group, urgent ones lead their group, and risky ones say "read carefully".

Cheap rules answer first (bots, dependency bumps, one-line changes). A pull request is sent to the model only when it asks something of you, is in a public repository, and its text changed since it was last judged. Each sync sends at most 30, and the app stops asking after $0.25 in one run.

Create `~/.config/meat-proxy/judge.json` to turn it on. Any OpenAI-compatible endpoint works, and the model must return log-probabilities:

```json
{ "base_url": "https://openrouter.ai/api/v1", "api_key": "…", "model": "openai/gpt-4o-mini" }
```

`include_private`, `max_per_sync` and `max_spend_usd` are optional. `MEAT_PROXY_JUDGE_URL`, `_KEY` and `_MODEL` override the file. `meat-proxy --judge-eval` prints what the model says about your open pull requests, without opening a window.

## Build from source

Install Rust, Xcode Command Line Tools, and the GitHub CLI. From the repository root, run:

```sh
./package.sh
~/.cargo/bin/meat-proxy
```

This installs your build in `/Applications`, beside any released copy in `~/Applications`.
