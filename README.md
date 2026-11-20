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

## Inbox sections (experimental)

The Inbox tab is a list of sections, and the list is configuration: **Settings → Inbox sections → Edit** opens `~/.config/meat-proxy/sections.json`, filled with the defaults. A pull request goes to the first section whose conditions all hold, so the order is the priority and the default order on screen. Dragging only reorders the screen; the button next to "Sorted by …" puts it back. `hide` takes pull requests out of the Inbox tab (they stay under Theirs and All), and **Settings → Show outside contributors in the Inbox** is the simple flag for the most common case.

```json
{
  "questions": [{ "id": "payments", "ask": "Does the change move or count money?" }],
  "sections": [
    { "name": "Money", "when": ["asked:any", "payments"] },
    { "name": "Saleor core", "when": ["asked:any", "repo:saleor/*", "!bot"] },
    { "name": "Everything else", "when": "any" }
  ],
  "hide": [["bot", "lines<10"]]
}
```

A condition is a word, and `!` negates it: `asked:me`, `asked:team`, `asked:any`, `is:approved`, `is:changes-requested`, `is:unreviewed`, `is:stale-draft`, `mine`, `theirs`, `external`, `bot`, `draft`, `lines>=600`, `repo:org/*`, `author:name*`, `label:security`, `title:hotfix`, `any`, or the id of a question. A mistake is named in Settings and the defaults stay in use. Changes apply at the next sync.

A **question** is answered by a language model with a probability read from its token log-probabilities, so a section that names it is sorted by AI (a brain marks it). The model is asked only the questions the sections use, only about review requests sent to you, and only when a cheap rule (bots, dependency bumps, one-line changes) does not already know. Answers are cached until the text or the questions change, private repositories are never sent, each sync sends at most 30, and the app stops after $0.25 in one run. "Sorted by rules" ignores the model entirely.

To turn it on create `~/.config/meat-proxy/judge.json`. Any OpenAI-compatible endpoint works, and the model must return log-probabilities:

```json
{ "base_url": "https://openrouter.ai/api/v1", "api_key": "…", "model": "openai/gpt-4o-mini" }
```

`include_private`, `max_per_sync` and `max_spend_usd` are optional. `MEAT_PROXY_JUDGE_URL`, `_KEY` and `_MODEL` override the file. `meat-proxy --judge-eval` prints the section of each of your open pull requests without opening a window.

## Build from source

Install Rust, Xcode Command Line Tools, and the GitHub CLI. From the repository root, run:

```sh
./package.sh
~/.cargo/bin/meat-proxy
```

This installs your build in `/Applications`, beside any released copy in `~/Applications`.
