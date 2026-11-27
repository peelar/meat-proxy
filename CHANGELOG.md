# Changelog

## 0.3.26

- A new switch in Settings, Show my drafts in the Inbox, lists the drafts you're still working on under Get these ready. They don't count as needing your thumb. Drafts left alone for days still show under Finish or close.
- Messages at the top of the Inbox wrap instead of running off the edge, and errors show in red with what went wrong on its own lines.

## 0.3.24

- Stacked pull requests show as a stack in the Inbox tab too, each one indented under the pull request it builds on.

## 0.3.23

- Approve several pull requests in a row: each shows Approving… right away and finishes on its own, instead of later presses being ignored while the first one runs.

## 0.3.22

- CI status follows the latest check runs, so a cancelled or failed attempt replaced by a passing run no longer says CI failing.

## 0.3.21

- Meat Proxy no longer shows up as "sh" in Login Items. Updates come from the app itself, and it removes the old background updater on its next check.
- Approve stays visible on rows that ask for your review. Merge shows only when it can merge, and otherwise appears when you point at the row, with the reason.
- Theirs now lists the latest review request first, and the repository with it. Pinned repositories keep their place at the top.

## 0.3.19

- The toolbar is one row: Collapse all, sort and reset are icons beside the search, and Add repository is a plus next to the Pull Requests heading. A collapsed Scoreboard no longer shows its hint text.
- Meat Proxy now looks for a new version every 5 minutes and downloads it in the background. An Update button appears next to the logo; press it to restart into the new version. If you don't, it installs itself once the app has been in the background for a couple of minutes and nothing is running.

## 0.3.18

- Your own pull requests that are waiting on a review now show in the Inbox, under Waiting on review. They do not count as needing your thumb.
- Collapse all moved below the toolbar, as a quiet text toggle that rests next to the first section.
- Auto-merge buttons work in parallel: press several in a row and each finishes on its own.
- Settings and Add repositories no longer let scrolled cards paint over the dialog's border.
- Headings say Pull Requests.

## 0.3.16

- A pull request that targets a stack base says Merge to base.

## 0.3.15

- A row the agent fixed says Fixed, and the notification comes from the pull request the fix ran on.

## 0.3.14

- Choose the agent's model from a dropdown.
- The Fix button shows the Claude and OpenAI marks.
- The app is called Meat Proxy.

## 0.3.13

- Approve takes one press. Merge still asks for a second.

## 0.3.7

### Pace
- A panel above the Inbox keeps score: your streak of days with an approval or merge, quick approvals and quick merges in the last 7 days, the median time from approval to merge, and a GitHub-style grid of the last 18 weeks. Click its header to fold it to one line; it stays folded.
- Under the quick approvals and quick merges, the fastest of the week. Beat it and the row's pill says New best, with a cup, and so does the panel. Bold approvals never count as fastest.
- When one lands, its number rolls up, today's cell pops and rings, and the row's pill catches the light. The grid is inked against your own usual pace, so its darkest cells stay rare; today stays dashed until you clear one.
- Quick means within the hour: of being asked, for an approval; of the approval (or a later push), for a merge. A row says so for a moment when its approval or merge lands.
- A pull request over 400 lines is never a quick approval, and one approved faster than 100 lines a minute is called bold: counted, no medal, and drawn as a dashed outline instead of ink.
- Only what you approve or merge from meat proxy counts. Turn it all off in Settings › Scoreboard; nothing is counted while it is off.

## 0.3.6

### Autonomous
- A new Settings section, Autonomous, with two switches. Both are off until you turn them on. They act only on your own pull requests that are not drafts, at most once per push, and each action sends a notification.
- Update branches and auto-merge: updates a branch while the repository requires it to be up to date and its checks are not failing, and turns on auto-merge once a pull request is approved and only waits on its checks.
- Re-request reviews after I push: once you push after someone asked for changes, and no comment still waits for your answer, they are asked to review again. Bots are left out.

## 0.3.5

### Unblock
- A new Inbox section, Unblock, holds your open pull requests that cannot merge until you clear something: conflicts, a branch behind its base, or failing checks. An approved pull request in that state leaves Merge these until it is cleared. The sections you dragged go back to the default order once, because the list of sections changed.
- Each row there has a button for it. Update branch brings a branch up to date with its base. Conflicts and failed checks take code to clear, so their button is Copy prompt: it copies a short prompt to paste to your coding agent, naming the pull request, the failed checks with links to their logs, and what to do. In other sections the same buttons show while the pointer is on the row.
- The row names the first failed check ("CI failing: lint +2"). An opened row lists the failed checks as links to their logs, with a link that runs the failed GitHub Actions jobs again, whether the branch is behind or in conflict, who approved and who is still to review, and the changed files.
- Review comments waiting for your answer count as feedback: the pull request goes to Address feedback and says how many ("2 comments to answer"), until someone approves. Threads you answered last, resolved threads and bots' threads do not count.

### Review requests
- A request that has waited a day or more says for how long ("Waiting on you 3d"), in amber once it is as old as your own nudge limit. One made after you already reviewed says "Review requested again".
- Conflicts and failing checks on someone else's pull request are shown in grey, with a tooltip saying they are the author's to clear.
- Your own pull request waiting for review names who it waits on ("Waiting on kai +1").

### Notifications
- New ones for your pull requests: checks failed (naming them), conflicts appeared, comments wait for your answer, and "can merge" when an approved pull request becomes mergeable.
- Being asked to review again after you reviewed notifies again ("acme/cli#904 is back for your thumb"), and so do a second approval by the same person and a second round of changes requested. Before, each of these notified once per pull request.

### Sync
- A sync asks GitHub for 10 pull requests at a time instead of 30, and asks once more for a page that failed. With checks and review threads included, pages of 30 ran into GitHub's ten-second limit. A long list of review requests takes more round trips, so its sync takes longer.

## 0.3.4

- Approve and Merge are quieter at rest: no fill, only the icon in green or violet. The first press fills the button and a thin bar drains until the second press times out; when it lands, "Approved" or "Merged" pops in once and the row washes in its color for a moment.
- Every new approval on your pull requests notifies you ("@kai approved acme/cli#908"), including second approvals and approvals in repositories that don't require reviews.

## 0.3.1

- First launch opens with an intro: a finger presses the approve button, and the logo and name fly into the header. A click or Escape skips it; Reduce Motion turns it off.
- Setup is one screen. Authenticate with GitHub CLI signs in from inside the app and shows gh's one-time code; Terminal no longer opens. A switch turns notifications on (they start off), and Open inbox finishes setup.

## 0.3.0

### Inbox
- The Inbox is about your pull requests and the review requests that name you. Team requests leave it (a Settings switch brings them back) and stay under Theirs and All.
- Review requests are split into sections: Merge these, Hotfixes, Security, Do first, Read carefully, Approve at will, Big reviews and Review these. Your own pull requests keep Address feedback, Get a reviewer and Finish or close.
- Three of those sections can use a decision model's yes/no answers (a brain marks them). Rules answer first, answers are cached, and a spending cap applies. Once it is on, pull requests in private repositories are sent too. Jev, through OpenRouter or TypeSafe, and Kev-4B under llama.cpp are supported, and so is any chat model that returns log-probabilities. `meat-proxy --judge-eval` prints the result on your own pull requests.
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
