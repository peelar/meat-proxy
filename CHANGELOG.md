# Changelog

## 0.2.1

- Settings: the custom searches ("Also include pull requests matching") sit in the Pull requests section. The two rules an early version seeded are removed; the built-in searches already cover them.
- Teammates' pull requests sort ahead of outsiders' within each section, and outsiders' carry a faint "external" label. Outsiders are GitHub authors who aren't members of the organization.

## 0.2.0

### Inbox
- Four tabs: Inbox (automatic and minimal), Mine, Theirs and All.
- The Inbox groups pull requests by the action they need: review, address feedback, merge, team reviews, get a reviewer, finish or close. Groups can be dragged and folded, and each lists its newest activity first.
- Requests for your review by name rank above requests to a team you belong to.
- Sections show a summary when folded, and one button collapses or expands them all.
- Up to 100 pull requests are shown at once.

### Repositories
- Discover repositories from your organization through the GitHub CLI.
- Add, pin, remove (with confirmation) and reorder repositories from the inbox. Pinned repositories come first; the rest rank automatically by recent activity.
- Archived repositories and pull requests idle for more than 30 days are hidden. Both are settings, and 0 days turns the age limit off.

### Merging
- Merge a pull request from the app once GitHub allows it, including when only optional checks are pending. A second press confirms.
- A merged pull request leaves the list as soon as GitHub reports it closed.

### Sync
- The top bar shows how long ago the last sync ran. Click it to sync again.
- Searches run in parallel, so a sync takes seconds instead of tens of seconds.
- Notifications for review requests read "<repo>#<n> is waiting for your thumb". Turning notifications on sends a welcome notification.

### Look
- The app is now called "meat proxy", with a new thumb logo in the app, the setup screen, the Dock and the README.
- Lighter icon buttons, a modal for adding repositories, and a fix for scrolling.

### Removed
- The snooze and copy-link buttons on pull request rows.
