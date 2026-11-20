# meat-proxy

An inbox for GitHub pull requests, for the web and macOS, built with [Exact](https://github.com/expo/exact). It groups PRs by repository, nests branch-based stacks, and reminds you about pending reviews and stale drafts.

![The pull request inbox in dark mode](snapshots/inbox-dark.png)

## Install

The app lives in `exact/`. Each install keeps itself at the latest release: every push to
`main` publishes one (`.github/workflows/release.yml`, the next patch unless `exact/package.json` names a higher
version), and installed copies pick it up within the hour.

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

To try it against a fake GitHub, run `python3 harness/api.py` and launch with
`GITHUB_API=http://127.0.0.1:8787 MEAT_PROXY_TEST_TOKEN=fake`.
