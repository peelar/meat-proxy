<picture>
  <source media="(prefers-color-scheme: dark)" srcset="snapshots/onboarding-dark.gif">
  <img alt="First launch: a finger presses the approve button, then you connect the GitHub CLI, turn on notifications, and open the inbox" src="snapshots/onboarding-light.gif">
</picture>

# meat proxy

Keep up with the pace of LLM-generated code.

meat proxy is a native macOS inbox for your GitHub pull requests. It gathers the reviews requested of you and the pull requests you opened, then sorts them by what each one needs from you: merge, review, address feedback, find a reviewer, or finish a draft. Your own pull requests merge from the list once GitHub allows it, and macOS notifications tap you on the shoulder when a review lands on you or your pull request sits unreviewed. It reads GitHub through the GitHub CLI, signed in as you, so there are no tokens to paste.

## Install

Install the [GitHub CLI](https://cli.github.com) (`brew install gh`), then:

```sh
curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-mac.sh | sh
```

It updates itself hourly. Add `sh -s -- --uninstall` to the same command to remove it.

## Inbox sections and AI sorting (experimental)

AI sorting is hidden unless you start the app with `MEAT_PROXY_AI=1`.

The Inbox tab is a list of sections: Merge these, Hotfixes, Security, Do first, Read carefully, Dependency bumps, Docs, Approve at will, Big reviews, Review these, Address feedback, Team reviews, Get a reviewer and Finish or close. A pull request goes to the first section it fits, so the list order is the priority and the default order on screen. Dragging only reorders the screen; the button next to "Sorted by …" puts it back. **Settings → Show outside contributors in the Inbox** hides outsiders from the Inbox tab; they stay under Theirs and All.

Most sections are plain rules (who asked, size, title, author). Three (Do first, Read carefully, Approve at will) can also use a language model's yes/no answers, and a brain marks them. The model answers with a probability read from its token log-probabilities. It is asked only about review requests sent to you, and only when a cheap rule (bots, dependency bumps, one-line changes) does not already know. Answers are cached until the text changes, AI sorting is off until you turn it on in Settings (it sends the description and changed file paths of review requests, private repositories included, to the model you configure), each sync sends at most 30, and the app stops after $0.25 in one run. "Sorted by rules" ignores the model entirely, and without a key the Inbox runs on rules alone.

To turn the model on, fill in the **AI** section of Settings (API URL, model, key; the app writes `~/.config/meat-proxy/judge.json` for you). An address ending in `/decisions` or `/systemone` is a decision model, any other a chat endpoint. You can also write the file by hand. Two kinds of endpoint work. A **decision model** (TypeSafe's Jev, or `ggml-org/Kev-4B-GGUF` served by llama.cpp) takes a state and typed yes/no questions at `/v1/systemone` and returns probabilities itself (`"provider": "systemone"`). Any **OpenAI-compatible chat model** that returns token log-probabilities can stand in for one (`"provider": "chat"`, the default):

```json
{ "provider": "systemone", "base_url": "https://openrouter.ai/api/v1", "decisions_url": "https://openrouter.ai/api/alpha/decisions",
  "api_key": "…", "model": "typesafe/jev-1.13" }
```

That is Jev through OpenRouter. Straight from TypeSafe, use `"base_url": "https://api.typesafe.ai/v1"`, `"model": "jev-latest"` and no `decisions_url`; a local llama.cpp server running `ggml-org/Kev-4B-GGUF` works the same way.

The chat stand-in looks like this:

```json
{ "base_url": "https://openrouter.ai/api/v1", "api_key": "…", "model": "openai/gpt-4o-mini" }
```

`max_per_sync`, `max_spend_usd` and `input_usd_per_million` are optional. `MEAT_PROXY_JUDGE_URL`, `_KEY`, `_MODEL` and `_PROVIDER` override the file. `meat-proxy --judge-eval` prints the section of each of your open pull requests without opening a window.

## Build from source

With Rust, Xcode Command Line Tools and the GitHub CLI installed:

```sh
./package.sh && ~/.cargo/bin/meat-proxy
```
