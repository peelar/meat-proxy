"""The GitHub that parity drives see: one signed-in account and `github::demo_prs()`'s nine pull requests.

It answers what meat-proxy (Rune) and its Exact port ask `gh`: `auth token`,
`auth status`, `api graphql` searches, and `api /notifications -i`. The pull
requests are `github::demo_prs()`'s nine, as GraphQL nodes, timed relative to
now so both apps compute the same ages. MEAT_PROXY_FAKE_GH=signed-out makes
every call fail as a signed-out gh does.
"""
import json
import os
import re
import sys
from datetime import datetime, timedelta, timezone

NOW = datetime.now(timezone.utc)


def ago(hours):
    return (NOW - timedelta(hours=hours)).strftime("%Y-%m-%dT%H:%M:%SZ")


# number, repo, title, author, draft, idle h, unreviewed h, decision, ci, requested,
# reviews [(login, state, hours ago)], head, base, +, -, which search finds it
PRS = [
    (418, "anomalyco/rune", "metal-backend: coalesce offscreen Rgba16Float layer binds during pinch", "thdxr",
     False, 1, 1, "REVIEW_REQUIRED", "SUCCESS", ["peelar"], [], "metal-layer-coalesce", "main", 142, 38, "review"),
    (412, "anomalyco/rune", "taffy: preserve subpixel baseline cache across color-only commits", "peelar",
     False, 54, 54, None, "SUCCESS", [], [], "feat/baseline-cache", "metal-layer-coalesce", 89, 14, "mine"),
    (397, "anomalyco/rune", "WIP: experimental CoreAudio low-latency spatial voice ring", "peelar",
     True, 120, 120, None, "PENDING", [], [], "draft/coreaudio-ring", "main", 412, 19, "mine"),
    (421, "anomalyco/rune", "windfoil: tighten CJK autohint stem dilation at 1x scale", "adamw",
     False, 5, 5, None, "SUCCESS", [], [("thdxr", "COMMENTED", 5)], "windfoil-cjk-1x", "feat/baseline-cache", 64, 29, "watch"),
    (904, "anomalyco/opencode", "acp: stream tool call diffs over zero-copy IPC channel", "rekram",
     False, 3, 3, "REVIEW_REQUIRED", "SUCCESS", ["peelar"], [], "acp-zero-copy-diffs", "dev", 275, 84, "review"),
    (889, "anomalyco/opencode", "session: retry transient rate-limit 529 with jittered backoff", "peelar",
     False, 74, 74, "REVIEW_REQUIRED", "SUCCESS", ["rekram"], [], "fix/retry-529-backoff", "dev", 53, 12, "mine"),
    (908, "anomalyco/opencode", "cli: add `--json-events` flag for headless subagent pipelines", "peelar",
     False, 2, 0, "APPROVED", "SUCCESS", [], [("thdxr", "APPROVED", 0)], "feat/json-events", "dev", 118, 7, "mine"),
    (14, "peelar/meat-proxy", "sqlite: persist per-repo smart importance weights & snooze deadlines", "peelar",
     False, 6, 6, "CHANGES_REQUESTED", "FAILURE", [], [("alex", "CHANGES_REQUESTED", 6)], "sqlite-importance-weights", "main", 310, 44, "mine"),
    (19, "peelar/meat-proxy", "draft: webhook websocket bridge for sub-second PR assignment pushes", "peelar",
     True, 8, 8, None, "PENDING", [], [], "draft/ws-webhook-bridge", "main", 195, 0, "mine"),
]
DEFAULT_BRANCH = {"anomalyco/rune": "main", "anomalyco/opencode": "dev", "peelar/meat-proxy": "main"}
VIEWER = "peelar"


def node(pr):
    (number, repo, title, author, draft, idle, unreviewed, decision, ci, requested, reviews,
     head, base, adds, dels, _) = pr
    return {
        "id": f"PR_{number}", "number": number, "title": title,
        "url": f"https://github.com/{repo}/pull/{number}", "isDraft": draft,
        "createdAt": ago(unreviewed), "updatedAt": ago(idle),
        "baseRefName": base, "headRepository": {"nameWithOwner": repo}, "headRefName": head,
        "additions": adds, "deletions": dels, "author": {"login": author},
        "repository": {"nameWithOwner": repo, "defaultBranchRef": {"name": DEFAULT_BRANCH[repo]}},
        "reviewDecision": decision,
        "reviewRequests": {"nodes": [{"requestedReviewer": {"login": r}} for r in requested]},
        "reviews": {"nodes": [{"author": {"login": a}, "state": s, "submittedAt": ago(h)} for a, s, h in reviews]},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": ci}}}]},
    }


def search(term):
    if re.search(r"(^|\s)author:@me", term):
        kinds = {"mine"}
    elif "review-requested:@me" in term or "assignee:@me" in term:
        kinds = {"review"} if "review-requested" in term else set()
    else:
        # A watched repository's or a rule's search: the others' PRs it names.
        repo = re.search(r"repo:(\S+)", term)
        return [p for p in PRS if p[15] == "watch" and (not repo or p[1] == repo.group(1))]
    return [p for p in PRS if p[15] in kinds]


def graphql(query):
    data = {"viewer": {"login": VIEWER}}
    for alias, term in re.findall(r'(s\d+): search\(query: ("(?:[^"\\]|\\.)*")', query):
        data[alias] = {"nodes": [node(p) for p in search(json.loads(term))],
                       "pageInfo": {"hasNextPage": False, "endCursor": None}}
    return {"data": data}
