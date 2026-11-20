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

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="snapshots/onboarding-dark.gif">
  <img alt="First launch: a finger presses the approve button, then you connect the GitHub CLI, turn on notifications, and open the inbox" src="snapshots/onboarding-light.gif">
</picture>

Every push to `main` publishes a release (`.github/workflows/release.yml`): the next patch, unless `Cargo.toml` names a
higher version.

## Inbox sections and AI sorting (experimental)

The Inbox tab is a list of sections: Merge these, Hotfixes, Security, Do first, Read carefully, Dependency bumps, Docs, Approve at will, Big reviews, Review these, Address feedback, Team reviews, Get a reviewer and Finish or close. A pull request goes to the first section it fits, so the list order is the priority and the default order on screen. Dragging only reorders the screen; the button next to "Sorted by …" puts it back. **Settings → Show outside contributors in the Inbox** hides outsiders from the Inbox tab; they stay under Theirs and All.

Most sections are plain rules (who asked, size, title, author). Three (Do first, Read carefully, Approve at will) can also use a language model's yes/no answers, and a brain marks them. The model answers with a probability read from its token log-probabilities. It is asked only about review requests sent to you, and only when a cheap rule (bots, dependency bumps, one-line changes) does not already know. Answers are cached until the text changes, private repositories are never sent, each sync sends at most 30, and the app stops after $0.25 in one run. "Sorted by rules" ignores the model entirely, and without a key the Inbox runs on rules alone.

To turn the model on create `~/.config/meat-proxy/judge.json`. Two kinds of endpoint work. A **decision model** (TypeSafe's Jev, or `ggml-org/Kev-4B-GGUF` served by llama.cpp) takes a state and typed yes/no questions at `/v1/systemone` and returns probabilities itself (`"provider": "systemone"`). Any **OpenAI-compatible chat model** that returns token log-probabilities can stand in for one (`"provider": "chat"`, the default):

```json
{ "provider": "systemone", "base_url": "https://openrouter.ai/api/v1", "decisions_url": "https://openrouter.ai/api/alpha/decisions",
  "api_key": "…", "model": "typesafe/jev-1.13" }
```

That is Jev through OpenRouter. Straight from TypeSafe, use `"base_url": "https://api.typesafe.ai/v1"`, `"model": "jev-latest"` and no `decisions_url`; a local llama.cpp server running `ggml-org/Kev-4B-GGUF` works the same way.

The chat stand-in looks like this:

```json
{ "base_url": "https://openrouter.ai/api/v1", "api_key": "…", "model": "openai/gpt-4o-mini" }
```

`include_private`, `max_per_sync`, `max_spend_usd` and `input_usd_per_million` are optional. `MEAT_PROXY_JUDGE_URL`, `_KEY`, `_MODEL` and `_PROVIDER` override the file. `meat-proxy --judge-eval` prints the section of each of your open pull requests without opening a window.

## Build from source

Install Rust, Xcode Command Line Tools, and the GitHub CLI. From the repository root, run:

```sh
./package.sh
~/.cargo/bin/meat-proxy
```

This installs your build in `/Applications`, beside any released copy in `~/Applications`.
