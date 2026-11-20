# meat-proxy

A native macOS inbox for GitHub pull requests, built with Rune. It groups PRs by repository, nests branch-based stacks, and reminds you about pending reviews and stale drafts.

![The pull request inbox in dark mode](snapshots/inbox-dark.png)

## Install

The Exact app in `exact/` runs on the web and as a Mac app. Each install keeps itself at the latest release: pushing a
`vX.Y.Z` tag publishes one (`.github/workflows/release.yml`), and installed copies pick it up within the hour.

**Mac app** (in `~/Applications`, updated by a launchd agent while it is closed):

```sh
curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-mac.sh | sh
```

**Web app on your own computer** (a service on http://127.0.0.1:8741, launchd on macOS or systemd on Linux; needs
Node 20+):

```sh
curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-web.sh | sh
```

Either script with `sh -s -- --uninstall` removes it.

## Develop

`sh scripts/setup.sh` (or `sh scripts/setup.sh mac` on a Mac) installs Bun, Rust and Exact at the pin in
`exact/exact-rev`. Then, in `exact/`: `bun exact.mjs web` runs the web app, `bun exact.mjs mac --run` the Mac app.
Sign-in needs a GitHub OAuth app in `exact/.env.local` (`GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`).

## The Rune app

Install Rust, Xcode Command Line Tools, and the GitHub CLI. From the repository root, run:

```sh
./package.sh
~/.cargo/bin/meat-proxy
```

The installer puts the app in `/Applications`. On first launch, connect your GitHub CLI account and choose your inbox filters and notification preferences.
