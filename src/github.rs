use std::process::Command;

use crate::model::{
    CiStatus, IncomingReason, NotificationEvent, NotificationKind, Ownership, PullRequest,
    RepoConfig, ReviewState, WatchRule,
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
                format!("Review requested on {repo}#{number}")
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
      repository { nameWithOwner defaultBranchRef { name } }
      reviewDecision
      reviewRequests(first: 6) {
        nodes {
          requestedReviewer {
            ... on User { login }
            ... on Team { name }
          }
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
}

pub fn fetch_live(repos: &[RepoConfig], rules: &[WatchRule]) -> Result<SyncOutput, String> {
    let incoming = IncomingReason::AssignedOrReviewRequested;
    let mut searches = vec![
        Search {
            query: "is:pr is:open author:@me".into(),
            reason: IncomingReason::NotIncoming,
            label: None,
        },
        Search {
            query: "is:pr is:open -author:@me -is:draft review-requested:@me".into(),
            reason: incoming,
            label: Some("Review Requested".into()),
        },
        Search {
            query: "is:pr is:open -author:@me -is:draft assignee:@me".into(),
            reason: incoming,
            label: Some("Assigned".into()),
        },
    ];
    searches.extend(
        repos
            .iter()
            .filter(|repo| repo.watch_all_open_prs)
            .map(|repo| Search {
                query: format!("is:pr is:open -is:draft -author:@me repo:{}", repo.name),
                reason: IncomingReason::SubscribedRule,
                label: Some("Watched Repo".into()),
            }),
    );
    searches.extend(rules.iter().filter(|rule| rule.enabled).map(|rule| Search {
        query: rule.to_github_search_query(),
        reason: IncomingReason::SubscribedRule,
        label: Some(rule.name.clone()),
    }));

    let mut viewer = String::new();
    let mut prs = Vec::<PullRequest>::new();
    // Bound each request so several large search results and their review/check data can coexist.
    for batch in searches.chunks(3) {
        let mut pending: Vec<_> = batch
            .iter()
            .map(|search| (search, None::<String>))
            .collect();
        while !pending.is_empty() {
            let mut query = String::from("query { viewer { login }");
            for (i, (search, cursor)) in pending.iter().enumerate() {
                let term = serde_json::to_string(&search.query).expect("serialize search");
                let cursor = serde_json::to_string(cursor).expect("serialize cursor");
                query.push_str(&format!(
                    " s{i}: search(query: {term}, type: ISSUE, first: 12, after: {cursor})                      {{ nodes {{ ...PrFields }} pageInfo {{ hasNextPage endCursor }} }}"
                ));
            }
            query.push('}');
            query.push_str(PR_FIELDS);

            let data = fetch_graphql(&query)?;
            viewer = data["viewer"]["login"]
                .as_str()
                .ok_or_else(|| "GitHub did not return the signed-in account".to_string())?
                .to_owned();

            let mut next = Vec::new();
            for (i, (search, previous_cursor)) in pending.into_iter().enumerate() {
                let page = &data[format!("s{i}")];
                let nodes = page["nodes"]
                    .as_array()
                    .ok_or_else(|| "GitHub did not return search results".to_string())?;
                for node in nodes {
                    let mine = node["author"]["login"]
                        .as_str()
                        .is_some_and(|login| login.eq_ignore_ascii_case(&viewer));
                    let ownership = if mine {
                        Ownership::Mine
                    } else {
                        Ownership::Theirs
                    };
                    let reason = if mine {
                        IncomingReason::NotIncoming
                    } else {
                        search.reason
                    };
                    let Some(pr) =
                        parse_graphql_pr(node, ownership, reason, search.label.as_deref())
                    else {
                        continue;
                    };
                    if let Some(existing) = prs.iter_mut().find(|p| p.id == pr.id) {
                        if reason == incoming {
                            existing.incoming_reason = incoming;
                        }
                        if existing.matched_rule.is_none() {
                            existing.matched_rule = pr.matched_rule;
                        }
                    } else {
                        prs.push(pr);
                    }
                }
                if page["pageInfo"]["hasNextPage"].as_bool() == Some(true) {
                    let cursor = page["pageInfo"]["endCursor"]
                        .as_str()
                        .filter(|cursor| Some(*cursor) != previous_cursor.as_deref())
                        .ok_or_else(|| "GitHub returned an invalid search cursor".to_string())?;
                    next.push((search, Some(cursor.to_owned())));
                }
            }
            pending = next;
        }
    }

    Ok(SyncOutput {
        status: SyncStatus::Connected { username: viewer },
        prs,
    })
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
) -> Option<PullRequest> {
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
        idle_hours,
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
    })
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
                    watch_all_open_prs: false,
                    my_pr_volume: 0,
                    assigned_review_volume: 0,
                    collapsed: false,
                });
                rank
            });
        out[idx].my_pr_volume += u32::from(pr.ownership == Ownership::Mine);
        out[idx].assigned_review_volume +=
            u32::from(pr.incoming_reason == IncomingReason::AssignedOrReviewRequested);
    }
    out
}

pub fn demo_prs() -> Vec<PullRequest> {
    vec![
        // --- anomalyco/rune (High priority repo: assigned PRs + stale draft + unreviewed open PR) ---
        PullRequest {
            id: "anomalyco/rune#418".to_string(),
            repo: "anomalyco/rune".to_string(),
            number: 418,
            title: "metal-backend: coalesce offscreen Rgba16Float layer binds during pinch"
                .to_string(),
            url: "https://github.com/anomalyco/rune/pull/418".to_string(),
            author: "thdxr".to_string(),
            ownership: Ownership::Theirs,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::AssignedOrReviewRequested,
            reviewers: vec!["peelar".to_string()],
            idle_hours: 1,
            unreviewed_hours: 1,
            head_ref: "metal-layer-coalesce".to_string(),
            head_repo: Some("anomalyco/rune".into()),
            stack_base: None,
            matched_rule: Some("Assigned to you".to_string()),
            snoozed_until_hour: 0,
            additions: 142,
            deletions: 38,
        },
        PullRequest {
            id: "anomalyco/rune#412".to_string(),
            repo: "anomalyco/rune".to_string(),
            number: 412,
            title: "taffy: preserve subpixel baseline cache across color-only commits".to_string(),
            url: "https://github.com/anomalyco/rune/pull/412".to_string(),
            author: "peelar".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::None,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(), // No reviewer assigned -> NUDGE to send to colleague!
            idle_hours: 54,
            unreviewed_hours: 54, // > 48h (2 days) without review -> NUDGE!
            head_ref: "feat/baseline-cache".to_string(),
            head_repo: Some("anomalyco/rune".into()),
            stack_base: Some("metal-layer-coalesce".into()),
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 89,
            deletions: 14,
        },
        PullRequest {
            id: "anomalyco/rune#397".to_string(),
            repo: "anomalyco/rune".to_string(),
            number: 397,
            title: "WIP: experimental CoreAudio low-latency spatial voice ring".to_string(),
            url: "https://github.com/anomalyco/rune/pull/397".to_string(),
            author: "peelar".to_string(),
            ownership: Ownership::Mine,
            is_draft: true,
            review_state: ReviewState::None,
            ci_status: CiStatus::Pending,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(),
            idle_hours: 120, // 5 days idle draft -> NUDGE!
            unreviewed_hours: 120,
            head_ref: "draft/coreaudio-ring".to_string(),
            head_repo: Some("anomalyco/rune".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 412,
            deletions: 19,
        },
        PullRequest {
            id: "anomalyco/rune#421".to_string(),
            repo: "anomalyco/rune".to_string(),
            number: 421,
            title: "windfoil: tighten CJK autohint stem dilation at 1x scale".to_string(),
            url: "https://github.com/anomalyco/rune/pull/421".to_string(),
            author: "adamw".to_string(),
            ownership: Ownership::Theirs,
            is_draft: false,
            review_state: ReviewState::Commented,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::SubscribedRule,
            reviewers: vec!["thdxr".to_string()],
            idle_hours: 5,
            unreviewed_hours: 5,
            head_ref: "windfoil-cjk-1x".to_string(),
            head_repo: Some("anomalyco/rune".into()),
            stack_base: Some("feat/baseline-cache".into()),
            matched_rule: Some("Core Runtime Non-Draft PRs".to_string()),
            snoozed_until_hour: 0,
            additions: 64,
            deletions: 29,
        },
        // --- anomalyco/opencode (Second repo: assigned review + your open PR waiting > 3d on reviewer) ---
        PullRequest {
            id: "anomalyco/opencode#904".to_string(),
            repo: "anomalyco/opencode".to_string(),
            number: 904,
            title: "acp: stream tool call diffs over zero-copy IPC channel".to_string(),
            url: "https://github.com/anomalyco/opencode/pull/904".to_string(),
            author: "rekram".to_string(),
            ownership: Ownership::Theirs,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::AssignedOrReviewRequested,
            reviewers: vec!["peelar".to_string()],
            idle_hours: 3,
            unreviewed_hours: 3,
            head_ref: "acp-zero-copy-diffs".to_string(),
            head_repo: Some("anomalyco/opencode".into()),
            stack_base: None,
            matched_rule: Some("Assigned to you".to_string()),
            snoozed_until_hour: 0,
            additions: 275,
            deletions: 84,
        },
        PullRequest {
            id: "anomalyco/opencode#889".to_string(),
            repo: "anomalyco/opencode".to_string(),
            number: 889,
            title: "session: retry transient rate-limit 529 with jittered backoff".to_string(),
            url: "https://github.com/anomalyco/opencode/pull/889".to_string(),
            author: "peelar".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::ReviewRequested,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: vec!["rekram".to_string()],
            idle_hours: 74,
            unreviewed_hours: 74, // Waiting on @rekram for 3 days -> NUDGE to ping colleague!
            head_ref: "fix/retry-529-backoff".to_string(),
            head_repo: Some("anomalyco/opencode".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 53,
            deletions: 12,
        },
        PullRequest {
            id: "anomalyco/opencode#908".to_string(),
            repo: "anomalyco/opencode".to_string(),
            number: 908,
            title: "cli: add `--json-events` flag for headless subagent pipelines".to_string(),
            url: "https://github.com/anomalyco/opencode/pull/908".to_string(),
            author: "peelar".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::Approved,
            ci_status: CiStatus::Passing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: vec!["thdxr".to_string()],
            idle_hours: 2,
            unreviewed_hours: 0,
            head_ref: "feat/json-events".to_string(),
            head_repo: Some("anomalyco/opencode".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 118,
            deletions: 7,
        },
        // --- peelar/meat-proxy (Third repo: active draft + changes requested) ---
        PullRequest {
            id: "peelar/meat-proxy#14".to_string(),
            repo: "peelar/meat-proxy".to_string(),
            number: 14,
            title: "sqlite: persist per-repo smart importance weights & snooze deadlines"
                .to_string(),
            url: "https://github.com/peelar/meat-proxy/pull/14".to_string(),
            author: "peelar".to_string(),
            ownership: Ownership::Mine,
            is_draft: false,
            review_state: ReviewState::ChangesRequested,
            ci_status: CiStatus::Failing,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: vec!["alex".to_string()],
            idle_hours: 6,
            unreviewed_hours: 6,
            head_ref: "sqlite-importance-weights".to_string(),
            head_repo: Some("peelar/meat-proxy".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 310,
            deletions: 44,
        },
        PullRequest {
            id: "peelar/meat-proxy#19".to_string(),
            repo: "peelar/meat-proxy".to_string(),
            number: 19,
            title: "draft: webhook websocket bridge for sub-second PR assignment pushes"
                .to_string(),
            url: "https://github.com/peelar/meat-proxy/pull/19".to_string(),
            author: "peelar".to_string(),
            ownership: Ownership::Mine,
            is_draft: true,
            review_state: ReviewState::None,
            ci_status: CiStatus::Pending,
            incoming_reason: IncomingReason::NotIncoming,
            reviewers: Vec::new(),
            idle_hours: 8, // Fresh draft (< 72h), not stale
            unreviewed_hours: 8,
            head_ref: "draft/ws-webhook-bridge".to_string(),
            head_repo: Some("peelar/meat-proxy".into()),
            stack_base: None,
            matched_rule: None,
            snoozed_until_hour: 0,
            additions: 195,
            deletions: 0,
        },
    ]
}
