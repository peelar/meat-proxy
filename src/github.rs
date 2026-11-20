use std::process::Command;

use crate::model::{
    AutoMerge, CiStatus, IncomingReason, MergeMethod, MergeReadiness, NotificationEvent,
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
}

pub const DEFAULT_POLL_INTERVAL: u64 = 60;
const RECENT_PR_HOURS: u64 = 30 * 24;

pub struct NotificationPoll {
    pub modified: Option<String>,
    pub interval: u64,
    pub events: Vec<NotificationEvent>,
}

fn gh_command() -> Command {
    let mut cmd = Command::new("gh");
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<_> = std::env::split_paths(&inherited_path).collect();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(std::path::PathBuf::from(home).join(".local/share/mise/shims"));
    }
    paths.extend(["/opt/homebrew/bin".into(), "/usr/local/bin".into()]);
    if let Ok(path) = std::env::join_paths(paths) {
        cmd.env("PATH", path);
    }
    cmd
}

pub fn check_notifications(last_modified: Option<&str>) -> Result<NotificationPoll, String> {
    let mut cmd = gh_command();
    cmd.args([
        "api",
        "--hostname",
        "github.com",
        "/notifications?participating=true&per_page=50",
        "-i",
    ]);
    if let Some(lm) = last_modified {
        cmd.args(["-H", &format!("If-Modified-Since: {lm}")]);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
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

    let items: Vec<serde_json::Value> = serde_json::from_str(body).map_err(|e| e.to_string())?;
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
                 repository {{ nameWithOwner isArchived }} contributions(first: 1) {{ totalCount }}
               }}
               pullRequestReviewContributionsByRepository(maxRepositories: 50) {{
                 repository {{ nameWithOwner isArchived }} contributions(first: 1) {{ totalCount }}
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
        .output()
        .map_err(|e| format!("Could not run gh: {e}"))?;
    if !out.status.success() {
        return Err(format!("GitHub can't find {owner}/{repo}."));
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
      additions
      deletions
      author { login }
      authorAssociation
      bodyText
      body
      labels(first: 6) { nodes { name } }
      files(first: 40) { nodes { path } }
      state
      mergeable
      mergeStateStatus
      autoMergeRequest { enabledAt }
      viewerCanEnableAutoMerge
      repository { nameWithOwner isArchived isPrivate viewerDefaultMergeMethod defaultBranchRef { name } }
      reviewDecision
      reviewRequests(first: 6) {
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
        }
      }
      reviews(last: 6) {
        nodes {
          author { login }
          state
          submittedAt
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }
    }
    "#;

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
    busy.sort_by(|a, b| b.activity.cmp(&a.activity));
    searches.extend(busy.into_iter().take(BUSY_REPO_LIMIT).map(|repo| Search {
        query: format!(
            "is:pr is:open -is:draft -author:@me repo:{} updated:>={fresh_since}",
            repo.name
        ),
        reason: IncomingReason::SubscribedRule,
        label: Some("Active repo".into()),
        tracked_only: false,
    }));
    searches.extend(rules.iter().filter(|rule| rule.enabled).map(|rule| Search {
        query: rule.to_github_search_query(),
        reason: IncomingReason::SubscribedRule,
        label: Some(rule.name.clone()),
        tracked_only: false,
    }));

    // Each search runs on its own thread; GitHub's cost grows with the PRs returned, so they
    // overlap instead of queueing.
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = searches
            .iter()
            .map(|search| scope.spawn(move || fetch_search(search, options)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("A GitHub search stopped unexpectedly".to_string()))
            })
            .collect()
    });

    let mut viewer = String::new();
    let mut prs = Vec::<PullRequest>::new();
    for (search, result) in searches.iter().zip(results) {
        let (login, nodes) = result?;
        viewer = login;
        for node in &nodes {
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

const PAGE_SIZE: u32 = 30;

/// Every page of one search, with the signed-in account's login.
fn fetch_search(
    search: &Search,
    options: SyncOptions,
) -> Result<(String, Vec<serde_json::Value>), String> {
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

    let mut nodes = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let after = serde_json::to_string(&cursor).expect("serialize cursor");
        let query = format!(
            "query {{ viewer {{ login }} s0: search(query: {term}, type: ISSUE, first: {PAGE_SIZE}, after: {after}) {{ nodes {{ ...PrFields }} pageInfo {{ hasNextPage endCursor }} }} }}{PR_FIELDS}"
        );
        let data = fetch_graphql(&query)?;
        let viewer = data["viewer"]["login"]
            .as_str()
            .ok_or_else(|| "GitHub did not return the signed-in account".to_string())?
            .to_owned();
        let page = &data["s0"];
        nodes.extend(
            page["nodes"]
                .as_array()
                .ok_or_else(|| "GitHub did not return search results".to_string())?
                .iter()
                .cloned(),
        );
        if page["pageInfo"]["hasNextPage"].as_bool() != Some(true) {
            return Ok((viewer, nodes));
        }
        let next = page["pageInfo"]["endCursor"]
            .as_str()
            .filter(|next| Some(*next) != cursor.as_deref())
            .ok_or_else(|| "GitHub returned an invalid search cursor".to_string())?;
        cursor = Some(next.to_owned());
    }
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
        .output()
        .map_err(|e| format!("Could not run gh: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr
            .lines()
            .next()
            .unwrap_or("GitHub request failed")
            .to_owned());
    }

    let mut response: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("Invalid GraphQL JSON: {e}"))?;
    if let Some(message) = response["errors"][0]["message"].as_str() {
        return Err(message.to_owned());
    }
    let data = response["data"].take();
    if data.is_null() {
        return Err("GitHub response is missing data".to_string());
    }
    Ok(data)
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
            {
                if !reviewers.iter().any(|r| r == user) {
                    reviewers.push(user.to_string());
                }
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
    let mut verdicts: Vec<(String, bool)> = Vec::new();
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
            verdicts.retain(|(who, _)| who != login);
            verdicts.push((login.to_owned(), state == "APPROVED"));
        }
    }
    let approved_by: Vec<String> = verdicts
        .into_iter()
        .filter_map(|(who, approved)| approved.then_some(who))
        .collect();

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
    })
}

/// Merges the PR with `gh`, the way the repository prefers.
/// Turn auto-merge on (merging with `method`) or off. GitHub merges by itself once the checks and
/// reviews the repository asks for are in.
pub fn set_auto_merge(url: &str, method: MergeMethod, enable: bool) -> Result<(), String> {
    let mut command = gh_command();
    command.args(["pr", "merge", url]);
    if enable {
        command.args(["--auto", method.flag()]);
    } else {
        command.arg("--disable-auto");
    }
    let out = command
        .output()
        .map_err(|e| format!("Could not run gh: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("GitHub would not change auto-merge on this pull request")
        .trim()
        .to_owned())
}

/// OPEN, MERGED or CLOSED, for a pull request that has left the lists.
pub fn pr_state(url: &str) -> Result<String, String> {
    let out = gh_command()
        .args(["pr", "view", url, "--json", "state", "-q", ".state"])
        .output()
        .map_err(|e| format!("Could not run gh: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Approves the PR with `gh`, with no comment.
pub fn approve_pr(url: &str) -> Result<(), String> {
    let out = gh_command()
        .args(["pr", "review", url, "--approve"])
        .output()
        .map_err(|e| format!("Could not run gh: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("GitHub would not take this approval")
        .trim()
        .to_owned())
}

pub fn merge_pr(url: &str, method: MergeMethod) -> Result<(), String> {
    let out = gh_command()
        .args(["pr", "merge", url, method.flag()])
        .output()
        .map_err(|e| format!("Could not run gh: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("GitHub would not merge this pull request")
        .trim()
        .to_owned())
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

pub fn demo_prs() -> Vec<PullRequest> {
    let mut prs = vec![
        // --- acme/engine (High priority repo: assigned PRs + stale draft + unreviewed open PR) ---
        PullRequest {
            id: "acme/engine#418".to_string(),
            repo: "acme/engine".to_string(),
            number: 418,
            title: "metal-backend: coalesce offscreen Rgba16Float layer binds during pinch"
                .to_string(),
            url: "https://github.com/acme/engine/pull/418".to_string(),
            author: "kai".to_string(),
            ownership: Ownership::Theirs,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::AssignedOrReviewRequested,
            reviewers: vec!["mira".to_string()],
            idle_hours: 1,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 1,
            head_ref: "metal-layer-coalesce".to_string(),
            head_repo: Some("acme/engine".into()),
            stack_base: None,
            matched_rule: Some("Assigned to you".to_string()),
            snoozed_until_hour: 0,
            additions: 142,
            deletions: 38,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        PullRequest {
            id: "acme/engine#412".to_string(),
            repo: "acme/engine".to_string(),
            number: 412,
            title: "taffy: preserve subpixel baseline cache across color-only commits".to_string(),
            url: "https://github.com/acme/engine/pull/412".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::None,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(), // No reviewer assigned -> NUDGE to send to colleague!
            idle_hours: 54,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 54, // > 48h (2 days) without review -> NUDGE!
            head_ref: "feat/baseline-cache".to_string(),
            head_repo: Some("acme/engine".into()),
            stack_base: Some("metal-layer-coalesce".into()),
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 89,
            deletions: 14,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        PullRequest {
            id: "acme/engine#397".to_string(),
            repo: "acme/engine".to_string(),
            number: 397,
            title: "WIP: experimental CoreAudio low-latency spatial voice ring".to_string(),
            url: "https://github.com/acme/engine/pull/397".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: true,
            review_state: ReviewState::None,
            ci_status: CiStatus::Pending,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(),
            idle_hours: 120, // 5 days idle draft -> NUDGE!
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 120,
            head_ref: "draft/coreaudio-ring".to_string(),
            head_repo: Some("acme/engine".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 412,
            deletions: 19,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        PullRequest {
            id: "acme/engine#421".to_string(),
            repo: "acme/engine".to_string(),
            number: 421,
            title: "windfoil: tighten CJK autohint stem dilation at 1x scale".to_string(),
            url: "https://github.com/acme/engine/pull/421".to_string(),
            author: "sam".to_string(),
            ownership: Ownership::Theirs,
            is_draft: false,
            review_state: ReviewState::Commented,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::SubscribedRule,
            reviewers: vec!["kai".to_string()],
            idle_hours: 5,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 5,
            head_ref: "windfoil-cjk-1x".to_string(),
            head_repo: Some("acme/engine".into()),
            stack_base: Some("feat/baseline-cache".into()),
            matched_rule: Some("Core Runtime Non-Draft PRs".to_string()),
            snoozed_until_hour: 0,
            additions: 64,
            deletions: 29,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        // --- acme/cli (Second repo: assigned review + your open PR waiting > 3d on reviewer) ---
        PullRequest {
            id: "acme/cli#904".to_string(),
            repo: "acme/cli".to_string(),
            number: 904,
            title: "acp: stream tool call diffs over zero-copy IPC channel".to_string(),
            url: "https://github.com/acme/cli/pull/904".to_string(),
            author: "juno".to_string(),
            ownership: Ownership::Theirs,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::AssignedOrReviewRequested,
            reviewers: vec!["mira".to_string()],
            idle_hours: 3,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 3,
            head_ref: "acp-zero-copy-diffs".to_string(),
            head_repo: Some("acme/cli".into()),
            stack_base: None,
            matched_rule: Some("Assigned to you".to_string()),
            snoozed_until_hour: 0,
            additions: 275,
            deletions: 84,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: true,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        PullRequest {
            id: "acme/cli#889".to_string(),
            repo: "acme/cli".to_string(),
            number: 889,
            title: "session: retry transient rate-limit 529 with jittered backoff".to_string(),
            url: "https://github.com/acme/cli/pull/889".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: vec!["juno".to_string()],
            idle_hours: 74,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 74, // Waiting on @rekram for 3 days -> NUDGE to ping colleague!
            head_ref: "fix/retry-529-backoff".to_string(),
            head_repo: Some("acme/cli".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 53,
            deletions: 12,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        PullRequest {
            id: "acme/cli#908".to_string(),
            repo: "acme/cli".to_string(),
            number: 908,
            title: "cli: add `--json-events` flag for headless subagent pipelines".to_string(),
            url: "https://github.com/acme/cli/pull/908".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::Approved,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: vec!["kai".to_string()],
            idle_hours: 2,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 0,
            head_ref: "feat/json-events".to_string(),
            head_repo: Some("acme/cli".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 118,
            deletions: 7,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        // --- acme/inbox (Third repo: active draft + changes requested) ---
        PullRequest {
            id: "acme/inbox#14".to_string(),
            repo: "acme/inbox".to_string(),
            number: 14,
            title: "sqlite: persist per-repo smart importance weights & snooze deadlines"
                .to_string(),
            url: "https://github.com/acme/inbox/pull/14".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::ChangesRequested,
            ci_status: CiStatus::Failing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: vec!["alex".to_string()],
            idle_hours: 6,
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 6,
            head_ref: "sqlite-importance-weights".to_string(),
            head_repo: Some("acme/inbox".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 310,
            deletions: 44,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
        PullRequest {
            id: "acme/inbox#19".to_string(),
            repo: "acme/inbox".to_string(),
            number: 19,
            title: "draft: webhook websocket bridge for sub-second PR assignment pushes"
                .to_string(),
            url: "https://github.com/acme/inbox/pull/19".to_string(),
            author: "mira".to_string(),
            ownership: Ownership::Mine,
            is_draft: true,
            review_state: ReviewState::None,
            ci_status: CiStatus::Pending,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(),
            idle_hours: 8, // Fresh draft (< 72h), not stale
            approved_by: Vec::new(),
            opened_hours: 0,
            unreviewed_hours: 8,
            head_ref: "draft/ws-webhook-bridge".to_string(),
            head_repo: Some("acme/inbox".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 195,
            deletions: 0,
            merge_readiness: MergeReadiness::Unknown,
            merge_method: MergeMethod::Squash,
            auto_merge: AutoMerge::Unavailable,
            external: false,
            body: String::new(),
            description: String::new(),
            labels: Vec::new(),
            private_repo: false,
            files: Vec::new(),
        },
    ];
    for pr in &mut prs {
        if pr.number == 908 {
            pr.approved_by = vec!["kai".to_owned()];
        }
    }
    // Opened a while before it last moved, as real ones are.
    for pr in &mut prs {
        pr.opened_hours = pr.idle_hours * 4 + 20;
    }
    let base = prs[0].clone();
    let review = |repo: &str,
                  number: u64,
                  title: &str,
                  author: &str,
                  external: bool,
                  team: bool,
                  size: (u32, u32),
                  idle: u64| {
        let mut pr = base.clone();
        pr.id = format!("{repo}#{number}");
        pr.repo = repo.into();
        pr.number = number;
        pr.title = title.into();
        pr.url = format!("https://github.com/{repo}/pull/{number}");
        pr.author = author.into();
        pr.external = external;
        pr.incoming_reason = if team {
            IncomingReason::TeamReviewRequested
        } else {
            IncomingReason::AssignedOrReviewRequested
        };
        pr.matched_rule = None;
        pr.reviewers = if team {
            Vec::new()
        } else {
            vec!["mira".into()]
        };
        pr.head_ref = format!("demo-{number}");
        pr.head_repo = Some(repo.into());
        pr.stack_base = None;
        (pr.additions, pr.deletions) = size;
        pr.idle_hours = idle;
        pr.opened_hours = idle * 4 + 20;
        pr.unreviewed_hours = idle;
        pr
    };
    prs.push(review(
        "acme/cli",
        910,
        "docs: fix typos in the contributing guide",
        "juno",
        true,
        false,
        (4, 4),
        7,
    ));
    prs.push(review(
        "acme/cli",
        912,
        "parser: migrate the command parser to the new tokenizer",
        "kai",
        false,
        false,
        (1240, 860),
        9,
    ));
    prs.push(review(
        "acme/engine",
        920,
        "webhooks: retry deliveries with exponential backoff",
        "sam",
        false,
        false,
        (96, 31),
        11,
    ));
    prs.push(review(
        "acme/engine",
        925,
        "metrics: export the queue depth gauge",
        "kai",
        false,
        true,
        (58, 6),
        13,
    ));
    prs.push(review(
        "acme/cli",
        931,
        "i18n: update the Polish translations",
        "juno",
        true,
        true,
        (310, 290),
        15,
    ));
    prs
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
