// meat-proxy's inbox rules, ported from the Rust app (src/model.rs, src/stacks.rs, src/notify.rs and the
// parsing in src/github.rs). Pure: no I/O, and time comes in as `nowMs`.

export type ReviewState = 'NONE' | 'REVIEW_REQUESTED' | 'CHANGES_REQUESTED' | 'APPROVED' | 'COMMENTED';
export type CiStatus = 'PASSING' | 'PENDING' | 'FAILING' | 'NONE';
export type Reason = 'MINE' | 'ASSIGNED' | 'WATCHED';
export type Filter = 'all' | 'action' | 'mine' | 'drafts' | 'theirs';
export type Attention =
  | 'IncomingAssigned' | 'MyOpenNeedsNudge' | 'MyOpenChangesRequested' | 'MyDraftNeedsNudge'
  | 'MyOpenApproved' | 'IncomingWatched' | 'MyOpenWaiting' | 'MyDraftActive';

export interface PullRequest {
  id: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  author: string;
  mine: boolean;
  draft: boolean;
  review: ReviewState;
  ci: CiStatus;
  reason: Reason;
  reviewers: string[];
  // ISO times; ages are computed against the caller's clock.
  updatedAt: string;
  reviewClock: string;
  headRef: string;
  headRepo: string | null;
  stackBase: string | null;
  rule: string | null;
}

export interface RepoConfig {
  name: string;
  rank: number;
  pinned: boolean;
  watch: boolean;
  myVolume: number;
  assignedVolume: number;
}

export interface Rule {
  id: string;
  name: string;
  query: string;
  excludeDrafts: boolean;
  excludeMine: boolean;
  enabled: boolean;
}

export interface Event {
  id: string;
  title: string;
  body: string;
  url: string;
}

export interface Thresholds {
  draft: number;
  open: number;
  nowMs: number;
  snoozes: Record<string, number>;
}

const HOUR = 3_600_000;
export const hoursSince = (iso: string, nowMs: number): number => {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? 0 : Math.max(0, Math.floor((nowMs - t) / HOUR));
};
export const idleHours = (pr: PullRequest, nowMs: number) => hoursSince(pr.updatedAt, nowMs);
export const unreviewedHours = (pr: PullRequest, nowMs: number) => hoursSince(pr.reviewClock, nowMs);
const snoozed = (pr: PullRequest, t: Thresholds) => (t.snoozes[pr.id] ?? 0) > Math.floor(t.nowMs / HOUR);

export function attention(pr: PullRequest, t: Thresholds): Attention {
  if (!pr.mine) return pr.reason === 'ASSIGNED' ? 'IncomingAssigned' : 'IncomingWatched';
  const quiet = snoozed(pr, t);
  if (pr.draft) return !quiet && idleHours(pr, t.nowMs) >= t.draft ? 'MyDraftNeedsNudge' : 'MyDraftActive';
  if (pr.review === 'CHANGES_REQUESTED') return 'MyOpenChangesRequested';
  if (pr.review === 'APPROVED') return 'MyOpenApproved';
  const stale = pr.reviewers.length === 0 || unreviewedHours(pr, t.nowMs) >= t.open;
  return !quiet && stale ? 'MyOpenNeedsNudge' : 'MyOpenWaiting';
}

const RANK: Attention[] = [
  'IncomingAssigned', 'MyOpenNeedsNudge', 'MyOpenChangesRequested', 'MyDraftNeedsNudge',
  'MyOpenApproved', 'IncomingWatched', 'MyOpenWaiting', 'MyDraftActive',
];
const SCORE: Record<Attention, number> = {
  IncomingAssigned: 45, MyOpenNeedsNudge: 38, MyOpenChangesRequested: 32, MyDraftNeedsNudge: 26,
  MyOpenApproved: 20, IncomingWatched: 12, MyOpenWaiting: 10, MyDraftActive: 6,
};
export const requiresAction = (a: Attention) => RANK.indexOf(a) <= RANK.indexOf('MyOpenApproved');
export const isNudge = (a: Attention) => a === 'MyOpenNeedsNudge' || a === 'MyDraftNeedsNudge';

export function matchesFilter(f: Filter, pr: PullRequest, t: Thresholds): boolean {
  switch (f) {
    case 'action': return requiresAction(attention(pr, t));
    case 'mine': return pr.mine;
    case 'drafts': return pr.mine && pr.draft;
    case 'theirs': return !pr.mine;
    default: return true;
  }
}

export function ageLabel(hours: number): string {
  return hours === 0 ? 'just now' : hours < 24 ? `${hours}h ago` : `${Math.floor(hours / 24)}d ago`;
}

export function statusOf(pr: PullRequest, t: Thresholds): { label: string; tone: string } {
  switch (attention(pr, t)) {
    case 'IncomingAssigned': return { label: 'Review requested', tone: 'accent' };
    case 'MyOpenNeedsNudge':
      return pr.reviewers.length === 0
        ? { label: 'Needs reviewer', tone: 'warning' }
        : { label: `Unreviewed ${Math.max(1, Math.floor(unreviewedHours(pr, t.nowMs) / 24))}d`, tone: 'warning' };
    case 'MyDraftNeedsNudge': return { label: 'Stale draft', tone: 'warning' };
    case 'MyOpenChangesRequested': return { label: 'Changes requested', tone: 'danger' };
    case 'MyOpenApproved': return { label: 'Approved', tone: 'accent' };
    case 'MyOpenWaiting': return { label: 'Waiting for review', tone: 'muted' };
    case 'MyDraftActive': return { label: 'Draft', tone: 'muted' };
    default: return { label: 'Watching', tone: 'muted' };
  }
}

export function smartScore(repo: RepoConfig, prs: PullRequest[], t: Thresholds): number {
  let score = repo.assignedVolume * 8 + repo.myVolume * 5 + (repo.pinned ? 500 : 0);
  for (const pr of prs) if (pr.repo === repo.name) score += SCORE[attention(pr, t)];
  return score;
}

export function orderedRepos(repos: RepoConfig[], prs: PullRequest[], smart: boolean, t: Thresholds): RepoConfig[] {
  const list = [...repos];
  if (smart) list.sort((a, b) => smartScore(b, prs, t) - smartScore(a, prs, t) || a.rank - b.rank);
  else list.sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.rank - b.rank);
  return list;
}

export function prsForRepo(name: string, prs: PullRequest[], filter: Filter, query: string, t: Thresholds): PullRequest[] {
  const q = query.trim().toLowerCase();
  return prs
    .filter((pr) => pr.repo === name && matchesFilter(filter, pr, t))
    .filter((pr) => !q || pr.title.toLowerCase().includes(q) || pr.author.toLowerCase().includes(q)
      || pr.repo.toLowerCase().includes(q) || String(pr.number).includes(q))
    .sort((a, b) => RANK.indexOf(attention(a, t)) - RANK.indexOf(attention(b, t))
      || idleHours(b, t.nowMs) - idleHours(a, t.nowMs) || b.number - a.number);
}

export function inferRepos(prs: PullRequest[], existing: RepoConfig[]): RepoConfig[] {
  const out = existing.map((r) => ({ ...r, myVolume: 0, assignedVolume: 0 }));
  for (const pr of prs) {
    let repo = out.find((r) => r.name === pr.repo);
    if (!repo) {
      repo = { name: pr.repo, rank: out.length, pinned: false, watch: false, myVolume: 0, assignedVolume: 0 };
      out.push(repo);
    }
    repo.myVolume += Number(pr.mine);
    repo.assignedVolume += Number(pr.reason === 'ASSIGNED');
  }
  return out;
}

/** Moves the repository at `from` to `to` in the ordered list and fixes the order (Rust's `move_repo`). */
export function moveRepo(ordered: RepoConfig[], from: number, to: number): RepoConfig[] {
  if (from >= ordered.length || to >= ordered.length || from === to) return ordered;
  const list = [...ordered];
  const [item] = list.splice(from, 1);
  list.splice(to, 0, item);
  return list.map((r, i) => ({ ...r, rank: i }));
}

export interface StackRow { pr: PullRequest; depth: number; parent: number | null }

/** One repository's visible PRs, in inbox order, nested under the PRs whose branch they target. */
export function stackRows(visible: PullRequest[], inbox: PullRequest[]): StackRow[] {
  if (visible.length === 0) return [];
  const repo = visible[0].repo;
  const heads = new Map<string, number | null>();
  for (const pr of inbox) {
    if (pr.repo !== repo || !pr.headRef || pr.headRepo?.toLowerCase() !== repo.toLowerCase()) continue;
    // A branch with two open PRs has no unique parent, even if one is filtered out.
    heads.set(pr.headRef, heads.has(pr.headRef) ? null : pr.number);
  }
  const position = new Map(visible.map((pr, i) => [pr.number, i]));
  const parents = visible.map((pr) => {
    const n = pr.stackBase ? heads.get(pr.stackBase) : undefined;
    return n == null || n === pr.number ? null : position.get(n) ?? null;
  });
  const children: number[][] = visible.map(() => []);
  parents.forEach((p, i) => { if (p != null) children[p].push(i); });
  const seen = new Set<number>();
  const out: StackRow[] = [];
  for (let i = 0; i < visible.length; i++) {
    if (seen.has(i)) continue;
    let root = i;
    const ancestors = new Set<number>();
    while (parents[root] != null) {
      if (ancestors.has(root)) break;
      ancestors.add(root);
      root = parents[root]!;
    }
    if (parents[root] != null) {
      // Mutually targeted branches are not a stack.
      seen.add(i);
      out.push({ pr: visible[i], depth: 0, parent: null });
      continue;
    }
    const pending: [number, number][] = [[root, 0]];
    while (pending.length) {
      const [index, depth] = pending.pop()!;
      if (seen.has(index)) continue;
      seen.add(index);
      const p = parents[index];
      out.push({ pr: visible[index], depth, parent: p == null ? null : visible[p].number });
      for (const c of [...children[index]].reverse()) pending.push([c, depth + 1]);
    }
  }
  return out;
}

export function ruleQuery(rule: Rule): string {
  return ['is:pr', 'is:open', ...(rule.excludeDrafts ? ['-is:draft'] : []), ...(rule.excludeMine ? ['-author:@me'] : []),
    ...(rule.query.trim() ? [rule.query.trim()] : [])].join(' ');
}

/** Rust's `evaluate_inbox_notifications`: assignments, nudges and review changes not yet sent. */
export function notificationEvents(previous: PullRequest[], current: PullRequest[], sent: Set<string>, t: Thresholds): Event[] {
  const events: Event[] = [];
  const add = (e: Event) => { if (!sent.has(e.id)) events.push(e); };
  for (const pr of current) {
    const prev = previous.find((p) => p.id === pr.id);
    const a = attention(pr, t);
    if (a === 'IncomingAssigned') {
      if (!prev || prev.reason !== 'ASSIGNED' || previous.length === 0)
        add({ id: `assign-${pr.id}`, title: `Assigned to review ${pr.repo}#${pr.number}`, body: `@${pr.author}: ${pr.title}`, url: pr.url });
    } else if (a === 'MyOpenNeedsNudge') {
      const why = pr.reviewers.length === 0
        ? 'No reviewer assigned yet — send to a colleague!'
        : `No review from @${pr.reviewers.join(', @')} in ${Math.max(1, Math.floor(unreviewedHours(pr, t.nowMs) / 24))}d — nudge your reviewer!`;
      add({ id: `open-nudge-${pr.id}`, title: `Open PR Nudge · ${pr.repo}#${pr.number}`, body: `${pr.title} (${why})`, url: pr.url });
    } else if (a === 'MyDraftNeedsNudge') {
      const d = Math.max(1, Math.floor(idleHours(pr, t.nowMs) / 24));
      add({ id: `draft-nudge-${pr.id}`, title: `Forgotten Draft (${d}d idle) · ${pr.repo}#${pr.number}`, body: `Resume or close: ${pr.title}`, url: pr.url });
    } else if (pr.mine && prev && prev.review !== pr.review) {
      add({ id: `review-state-${pr.id}-${pr.review}`, title: `PR ${pr.review} · ${pr.repo}#${pr.number}`, body: pr.title, url: pr.url });
    }
  }
  return events;
}

// --- GitHub's answers -----------------------------------------------------------------------------------

export const PR_FIELDS = `
fragment PrFields on PullRequest {
  id number title url isDraft createdAt updatedAt baseRefName
  headRepository { nameWithOwner } headRefName additions deletions
  author { login } repository { nameWithOwner defaultBranchRef { name } } reviewDecision
  reviewRequests(first: 6) { nodes { requestedReviewer { ... on User { login } ... on Team { name } } } }
  reviews(last: 6) { nodes { author { login } state submittedAt } }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}`;

export interface Search { query: string; reason: Reason; label: string | null }

export function searches(repos: RepoConfig[], rules: Rule[]): Search[] {
  return [
    { query: 'is:pr is:open author:@me', reason: 'MINE', label: null },
    { query: 'is:pr is:open -author:@me -is:draft review-requested:@me', reason: 'ASSIGNED', label: 'Review Requested' },
    { query: 'is:pr is:open -author:@me -is:draft assignee:@me', reason: 'ASSIGNED', label: 'Assigned' },
    ...repos.filter((r) => r.watch).map((r) => ({ query: `is:pr is:open -is:draft -author:@me repo:${r.name}`, reason: 'WATCHED' as Reason, label: 'Watched Repo' })),
    ...rules.filter((r) => r.enabled).map((r) => ({ query: ruleQuery(r), reason: 'WATCHED' as Reason, label: r.name })),
  ];
}

// deno-lint-ignore no-explicit-any
type Json = any;

export function parsePr(node: Json, viewer: string, reason: Reason, label: string | null): PullRequest | null {
  if (typeof node?.number !== 'number' || typeof node.title !== 'string' || typeof node.url !== 'string') return null;
  const repo: string | undefined = node.repository?.nameWithOwner;
  if (!repo) return null;
  const author: string = node.author?.login ?? 'ghost';
  const mine = author.toLowerCase() === viewer.toLowerCase();
  const review: ReviewState = ({ APPROVED: 'APPROVED', CHANGES_REQUESTED: 'CHANGES_REQUESTED', REVIEW_REQUIRED: 'REVIEW_REQUESTED' } as Record<string, ReviewState>)[node.reviewDecision] ?? 'NONE';
  const state = node.commits?.nodes?.[0]?.commit?.statusCheckRollup?.state;
  const ci: CiStatus = state === 'SUCCESS' ? 'PASSING' : state === 'PENDING' || state === 'EXPECTED' ? 'PENDING'
    : state === 'FAILURE' || state === 'ERROR' ? 'FAILING' : 'NONE';
  const reviewers: string[] = [];
  for (const r of node.reviewRequests?.nodes ?? []) {
    const who = r?.requestedReviewer?.login ?? r?.requestedReviewer?.name;
    if (typeof who === 'string' && !reviewers.includes(who)) reviewers.push(who);
  }
  let clock: string = node.createdAt ?? '';
  for (const r of node.reviews?.nodes ?? []) {
    const who = r?.author?.login;
    if (typeof who === 'string' && who !== author) {
      if (typeof r.submittedAt === 'string' && r.submittedAt > clock) clock = r.submittedAt;
      if (!reviewers.includes(who)) reviewers.push(who);
    }
  }
  const base: string | undefined = node.baseRefName;
  const defaultBranch: string | undefined = node.repository?.defaultBranchRef?.name;
  return {
    id: `${repo}#${node.number}`, repo, number: node.number, title: node.title, url: node.url, author, mine,
    draft: node.isDraft === true, review, ci, reason: mine ? 'MINE' : reason, reviewers,
    updatedAt: node.updatedAt ?? '', reviewClock: clock, headRef: node.headRefName ?? '',
    headRepo: node.headRepository?.nameWithOwner ?? null,
    stackBase: base && defaultBranch && base !== defaultBranch ? base : null, rule: label,
  };
}

/** Adds a page's PRs to `prs`, keeping a PR once and its strongest reason (Rust's merge in `fetch_live`). */
export function mergePrs(prs: PullRequest[], incoming: PullRequest[]): void {
  for (const pr of incoming) {
    const existing = prs.find((p) => p.id === pr.id);
    if (!existing) prs.push(pr);
    else {
      if (pr.reason === 'ASSIGNED') existing.reason = 'ASSIGNED';
      existing.rule ??= pr.rule;
    }
  }
}

/** The events in GitHub's `/notifications` answer that meat-proxy alerts on at once. */
export function notificationPoll(body: Json[]): Event[] {
  const out: Event[] = [];
  for (const item of body) {
    if (!['review_requested', 'assign'].includes(item?.reason) || item.unread !== true || item.subject?.type !== 'PullRequest') continue;
    const repo = item.repository?.full_name;
    const number = Number(String(item.subject?.url ?? '').split('/').pop());
    if (!repo || !Number.isInteger(number)) continue;
    out.push({
      id: `assign-${repo}#${number}`,
      title: item.reason === 'review_requested' ? `Review requested on ${repo}#${number}` : `Assigned to ${repo}#${number}`,
      body: item.subject?.title ?? 'Pull request',
      url: `https://github.com/${repo}/pull/${number}`,
    });
  }
  return out;
}
