https://github.com/user-attachments/assets/184cc8f9-1b30-4865-9424-a2166eb49900

# <img src="assets/logo.svg" alt="" width="40" height="40" align="top"> Meat Proxy

Keep up with the pace of LLM-generated code.

Meat Proxy is a native macOS inbox for your GitHub pull requests. It gathers the reviews requested of you and the pull requests you opened, then sorts them by what each one needs from you: merge, review, unblock, address feedback, find a reviewer, or finish a draft. Your own pull requests merge from the list once GitHub allows it, and what holds one up (a branch behind its base, failed checks, conflicts) has its button on the row. macOS notifications tap you on the shoulder when a review lands on you, when your pull request sits unreviewed, and when it gets stuck or becomes mergeable. It reads GitHub through the GitHub CLI, signed in as you, so there are no tokens to paste.

## Install

Install the [GitHub CLI](https://cli.github.com) (`brew install gh`), then:

```sh
curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-mac.sh | sh
```

It updates itself hourly. Add `sh -s -- --uninstall` to the same command to remove it.

## Build from source

With Rust, Xcode Command Line Tools and the GitHub CLI installed:

```sh
./package.sh && ~/.cargo/bin/meat-proxy
```

## Recording a demo

`MEAT_PROXY_HIDE_PRIVATE=1` keeps private repositories out of the app: their pull requests, notifications and names. Start it from a terminal:

```sh
MEAT_PROXY_HIDE_PRIVATE=1 ~/.cargo/bin/meat-proxy
```
