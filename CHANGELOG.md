# Changelog

## 0.3.0

### Inbox
- The Inbox is about your pull requests and the review requests that name you. Team requests leave it (a Settings switch brings them back) and stay under Theirs and All.
- Review requests are split into sections: Merge these, Hotfixes, Security, Do first, Read carefully, Approve at will, Big reviews and Review these. Your own pull requests keep Address feedback, Get a reviewer and Finish or close.
- Three of those sections can use a decision model's yes/no answers (a brain marks them). Rules answer first, answers are cached, private repositories are never sent, and a spending cap applies. Jev, through OpenRouter or TypeSafe, and Kev-4B under llama.cpp are supported, and so is any chat model that returns log-probabilities. `meat-proxy --judge-eval` prints the result on your own pull requests.
- A "Sorted by AI / rules" label switches the model off, and a button next to it puts the sections back in the default order.
- An Ignore button moves a pull request to an Ignored section at the bottom, folded.
- Settings can hide outside contributors from the Inbox.
- New pull requests slide in. Rows show how old a pull request is ("6mo old") and the tooltip gives its last activity. Approvals name who approved.
- The age limit holds a pull request to both its created and updated dates, and the setting reads "Show PRs opened in the last".

### Merging
- Auto-merge is a state of the Merge button: Auto-merge turns it on, Auto-merge on turns it off. A pull request GitHub merges by itself stays at the bottom of its section, faded, until cleared, and sends a notification.

### Look
- New thumb logo, called "meat proxy" in the app; the Dock icon follows. The settings header shows the version, tooltips appear sooner, and notifications moved under their own heading.
- Setup is one step: connect the GitHub CLI and the inbox opens.

## 0.2.2

- Setup is one step: once the GitHub CLI is connected, the inbox opens with the defaults and finds your repositories itself. Everything it asked on the second screen is in Settings.
- Settings shows the app version beside its title.

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
