<picture>
  <source type="image/avif" srcset="snapshots/launch.avif">
  <img alt="meat proxy: the inbox sorted by what each pull request needs, approving a review request, merging your own, fixing conflicts with Claude Code, and updating a branch" src="snapshots/launch.gif" width="100%">
</picture>

<sub>[Watch in full quality (MP4)](snapshots/launch.mp4)</sub>

# <img src="assets/logo.svg" alt="" width="40" height="40" align="top"> meat proxy

Keep up with the pace of LLM-generated code.

meat proxy is a native macOS inbox for your GitHub pull requests. It gathers the reviews requested of you and the pull requests you opened, then sorts them by what each one needs from you: merge, review, unblock, address feedback, find a reviewer, or finish a draft. Your own pull requests merge from the list once GitHub allows it, and what holds one up (a branch behind its base, failed checks, conflicts) has its button on the row. macOS notifications tap you on the shoulder when a review lands on you, when your pull request sits unreviewed, and when it gets stuck or becomes mergeable. It reads GitHub through the GitHub CLI, signed in as you, so there are no tokens to paste.

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
