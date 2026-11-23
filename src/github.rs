use std::process::Command;

use crate::model::{
    Ask, AutoMerge, Check, CiStatus, IncomingReason, MergeMethod, MergeReadiness, NotificationEvent,
    NotificationKind, Ownership, PullRequest, RepoConfig, ReviewState, WatchRule,
};

#[derive(Clone, Debug)]
pub enum SyncStatus {
    Connected { username: String },
    NeedsAuth { reason: String },
    Checking,
}

#[derive(Clone, Debug)]
pub struct SyncOutput {
    pub status: SyncStatus,
    pub prs: Vec<PullRequest>,
    /// The user's own rules whose search failed, with why; the rest of the sync stands.
    pub failed_rules: Vec<String>,
}

pub const DEFAULT_POLL_INTERVAL: u64 = 60;
const RECENT_PR_HOURS: u64 = 30 * 24;

pub struct NotificationPoll {
    pub modified: Option<String>,
    pub interval: u64,
    pub events: Vec<NotificationEvent>,
}

pub(crate) fn gh_command() -> Command {
    let mut cmd = Command::new("gh");
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    // A relative or empty entry would find a `gh` in whatever directory the app was started from.
    let mut paths: Vec<_> = std::env::split_paths(&inherited_path)
        .filter(|path| path.is_absolute())
        .collect();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(std::path::PathBuf::from(home).join(".local/share/mise/shims"));
    }
    paths.extend(["/opt/homebrew/bin".into(), "/usr/local/bin".into()]);
    if let Ok(path) = std::env::join_paths(paths) {
        cmd.env("PATH", path);
    }
    cmd
}

/// How long one gh call may take. GitHub stops a query after about ten seconds; past a minute gh
/// is stuck (a dead network, a prompt no one sees), and a stuck call would hold up every later
/// sync, which waits for the one before it.
const GH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

trait RunGh {
    /// Runs gh to the end and returns what it printed, or stops it after `GH_TIMEOUT`.
    fn run_gh(&mut self) -> Result<std::process::Output, String>;
}

impl RunGh for Command {
    fn run_gh(&mut self) -> Result<std::process::Output, String> {
        use std::io::Read;
        use std::process::Stdio;
        let mut child = self
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not run gh: {e}"))?;
        // Read both pipes while waiting, so a long answer cannot fill one and stall gh.
        fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buf);
                }
                buf
            })
        }
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let deadline = std::time::Instant::now() + GH_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| format!("Could not run gh: {e}"))? {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("gh gave no answer in {} seconds", GH_TIMEOUT.as_secs()));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        Ok(std::process::Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }
}

/// The first line gh printed to stderr, or `fallback` when it printed nothing.
fn gh_error(out: &std::process::Output, fallback: &str) -> String {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

/// Signs the GitHub CLI in through the browser without a terminal. Each line gh prints is
/// appended to `lines` as it arrives, and the device page opens once gh shows its one-time code
/// (which `--clipboard` copies). Returns when gh finishes.
pub fn login_in_browser(
    lines: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;
    let mut child = gh_command()
        .args([
            "auth",
            "login",
            "--hostname",
            "github.com",
            "--web",
            "--clipboard",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not run gh. {error}"))?;
    let opened = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let follow = |stream: Box<dyn Read + Send>| {
        let (lines, opened) = (lines.clone(), opened.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let line = line.trim().to_owned();
                if line.is_empty() {
                    continue;
                }
                if line.contains("github.com/login/device")
                    && !opened.swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    crate::notify::open_url_in_browser("https://github.com/login/device", true);
                }
                lines.lock().unwrap().push(line);
            }
        })
    };
    let readers = [
        child.stdout.take().map(|s| follow(Box::new(s))),
        child.stderr.take().map(|s| follow(Box::new(s))),
    ];
    for reader in readers.into_iter().flatten() {
        let _ = reader.join();
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(lines
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_else(|| "gh auth login failed.".to_owned()))
    }
}

/// GitHub's largest page of notifications, and how many pages one check reads at most.
const NOTIFICATIONS_PER_PAGE: usize = 50;
const NOTIFICATIONS_PAGES: usize = 3;

pub fn check_notifications(last_modified: Option<&str>) -> Result<NotificationPoll, String> {
    let mut cmd = gh_command();
    cmd.args([
        "api",
        "--hostname",
        "github.com",
        &format!("/notifications?participating=true&per_page={NOTIFICATIONS_PER_PAGE}"),
        "-i",
    ]);
    if let Some(lm) = last_modified {
        cmd.args(["-H", &format!("If-Modified-Since: {lm}")]);
    }
    let out = cmd.run_gh()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (headers, body) = stdout
        .split_once("\r\n\r\n")
        .or_else(|| stdout.split_once("\n\n"))
        .unwrap_or((&stdout, ""));
    let header = |key: &str| {
        headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case(key).then(|| value.trim())
        })
    };
    let interval = header("x-poll-interval")
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_POLL_INTERVAL)
        .max(15);
    let modified = header("last-modified").or(last_modified).map(str::to_owned);
    if headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        == Some("304")
    {
        return Ok(NotificationPoll {
            modified,
            interval,
            events: Vec::new(),
        });
    }
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }

    let mut items: Vec<serde_json::Value> = serde_json::from_str(body).map_err(|e| e.to_string())?;
    // A full page may have more behind it. A few more pages cover a busy morning; the full sync
    // each minute finds anything older.
    let mut page = 1;
    let mut last = items.len();
    while last == NOTIFICATIONS_PER_PAGE && page < NOTIFICATIONS_PAGES {
        page += 1;
        let out = gh_command()
            .args([
                "api",
                "--hostname",
                "github.com",
                &format!("/notifications?participating=true&per_page={NOTIFICATIONS_PER_PAGE}&page={page}"),
            ])
            .run_gh()?;
        if !out.status.success() {
            break;
        }
        let more: Vec<serde_json::Value> =
            serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        last = more.len();
        items.extend(more);
    }
    let mut events = Vec::new();
    for item in items {
        let reason = item["reason"].as_str().unwrap_or_default();
        let subject = &item["subject"];
        if !matches!(reason, "review_requested" | "assign")
            || item["unread"].as_bool() != Some(true)
            || subject["type"].as_str() != Some("PullRequest")
        {
            continue;
        }
        let Some(repo) = item["repository"]["full_name"].as_str() else {
            continue;
        };
        if crate::profile::hide_private() && item["repository"]["private"].as_bool() != Some(false) {
            continue;
        }
        let Some(number) = subject["url"]
            .as_str()
            .and_then(|url| url.rsplit('/').next())
            .and_then(|number| number.parse::<u64>().ok())
        else {
            continue;
        };
        events.push(NotificationEvent {
            id: format!("assign-{repo}#{number}"),
            title: if reason == "review_requested" {
                format!("{repo}#{number} is waiting for your thumb")
            } else {
                format!("Assigned to {repo}#{number}")
            },
            body: subject["title"]
                .as_str()
                .unwrap_or("Pull request")
                .to_owned(),
            pr_url: format!("https://github.com/{repo}/pull/{number}"),
            kind: NotificationKind::InstantAssigned,
            timestamp_label: "just now".to_string(),
            unread: true,
        });
    }

    Ok(NotificationPoll {
        modified,
        interval,
        events,
    })
}

/// A repository the user has recently worked in, found through their GitHub contributions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredRepo {
    pub name: String,
    pub prs_90d: u32,
    pub reviews_90d: u32,
    pub prs_30d: u32,
    pub reviews_30d: u32,
}

impl DiscoveredRepo {
    /// Recent work weighs more than older work; reviews count a little less than authored PRs.
    pub fn activity(&self) -> u32 {
        let recent = self.prs_30d * 3 + self.reviews_30d * 2;
        let quarter = self.prs_90d + self.reviews_90d / 2;
        recent + quarter
    }

    pub fn owner(&self) -> &str {
        self.name.split('/').next().unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Discovery {
    /// Organizations the signed-in account belongs to.
    pub organizations: Vec<String>,
    /// Most active first.
    pub repos: Vec<DiscoveredRepo>,
}

fn utc_days_ago(days: u64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        .saturating_sub(days * 86_400);
    let days_since_epoch = (secs / 86_400) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T00:00:00Z")
}

/// One request for the account's organizations and where it opened or reviewed PRs lately.
pub fn discover_repos() -> Result<Discovery, String> {
    let window = |alias: &str, days: u64| {
        format!(
            "{alias}: contributionsCollection(from: \"{}\") {{
               pullRequestContributionsByRepository(maxRepositories: 50) {{
                 repository {{ nameWithOwner isArchived isPrivate }} contributions(first: 1) {{ totalCount }}
               }}
               pullRequestReviewContributionsByRepository(maxRepositories: 50) {{
                 repository {{ nameWithOwner isArchived isPrivate }} contributions(first: 1) {{ totalCount }}
               }}
             }}",
            utc_days_ago(days)
        )
    };
    let query = format!(
        "query {{ viewer {{ organizations(first: 50) {{ nodes {{ login }} }} {} {} }} }}",
        window("d90", 90),
        window("d30", 30)
    );
    let data = fetch_graphql(&query)?;
    let viewer = &data["viewer"];

    let mut repos: Vec<DiscoveredRepo> = Vec::new();
    for (alias, days30) in [("d90", false), ("d30", true)] {
        for (key, is_review) in [
            ("pullRequestContributionsByRepository", false),
            ("pullRequestReviewContributionsByRepository", true),
        ] {
            let Some(items) = viewer[alias][key].as_array() else {
                continue;
            };
            for item in items {
                if item["repository"]["isArchived"].as_bool() == Some(true) {
                    continue;
                }
                if crate::profile::hide_private() && item["repository"]["isPrivate"].as_bool() != Some(false) {
                    continue;
                }
                let Some(name) = item["repository"]["nameWithOwner"].as_str() else {
                    continue;
                };
                let count = item["contributions"]["totalCount"].as_u64().unwrap_or(0) as u32;
                let idx = repos
                    .iter()
                    .position(|r| r.name == name)
                    .unwrap_or_else(|| {
                        repos.push(DiscoveredRepo {
                            name: name.to_owned(),
                            prs_90d: 0,
                            reviews_90d: 0,
                            prs_30d: 0,
                            reviews_30d: 0,
                        });
                        repos.len() - 1
                    });
                let repo = &mut repos[idx];
                match (days30, is_review) {
                    (false, false) => repo.prs_90d = count,
                    (false, true) => repo.reviews_90d = count,
                    (true, false) => repo.prs_30d = count,
                    (true, true) => repo.reviews_30d = count,
                }
            }
        }
    }
    repos.sort_by(|a, b| {
        b.activity()
            .cmp(&a.activity())
            .then_with(|| a.name.cmp(&b.name))
    });

    let organizations = viewer["organizations"]["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n["login"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(Discovery {
        organizations,
        repos,
    })
}

/// Confirms a repository exists and is readable; returns its canonical `owner/name`.
pub fn verify_repo(input: &str) -> Result<String, String> {
    let name = input
        .trim()
        .trim_start_matches("https://github.com/")
        .trim_matches('/');
    let mut parts = name.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("Use the owner/name form, like saleor/saleor.".to_string());
    };
    if owner.is_empty() || repo.is_empty() {
        return Err("Use the owner/name form, like saleor/saleor.".to_string());
    }
    let out = gh_command()
        .args([
            "api",
            "--hostname",
            "github.com",
            &format!("repos/{owner}/{repo}"),
            "--jq",
            ".full_name",
        ])
        .run_gh()?;
    if !out.status.success() {
        // Only a 404 means the name is wrong; a lapsed login or a dead network says so itself.
        let error = gh_error(&out, "");
        if error.is_empty() || error.contains("Not Found") || error.contains("404") {
            return Err(format!("GitHub can't find {owner}/{repo}."));
        }
        return Err(error);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

const PR_FIELDS: &str = r#"
    fragment PrFields on PullRequest {
      id
      number
      title
      url
      isDraft
      createdAt
      updatedAt
      baseRefName
      headRepository { nameWithOwner }
      headRefName
      headRefOid
      additions
      deletions
      author { login }
      authorAssociation
      bodyText
      body
      labels(first: 6) { nodes { name } }
      files(first: $files) { nodes { path } }
      state
      mergeable
      mergeStateStatus
      autoMergeRequest { enabledAt }
      viewerCanEnableAutoMerge
      viewerCanUpdateBranch
      viewerLatestReview { submittedAt }
      repository { nameWithOwner isArchived isPrivate viewerDefaultMergeMethod defaultBranchRef { name } }
      reviewDecision
      reviewRequests(first: 20) {
        nodes {
          requestedReviewer {
            ... on User { login }
            ... on Team { name }
          }
        }
      }
      decisions: reviews(last: 20, states: [APPROVED, CHANGES_REQUESTED, DISMISSED]) {
        nodes {
          author { login }
          state
          submittedAt
          commit { oid }
        }
      }
      reviews(last: 6) {
        nodes {
          author { login }
          state
          submittedAt
        }
      }
      asks: timelineItems(last: 10, itemTypes: [REVIEW_REQUESTED_EVENT]) {
        nodes {
          ... on ReviewRequestedEvent {
            createdAt
            requestedReviewer { ... on User { login } }
          }
        }
      }
      reviewThreads(last: 20) @include(if: $own) {
        pageInfo { hasPreviousPage }
        nodes {
          isResolved
          comments(last: 1) { nodes { author { __typename login } } }
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            committedDate
            statusCheckRollup {
              state
              contexts(first: 20) {
                nodes {
                  __typename
                  ... on CheckRun { name conclusion detailsUrl }
                  ... on StatusContext { context state targetUrl }
                }
              }
            }
          }
        }
      }
    }
    "#;

/// How many changed file paths a sync asks for per pull request; one with this many has more.
pub const FILES_FETCHED: usize = 40;

struct Search {
    query: String,
    reason: IncomingReason,
    label: Option<String>,
    /// Keep only PRs in repositories the user tracks.
    tracked_only: bool,
}

/// Repositories at least this active get all of their fresh PRs, not just the ones involving the user.
const BUSY_REPO_ACTIVITY: u32 = 30;
const BUSY_REPO_LIMIT: usize = 3;
const FRESH_PR_DAYS: u64 = 7;

/// Which pull requests the sync brings in. Every choice has a default and a switch in Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncOptions {
    /// Skip PRs in archived repositories.
    pub hide_archived: bool,
    /// Skip PRs with no activity for this many days. 0 keeps everything.
    pub max_age_days: u32,
}

pub const DEFAULT_MAX_AGE_DAYS: u32 = 30;

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions {
            hide_archived: true,
            max_age_days: DEFAULT_MAX_AGE_DAYS,
        }
    }
}

pub fn fetch_live(
    repos: &[RepoConfig],
    rules: &[WatchRule],
    options: SyncOptions,
) -> Result<SyncOutput, String> {
    let incoming = IncomingReason::AssignedOrReviewRequested;
    let mut searches = vec![
        Search {
            query: "is:pr is:open author:@me".into(),
            reason: IncomingReason::NotIncoming,
            label: None,
            tracked_only: false,
        },
        Search {
            query: "is:pr is:open -author:@me -is:draft review-requested:@me".into(),
            reason: incoming,
            label: Some("Review Requested".into()),
            tracked_only: false,
        },
        Search {
            query: "is:pr is:open -author:@me -is:draft assignee:@me".into(),
            reason: incoming,
            label: Some("Assigned".into()),
            tracked_only: false,
        },
    ];
    {
        // Others' PRs the user has already commented on, reviewed or been mentioned in.
        searches.push(Search {
            query: "is:pr is:open -author:@me -is:draft involves:@me".into(),
            reason: IncomingReason::SubscribedRule,
            label: Some("Involved".into()),
            tracked_only: true,
        });
    }
    // In the repositories the user is busiest in, anything recently active is worth seeing.
    let fresh_since = &utc_days_ago(FRESH_PR_DAYS)[..10];
    let mut busy: Vec<_> = repos
        .iter()
        .filter(|repo| !repo.removed && repo.activity >= BUSY_REPO_ACTIVITY)
        .collect();
    busy.sort_by_key(|pr| std::cmp::Reverse(pr.activity));
    searches.extend(busy.into_iter().take(BUSY_REPO_LIMIT).map(|repo| Search {
        query: format!(
            "is:pr is:open -is:draft -author:@me repo:{} updated:>={fresh_since}",
            repo.name
        ),
        reason: IncomingReason::SubscribedRule,
        label: Some("Active repo".into()),
        tracked_only: false,
    }));
    // A rule is the user's own query, and a mistake in one must not stop the whole sync.
    let first_rule = searches.len();
    searches.extend(rules.iter().filter(|rule| rule.enabled).map(|rule| Search {
        query: rule.to_github_search_query(),
        reason: IncomingReason::SubscribedRule,
        label: Some(rule.name.clone()),
        tracked_only: false,
    }));

    // Two rounds. Each search, on its own thread, lists only the IDs it finds, which is cheap
    // enough to ask for 100 at a time. Then every pull request found, once however many searches
    // found it, has its fields fetched in small batches side by side. Asking each search for the
    // fields page by page queued its pages behind each other and fetched the same pull request
    // once per search that found it.
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = searches
            .iter()
            .map(|search| scope.spawn(move || search_ids(search, options)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("A GitHub search stopped unexpectedly".to_string()))
            })
            .collect::<Vec<_>>()
    });
    let mut failed_rules = Vec::new();
    let mut found = Vec::new();
    for (at, (search, result)) in searches.iter().zip(results).enumerate() {
        match result {
            Ok(ids) => found.push(ids),
            Err(error) if at >= first_rule => {
                failed_rules.push(format!("{}: {error}", search.label.as_deref().unwrap_or("rule")));
                found.push((String::new(), Vec::new()));
            }
            Err(error) => return Err(error),
        }
    }

    let mut viewer = String::new();
    let mut own_ids = Vec::new();
    let mut other_ids = Vec::new();
    for (search, (login, ids)) in searches.iter().zip(&found) {
        if !login.is_empty() {
            viewer.clone_from(login);
        }
        for id in ids {
            if own_ids.contains(id) || other_ids.contains(id) {
                continue;
            }
            // The user's own search comes first, so their pull requests land here.
            if search.reason == IncomingReason::NotIncoming {
                own_ids.push(id.clone());
            } else {
                other_ids.push(id.clone());
            }
        }
    }
    let nodes = fetch_nodes(&own_ids, &other_ids)?;

    let mut prs = Vec::<PullRequest>::new();
    for (search, (_, ids)) in searches.iter().zip(&found) {
        for node in ids.iter().filter_map(|id| nodes.get(id)) {
            let mine = node["author"]["login"]
                .as_str()
                .is_some_and(|login| login.eq_ignore_ascii_case(&viewer));
            let ownership = if mine {
                Ownership::Mine
            } else {
                Ownership::Theirs
            };
            let mut reason = if mine {
                IncomingReason::NotIncoming
            } else {
                search.reason
            };
            // A review requested only through a team you belong to matters less than
            // one addressed to you by name.
            if reason == incoming
                && search.label.as_deref() == Some("Review Requested")
                && !requested_by_name(node, &viewer)
            {
                reason = IncomingReason::TeamReviewRequested;
            }
            let Some(pr) = parse_graphql_pr(
                node,
                ownership,
                reason,
                search.label.as_deref(),
                options.hide_archived,
            ) else {
                continue;
            };
            if !pr.within_days(options.max_age_days) {
                continue;
            }
            let mut pr = pr;
            pr.asked = asked(node, &viewer);
            if search.tracked_only && !repos.iter().any(|r| !r.removed && r.name == pr.repo) {
                continue;
            }
            if let Some(existing) = prs.iter_mut().find(|p| p.id == pr.id) {
                if reason == incoming
                    || (reason == IncomingReason::TeamReviewRequested
                        && existing.incoming_reason == IncomingReason::SubscribedRule)
                {
                    existing.incoming_reason = reason;
                }
                if existing.matched_rule.is_none() {
                    existing.matched_rule = pr.matched_rule;
                }
            } else {
                prs.push(pr);
            }
        }
    }

    Ok(SyncOutput {
        status: SyncStatus::Connected { username: viewer },
        prs,
        failed_rules,
    })
}

/// Whether the PR's review requests include `login` itself, not just a team they are in.
fn requested_by_name(node: &serde_json::Value, login: &str) -> bool {
    node["reviewRequests"]["nodes"]
        .as_array()
        .is_some_and(|nodes| {
            nodes.iter().any(|n| {
                n["requestedReviewer"]["login"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(login))
            })
        })
}

/// The last time `login` was asked by name to review this pull request.
fn asked(node: &serde_json::Value, login: &str) -> Option<Ask> {
    let at = node["asks"]["nodes"]
        .as_array()?
        .iter()
        .filter(|ask| {
            ask["requestedReviewer"]["login"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(login))
        })
        .filter_map(|ask| ask["createdAt"].as_str())
        .max()?;
    Some(Ask {
        at: at.to_owned(),
        hours_ago: estimate_hours_since_iso8601(at),
        // Both are UTC timestamps in one format, so they compare as text.
        again: node["viewerLatestReview"]["submittedAt"]
            .as_str()
            .is_some_and(|reviewed| reviewed < at),
    })
}

/// The checks of the last commit that ended badly, with where to read why.
fn failing_checks(node: &serde_json::Value) -> Vec<Check> {
    let Some(contexts) =
        node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"].as_array()
    else {
        return Vec::new();
    };
    let mut out: Vec<Check> = Vec::new();
    for context in contexts {
        // A check run has a conclusion and a status context a state; one of the two is set.
        let (name, verdict, url) = if context["__typename"].as_str() == Some("CheckRun") {
            (&context["name"], &context["conclusion"], &context["detailsUrl"])
        } else {
            (&context["context"], &context["state"], &context["targetUrl"])
        };
        let failed = matches!(
            verdict.as_str(),
            Some("FAILURE" | "ERROR" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED")
        );
        if let Some(name) = name.as_str().filter(|_| failed)
            && !out.iter().any(|known| known.name == name)
        {
            out.push(Check {
                name: name.to_owned(),
                url: url.as_str().unwrap_or_default().to_owned(),
            });
        }
    }
    out
}

/// Review threads still open whose last comment is neither the author's nor a bot's: the ones
/// waiting for the author. None when the threads were not asked for, or when there are more than
/// were fetched and none of those waits: an older one may.
fn unanswered_threads(node: &serde_json::Value, author: &str) -> Option<u32> {
    let threads = node["reviewThreads"]["nodes"].as_array()?;
    let cut_off = node["reviewThreads"]["pageInfo"]["hasPreviousPage"].as_bool() == Some(true);
    let waiting = threads
        .iter()
        .filter(|thread| thread["isResolved"].as_bool() == Some(false))
        .filter(|thread| {
            let last = &thread["comments"]["nodes"][0]["author"];
            last["__typename"].as_str() != Some("Bot")
                && last["login"]
                    .as_str()
                    .is_some_and(|login| login != author && !crate::sections::is_bot(login))
        })
        .count();
    (waiting > 0 || !cut_off).then_some(waiting as u32)
}

/// Search results listed by ID alone are cheap, so a page can be long.
const ID_PAGE_SIZE: u32 = 100;

/// The most pages one search reads every minute: 200 pull requests. Past that, a search is too
/// broad to be an inbox, and reading it all would spend GitHub's rate limit on every sync.
const MAX_PAGES: usize = 2;

/// The IDs of every pull request one search finds, with the signed-in account's login.
fn search_ids(search: &Search, options: SyncOptions) -> Result<(String, Vec<String>), String> {
    // Archived repositories and old PRs are skipped server-side, which keeps the sync light. A
    // pull request is old when it was opened long ago: updated is never before created, so the
    // created date is the one that has to be inside the limit.
    let archived = if options.hide_archived {
        " archived:false"
    } else {
        ""
    };
    let recent = if options.max_age_days > 0 {
        format!(
            " created:>={}",
            &utc_days_ago(u64::from(options.max_age_days))[..10]
        )
    } else {
        String::new()
    };
    let term = serde_json::to_string(&format!("{}{archived}{recent}", search.query))
        .expect("serialize search");

    let mut ids = Vec::new();
    let mut seen_viewer: Option<String> = None;
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let after = serde_json::to_string(&cursor).expect("serialize cursor");
        let query = format!(
            "query {{ viewer {{ login }} s0: search(query: {term}, type: ISSUE, first: {ID_PAGE_SIZE}, after: {after}) {{ nodes {{ ... on PullRequest {{ id }} }} pageInfo {{ hasNextPage endCursor }} }} }}"
        );
        // A page GitHub was too slow for once usually comes back the second time; any other
        // error (a lapsed login, a bad query) would fail the same way again.
        let data = fetch_graphql(&query).or_else(|error| {
            if is_transient(&error) {
                fetch_graphql(&query)
            } else {
                Err(error)
            }
        })?;
        let viewer = data["viewer"]["login"]
            .as_str()
            .ok_or_else(|| "GitHub did not return the signed-in account".to_string())?
            .to_owned();
        seen_viewer = Some(viewer.clone());
        let page = &data["s0"];
        ids.extend(
            page["nodes"]
                .as_array()
                .ok_or_else(|| "GitHub did not return search results".to_string())?
                .iter()
                .filter_map(|node| node["id"].as_str().map(str::to_owned)),
        );
        if page["pageInfo"]["hasNextPage"].as_bool() != Some(true) {
            return Ok((viewer, ids));
        }
        let next = page["pageInfo"]["endCursor"]
            .as_str()
            .filter(|next| Some(*next) != cursor.as_deref())
            .ok_or_else(|| "GitHub returned an invalid search cursor".to_string())?;
        cursor = Some(next.to_owned());
    }
    Ok((seen_viewer.unwrap_or_default(), ids))
}

/// An error worth one more try: GitHub ran out of time, or the network dropped.
fn is_transient(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    ["502", "503", "504", "timeout", "timed out", "no answer", "something went wrong", "connection"]
        .iter()
        .any(|sign| error.contains(sign))
}

/// GitHub gives a query about ten seconds and answers 502 past that, and the fields of each pull
/// request, its checks and reviews most of all, are what cost. Batches of 5 came back in about
/// three seconds; batches of 10 took a second or two longer than running twice as many of 5.
const BATCH_SIZE: usize = 5;
/// How many batches are asked for at once, so a long list does not open dozens of requests.
const PARALLEL_BATCHES: usize = 8;

/// The fields of each pull request by node ID. Review threads cost the most to ask for and only
/// say something about the user's own pull requests, so only `own` asks for them.
fn fetch_nodes(
    own: &[String],
    others: &[String],
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    let batches: Vec<(bool, &[String])> = own
        .chunks(BATCH_SIZE)
        .map(|ids| (true, ids))
        .chain(others.chunks(BATCH_SIZE).map(|ids| (false, ids)))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let fetched: Vec<Result<Vec<serde_json::Value>, String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..PARALLEL_BATCHES.min(batches.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut nodes = Vec::new();
                    while let Some((own, ids)) =
                        batches.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                    {
                        let fields = PR_FIELDS
                            .replace("$own", if *own { "true" } else { "false" })
                            .replace("$files", &FILES_FETCHED.to_string());
                        let ids = serde_json::to_string(ids).expect("serialize ids");
                        let query =
                            format!("query {{ nodes(ids: {ids}) {{ ...PrFields }} }}{fields}");
                        let mut data = fetch_graphql(&query).or_else(|error| {
                            if is_transient(&error) { fetch_graphql(&query) } else { Err(error) }
                        })?;
                        let serde_json::Value::Array(batch) = data["nodes"].take() else {
                            return Err("GitHub did not return the pull requests".to_string());
                        };
                        nodes.extend(batch);
                    }
                    Ok(nodes)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| {
                w.join()
                    .unwrap_or_else(|_| Err("A GitHub request stopped unexpectedly".to_string()))
            })
            .collect()
    });
    let mut by_id = std::collections::HashMap::new();
    for nodes in fetched {
        for node in nodes? {
            if let Some(id) = node["id"].as_str() {
                by_id.insert(id.to_owned(), node);
            }
        }
    }
    Ok(by_id)
}

fn fetch_graphql(query: &str) -> Result<serde_json::Value, String> {
    let out = gh_command()
        .args([
            "api",
            "--hostname",
            "github.com",
            "graphql",
            "-f",
            &format!("query={query}"),
        ])
        .run_gh()?;

    // gh fails when the answer has errors, but still prints it: GitHub answers what it can and
    // leaves out what it may not show (an organization that wants single sign-on, say).
    let Ok(response) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return Err(if out.status.success() {
            "GitHub answered with something that is not JSON".to_owned()
        } else {
            gh_error(&out, "GitHub request failed")
        });
    };
    graphql_data(response).map_err(|error| {
        if out.status.success() { error } else { gh_error(&out, &error) }
    })
}

/// The data of a GraphQL answer, when it has any; else its first error.
fn graphql_data(mut response: serde_json::Value) -> Result<serde_json::Value, String> {
    let data = response["data"].take();
    if data.is_object() {
        return Ok(data);
    }
    Err(response["errors"][0]["message"]
        .as_str()
        .unwrap_or("GitHub response is missing data")
        .to_owned())
}

fn parse_graphql_pr(
    node: &serde_json::Value,
    ownership: Ownership,
    reason: IncomingReason,
    rule_label: Option<&str>,
    hide_archived: bool,
) -> Option<PullRequest> {
    // Search results can lag behind: a PR merged a moment ago may still be listed as open.
    if node["state"].as_str().is_some_and(|state| state != "OPEN") {
        return None;
    }
    // Nothing happens in an archived repository anymore.
    if hide_archived && node["repository"]["isArchived"].as_bool() == Some(true) {
        return None;
    }
    let number = node.get("number")?.as_u64()?;
    let title = node.get("title")?.as_str()?.to_string();
    let url = node.get("url")?.as_str()?.to_string();
    let repo = node
        .get("repository")?
        .get("nameWithOwner")?
        .as_str()?
        .to_string();
    let author = node
        .get("author")
        .and_then(|a| a.get("login"))
        .and_then(|l| l.as_str())
        .unwrap_or("ghost")
        .to_string();
    let is_draft = node
        .get("isDraft")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let head_ref = node
        .get("headRefName")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let additions = node.get("additions").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let deletions = node.get("deletions").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    let review_state = match node.get("reviewDecision").and_then(|v| v.as_str()) {
        Some("APPROVED") => ReviewState::Approved,
        Some("CHANGES_REQUESTED") => ReviewState::ChangesRequested,
        Some("REVIEW_REQUIRED") => ReviewState::ReviewRequested,
        _ => ReviewState::None,
    };

    let ci_status = match node
        .get("commits")
        .and_then(|c| c.get("nodes"))
        .and_then(|n| n.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("commit"))
        .and_then(|c| c.get("statusCheckRollup"))
        .and_then(|s| s.get("state"))
        .and_then(|s| s.as_str())
    {
        Some("SUCCESS") => CiStatus::Passing,
        Some("PENDING" | "EXPECTED") => CiStatus::Pending,
        Some("FAILURE" | "ERROR") => CiStatus::Failing,
        _ => CiStatus::None,
    };

    let mut reviewers = Vec::new();
    if let Some(reqs) = node
        .get("reviewRequests")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for req in reqs {
            if let Some(user) = req
                .get("requestedReviewer")
                .and_then(|u| u.get("login").or_else(|| u.get("name")))
                .and_then(|l| l.as_str())
                && !reviewers.iter().any(|r| r == user)
            {
                reviewers.push(user.to_string());
            }
        }
    }
    let mut review_clock = node["createdAt"].as_str().unwrap_or_default();
    if let Some(revs) = node
        .get("reviews")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for rev in revs {
            if rev["author"]["login"]
                .as_str()
                .is_some_and(|login| login != author)
                && let Some(submitted) = rev["submittedAt"].as_str()
            {
                review_clock = review_clock.max(submitted);
            }
            if let Some(user) = rev
                .get("author")
                .and_then(|u| u.get("login"))
                .and_then(|l| l.as_str())
                && user != author
                && !reviewers.iter().any(|r| r == user)
            {
                reviewers.push(user.to_string());
            }
        }
    }

    // Who approved: the people whose latest deciding review is an approval.
    let mut verdicts: Vec<(String, String, String)> = Vec::new();
    // Whose latest verdict is on the head commit: they have seen the last push. Commit dates are
    // the author's to set and survive a rebase, so the commit itself says it.
    let head = node["headRefOid"].as_str().unwrap_or_default();
    let mut seen_head: Vec<String> = Vec::new();
    if let Some(nodes) = node["decisions"]["nodes"].as_array() {
        for rev in nodes {
            let (Some(login), Some(state)) =
                (rev["author"]["login"].as_str(), rev["state"].as_str())
            else {
                continue;
            };
            if login == author {
                continue;
            }
            verdicts.retain(|(who, _, _)| who != login);
            seen_head.retain(|who| who != login);
            if !head.is_empty() && rev["commit"]["oid"].as_str() == Some(head) {
                seen_head.push(login.to_owned());
            }
            let at = rev["submittedAt"].as_str().unwrap_or_default();
            verdicts.push((login.to_owned(), state.to_owned(), at.to_owned()));
        }
    }
    let approved_by: Vec<String> = verdicts
        .iter()
        .filter(|(_, state, _)| state == "APPROVED")
        .map(|(who, _, _)| who.clone())
        .collect();
    let approved_at = verdicts
        .iter()
        .filter(|(_, state, _)| state == "APPROVED")
        .map(|(_, _, at)| at.clone())
        .max()
        .unwrap_or_default();
    let changes_requested: Vec<(String, String)> = verdicts
        .into_iter()
        .filter(|(who, state, at)| {
            state == "CHANGES_REQUESTED" && !at.is_empty() && !seen_head.contains(who)
        })
        .map(|(who, _, at)| (who, at))
        .collect();
    let requested_reviewers: Vec<String> = node["reviewRequests"]["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n["requestedReviewer"]["login"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let unanswered_threads = unanswered_threads(node, &author);
    let updated_at = node
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let idle_hours = estimate_hours_since_iso8601(updated_at);

    Some(PullRequest {
        id: format!("{repo}#{number}"),
        repo,
        number,
        title,
        url,
        author,
        ownership,
        is_draft,
        review_state,
        ci_status,
        incoming_reason: reason,
        reviewers,
        approved_by,
        idle_hours,
        opened_hours: estimate_hours_since_iso8601(node["createdAt"].as_str().unwrap_or_default()),
        unreviewed_hours: estimate_hours_since_iso8601(review_clock),
        head_ref,
        head_repo: node["headRepository"]["nameWithOwner"]
            .as_str()
            .map(str::to_owned),
        stack_base: node["baseRefName"]
            .as_str()
            .filter(|base| {
                node["repository"]["defaultBranchRef"]["name"]
                    .as_str()
                    .is_some_and(|default| default != *base)
            })
            .map(str::to_owned),
        matched_rule: rule_label.map(str::to_owned),
        snoozed_until_hour: 0,
        additions,
        deletions,
        merge_readiness: MergeReadiness::from_github(
            node["mergeable"].as_str().unwrap_or_default(),
            node["mergeStateStatus"].as_str().unwrap_or_default(),
        ),
        auto_merge: if node["autoMergeRequest"].is_object() {
            AutoMerge::On
        } else if node["viewerCanEnableAutoMerge"].as_bool() == Some(true) {
            AutoMerge::Available
        } else {
            AutoMerge::Unavailable
        },
        merge_method: MergeMethod::from_github(
            node["repository"]["viewerDefaultMergeMethod"]
                .as_str()
                .unwrap_or_default(),
        ),
        external: matches!(
            node["authorAssociation"].as_str(),
            Some("CONTRIBUTOR" | "FIRST_TIME_CONTRIBUTOR" | "FIRST_TIMER" | "NONE")
        ),
        body: node["bodyText"].as_str().unwrap_or_default().to_owned(),
        description: node["body"].as_str().unwrap_or_default().to_owned(),
        labels: node["labels"]["nodes"]
            .as_array()
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|l| l["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        private_repo: node["repository"]["isPrivate"].as_bool().unwrap_or(false),
        files: node["files"]["nodes"]
            .as_array()
            .map(|files| {
                files
                    .iter()
                    .filter_map(|f| f["path"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        failing_checks: failing_checks(node),
        can_update_branch: node["viewerCanUpdateBranch"].as_bool() == Some(true),
        unanswered_threads,
        asked: None,
        head_oid: node["headRefOid"].as_str().unwrap_or_default().to_owned(),
        last_commit_at: node["commits"]["nodes"][0]["commit"]["committedDate"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        requested_reviewers,
        changes_requested,
        approved_at,
    })
}

/// Every write that approves, merges or moves a branch names `head`, the commit the list showed, and
/// GitHub refuses it once the branch has moved on, so nothing acts on code the user has not seen.
/// Saved data has no commit until the first sync.
fn require_head(head: &str) -> Result<(), String> {
    if head.is_empty() {
        return Err("Wait for the next sync: GitHub has not said which commit this is yet.".into());
    }
    Ok(())
}

/// Turn auto-merge on (merging with `method`, and only while the head is `head`) or off. GitHub
/// merges by itself once the checks and reviews the repository asks for are in.
pub fn set_auto_merge(url: &str, method: MergeMethod, enable: bool, head: &str) -> Result<(), String> {
    let mut command = gh_command();
    command.args(["pr", "merge", url]);
    if enable {
        require_head(head)?;
        command.args(["--auto", method.flag(), "--match-head-commit", head]);
    } else {
        command.arg("--disable-auto");
    }
    let out = command
        .run_gh()?;
    if out.status.success() {
        return Ok(());
    }
    Err(gh_error(&out, "GitHub would not change auto-merge on this pull request"))
}

/// OPEN, MERGED or CLOSED, for a pull request that has left the lists.
pub fn pr_state(url: &str) -> Result<String, String> {
    let out = gh_command()
        .args(["pr", "view", url, "--json", "state", "-q", ".state"])
        .run_gh()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Approves the PR at `head`, with no comment. GitHub's review API takes an approval of an older
/// commit without complaint, so the head is checked first, and the approval names the commit.
pub fn approve_pr(repo: &str, number: u64, head: &str) -> Result<(), String> {
    require_head(head)?;
    let path = format!("repos/{repo}/pulls/{number}");
    let out = gh_command()
        .args(["api", "--hostname", "github.com", &path, "--jq", ".head.sha"])
        .run_gh()?;
    if !out.status.success() {
        return Err(gh_error(&out, "GitHub would not say which commit this is"));
    }
    if String::from_utf8_lossy(&out.stdout).trim() != head {
        return Err(MOVED_ON.to_owned());
    }
    gh_api_write(
        "POST",
        &format!("{path}/reviews"),
        &["event=APPROVE".to_owned(), format!("commit_id={head}")],
    )
    .map_err(|e| e.unwrap_or_else(|| "GitHub would not take this approval".to_owned()))
}

/// What a write says when the branch has a newer commit than the list showed.
const MOVED_ON: &str = "New commits arrived since the list was loaded. Look again after the sync.";

pub fn merge_pr(url: &str, method: MergeMethod, head: &str) -> Result<(), String> {
    require_head(head)?;
    let out = gh_command()
        .args(["pr", "merge", url, method.flag(), "--match-head-commit", head])
        .run_gh()?;
    if out.status.success() {
        return Ok(());
    }
    Err(gh_error(&out, "GitHub would not merge this pull request"))
}

/// Asks these people to review the pull request again, as the re-request arrows on GitHub do.
pub fn request_reviewers(repo: &str, number: u64, logins: &[String]) -> Result<(), String> {
    let fields: Vec<String> = logins.iter().map(|l| format!("reviewers[]={l}")).collect();
    gh_api_write(
        "POST",
        &format!("repos/{repo}/pulls/{number}/requested_reviewers"),
        &fields,
    )
    .map_err(|e| e.unwrap_or_else(|| "GitHub would not ask for another review".to_owned()))
}

/// Brings the branch up to date with its base, the way GitHub's "Update branch" button does, while
/// its head is still `head`.
pub fn update_branch(repo: &str, number: u64, head: &str) -> Result<(), String> {
    require_head(head)?;
    gh_api_write(
        "PUT",
        &format!("repos/{repo}/pulls/{number}/update-branch"),
        &[format!("expected_head_sha={head}")],
    )
        .map_err(|e| e.unwrap_or_else(|| "GitHub would not update this branch".to_owned()))
}

/// A REST call that changes something and returns nothing worth reading. The error is the first
/// line gh printed, when it printed one.
fn gh_api_write(method: &str, path: &str, fields: &[String]) -> Result<(), Option<String>> {
    let mut command = gh_command();
    command.args(["api", "--hostname", "github.com", "--silent", "-X", method, path]);
    for field in fields {
        command.args(["-f", field]);
    }
    let out = command.run_gh().map_err(Some)?;
    if out.status.success() {
        return Ok(());
    }
    Err(Some(gh_error(&out, "")).filter(|e| !e.is_empty()))
}

fn estimate_hours_since_iso8601(iso: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|date| {
            (chrono::Utc::now() - date.with_timezone(&chrono::Utc))
                .num_hours()
                .max(0) as u64
        })
        .unwrap_or(0)
}

pub fn infer_repos_from_prs(prs: &[PullRequest], existing: &[RepoConfig]) -> Vec<RepoConfig> {
    let mut out = existing.to_vec();
    for cfg in &mut out {
        cfg.my_pr_volume = 0;
        cfg.assigned_review_volume = 0;
    }
    for pr in prs {
        let idx = out
            .iter()
            .position(|r| r.name == pr.repo)
            .unwrap_or_else(|| {
                let rank = out.len();
                out.push(RepoConfig {
                    name: pr.repo.clone(),
                    manual_rank: rank,
                    pinned: false,
                    my_pr_volume: 0,
                    assigned_review_volume: 0,
                    collapsed: false,
                    removed: false,
                    activity: 0,
                });
                rank
            });
        // Abandoned PRs say nothing about where the work is happening now.
        let recent = pr.idle_hours < RECENT_PR_HOURS;
        out[idx].my_pr_volume += u32::from(recent && pr.ownership == Ownership::Mine);
        out[idx].assigned_review_volume +=
            u32::from(recent && pr.incoming_reason == IncomingReason::AssignedOrReviewRequested);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_answer_with_data_and_errors_keeps_the_data() {
        // GitHub leaves out what a single-sign-on organization hides and answers the rest.
        let partial = json!({
            "data": { "s0": { "nodes": [{ "number": 1 }, null] } },
            "errors": [{ "type": "FORBIDDEN", "message": "Resource protected by SAML" }],
        });
        assert_eq!(graphql_data(partial).unwrap()["s0"]["nodes"][0]["number"], 1);
        let failed = json!({ "data": null, "errors": [{ "message": "Bad query" }] });
        assert_eq!(graphql_data(failed).unwrap_err(), "Bad query");
        assert!(is_transient("HTTP 502: Bad Gateway") && !is_transient("HTTP 401: Bad credentials"));
    }

    #[test]
    fn writes_refuse_a_pull_request_whose_commit_is_not_known() {
        // Saved data has no head commit; acting on it would act on whatever GitHub has now.
        let url = "https://github.com/o/r/pull/1";
        for result in [
            merge_pr(url, MergeMethod::Squash, ""),
            approve_pr("o/r", 1, ""),
            update_branch("o/r", 1, ""),
            set_auto_merge(url, MergeMethod::Squash, true, ""),
        ] {
            assert!(result.unwrap_err().starts_with("Wait for the next sync"));
        }
    }

    #[test]
    fn a_pull_request_carries_what_stands_between_it_and_its_merge() {
        let thread = |resolved: bool, kind: &str, login: &str| {
            json!({ "isResolved": resolved, "comments": { "nodes": [{ "author": { "__typename": kind, "login": login } }] } })
        };
        let node = json!({
            "number": 7, "title": "t", "url": "https://github.com/o/r/pull/7", "state": "OPEN",
            "repository": { "nameWithOwner": "o/r" },
            "author": { "login": "me" },
            "createdAt": "2026-11-01T00:00:00Z", "updatedAt": "2026-11-01T00:00:00Z",
            "mergeable": "MERGEABLE", "mergeStateStatus": "BLOCKED",
            "viewerCanUpdateBranch": true,
            "headRefOid": "h2",
            // kai asked for changes on an older commit, mira on the head: only kai is due again,
            // whatever dates the commits carry.
            "decisions": { "nodes": [
                { "author": { "login": "kai" }, "state": "CHANGES_REQUESTED", "submittedAt": "2026-11-09T00:00:00Z", "commit": { "oid": "h1" } },
                { "author": { "login": "mira" }, "state": "CHANGES_REQUESTED", "submittedAt": "2026-11-09T00:00:00Z", "commit": { "oid": "h2" } },
            ] },
            "viewerLatestReview": { "submittedAt": "2026-11-10T00:00:00Z" },
            "asks": { "nodes": [
                { "createdAt": "2026-11-05T00:00:00Z", "requestedReviewer": { "login": "Me" } },
                { "createdAt": "2026-11-12T00:00:00Z", "requestedReviewer": { "login": "me" } },
                { "createdAt": "2026-11-20T00:00:00Z", "requestedReviewer": { "login": "someone" } },
                { "createdAt": "2026-11-21T00:00:00Z", "requestedReviewer": {} },
            ] },
            "reviewThreads": { "nodes": [
                thread(false, "User", "kai"),
                // Answered by the author, resolved, or a bot's: none of these waits for the author.
                thread(false, "User", "me"),
                thread(true, "User", "kai"),
                thread(false, "Bot", "copilot-pull-request-reviewer"),
                thread(false, "User", "dependabot"),
            ] },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "lint", "conclusion": "FAILURE", "detailsUrl": "https://github.com/o/r/actions/runs/55/job/9" },
                { "__typename": "CheckRun", "name": "lint", "conclusion": "FAILURE", "detailsUrl": "https://github.com/o/r/actions/runs/55/job/10" },
                { "__typename": "CheckRun", "name": "unit", "conclusion": "SUCCESS", "detailsUrl": "" },
                { "__typename": "CheckRun", "name": "build", "conclusion": null, "detailsUrl": "" },
                { "__typename": "StatusContext", "context": "ci/vendor", "state": "ERROR", "targetUrl": "https://ci.example.com/1" },
                { "__typename": "StatusContext", "context": "ci/other", "state": "SUCCESS", "targetUrl": "" },
            ] } } } }] },
        });
        let mut pr =
            parse_graphql_pr(&node, Ownership::Mine, IncomingReason::NotIncoming, None, true).unwrap();
        assert!(pr.can_update_branch);
        assert_eq!(pr.unanswered_threads, Some(1));
        assert_eq!(pr.due_for_another_look(), ["kai"]);
        // More threads than were fetched, none of those waiting: an older one may be.
        let mut cut = node.clone();
        cut["reviewThreads"] = json!({ "pageInfo": { "hasPreviousPage": true }, "nodes": [thread(true, "User", "kai")] });
        let cut = parse_graphql_pr(&cut, Ownership::Mine, IncomingReason::NotIncoming, None, true).unwrap();
        assert_eq!(cut.unanswered_threads, None);
        let failed: Vec<&str> = pr.failing_checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(failed, ["lint", "ci/vendor"]);
        assert_eq!(pr.blockers(), [crate::model::Blocker::ChecksFailing]);

        // Asked by name on the 12th, after reviewing on the 10th: a second round.
        pr.asked = asked(&node, "me");
        let ask = pr.asked.unwrap();
        assert_eq!(ask.at, "2026-11-12T00:00:00Z");
        assert!(ask.again);
        // Without the threads in the answer nothing is known about them, and a first request is
        // not a second round.
        let mut bare = node.clone();
        bare.as_object_mut().unwrap().remove("reviewThreads");
        bare["viewerLatestReview"] = json!(null);
        let theirs = parse_graphql_pr(&bare, Ownership::Theirs, IncomingReason::NotIncoming, None, true);
        assert_eq!(theirs.unwrap().unanswered_threads, None);
        assert!(!asked(&bare, "me").unwrap().again);
        assert!(asked(&bare, "nobody").is_none());
    }
}

#[cfg(test)]
mod live_tests {
    #[test]
    #[ignore = "needs a signed-in gh"]
    fn discovers_repos_with_real_gh() {
        let found = super::discover_repos().expect("discovery");
        for r in found.repos.iter().take(10) {
            println!("{} {} {:?}", r.name, r.activity(), r);
        }
        println!("{:?}", found.organizations);
        assert!(!found.repos.is_empty());
        assert_eq!(
            super::verify_repo("saleor/saleor").unwrap(),
            "saleor/saleor"
        );
        assert!(super::verify_repo("saleor/does-not-exist-xyz").is_err());
        assert!(super::verify_repo("nonsense").is_err());
    }
}

#[cfg(test)]
mod live_sync_tests {
    use crate::model::RepoConfig;

    #[test]
    #[ignore = "needs a signed-in gh"]
    fn sync_with_busy_and_involved_searches() {
        let found = super::discover_repos().expect("discovery");
        let repos: Vec<RepoConfig> = found
            .repos
            .iter()
            .enumerate()
            .map(|(rank, r)| RepoConfig {
                name: r.name.clone(),
                manual_rank: rank,
                pinned: false,
                my_pr_volume: 0,
                assigned_review_volume: 0,
                collapsed: false,
                removed: false,
                activity: r.activity(),
            })
            .collect();
        let started = std::time::Instant::now();
        let out = super::fetch_live(&repos, &[], super::SyncOptions::default()).expect("sync");
        println!("{} PRs in {:?}", out.prs.len(), started.elapsed());
        for pr in out
            .prs
            .iter()
            .filter(|p| p.ownership == crate::model::Ownership::Mine)
            .take(12)
        {
            println!(
                "{} draft={} {:?} {:?}",
                pr.id, pr.is_draft, pr.merge_readiness, pr.merge_method
            );
        }
    }
}
