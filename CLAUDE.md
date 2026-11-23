# Changelog

Every push to `main` is a release, so `CHANGELOG.md` is written as you commit, not at release time.

- A commit that changes what a user sees or does (UI, behaviour, settings, notifications) adds a bullet under `## Unreleased` in `CHANGELOG.md`, in the same commit. Write it for the user, one line, no jargon.
- A commit with nothing user-visible (refactors, tests, CI, docs, dev tooling, demo-only flags) adds none. Say "No changelog" in the commit body so the choice is on record.
- Never edit a version heading. The release workflow renames `## Unreleased` to the version and uses it as the release notes.

# Shipping

No pull requests. Commit on a branch, then push to `main` only after the user approves that push. Every push to `main` releases, so ask before each one and never push unprompted.
