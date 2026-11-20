// meat-proxy's data module: GitHub through its API with a "Sign in with GitHub" token, what the person set up
// kept in one JSON file, and macOS notifications. The rules are logic.ts's; this file does the I/O and shapes
// rows for the view. The same code runs on the desktop and the web: only the token exchange differs (below).
import type { Answer, Result, Sources, Storage, Store } from './app.contract.d.ts';
import { oauth } from '@exact/oauth';
import { showNotification } from '@exact/notifications';
import {
  type Event, type Filter, type PullRequest, type RepoConfig, type Rule, type Thresholds,
  ageLabel, attention, idleHours, inferRepos, isNudge, mergePrs, moveRepo, notificationEvents, notificationPoll,
  orderedRepos, parsePr, PR_FIELDS, prsForRepo, requiresAction, searches, stackRows, statusOf,
} from './logic.ts';

// GitHub's OAuth app. Its token endpoint does not answer a browser (no CORS), so a web build sets
// GITHUB_TOKEN_ORIGIN to the origin it is served from, whose server.mjs adds the secret at /api/github-token;
// a desktop build exchanges the code with GitHub itself.
const CLIENT_ID = import.meta.env.GITHUB_CLIENT_ID ?? '';
const CLIENT_SECRET = import.meta.env.GITHUB_CLIENT_SECRET ?? '';
const TOKEN_ORIGIN = import.meta.env.GITHUB_TOKEN_ORIGIN ?? 'https://github.com';
const TOKEN_URL = TOKEN_ORIGIN === 'https://github.com' ? 'https://github.com/login/oauth/access_token' : `${TOKEN_ORIGIN}/api/github-token`;
// The API, which a launch may replace with the fake in harness/api.py. Its token is honoured
// only for an API that is not GitHub's, so a shipped build always signs in.
const API = import.meta.env.GITHUB_API ?? 'https://api.github.com';
const REAL = API === 'https://api.github.com';
const LAUNCH_TOKEN = import.meta.env.MEAT_PROXY_TEST_TOKEN ?? '';
const TEST_TOKEN = REAL ? '' : LAUNCH_TOKEN;

// Made on first use: a build with no OAuth app configured still runs, and its sign-in says what is missing.
let provider: ReturnType<typeof oauth> | null = null;
function github(): ReturnType<typeof oauth> {
  if (!CLIENT_ID) throw Object.assign(new Error('This build has no GitHub OAuth app: set GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET in .env.local.'), { code: 'config' });
  return provider ??= oauth({
    endpoints: { authorization: 'https://github.com/login/oauth/authorize', token: TOKEN_URL },
    clientId: CLIENT_ID,
    clientSecret: CLIENT_SECRET || undefined,
    scopes: ['repo', 'read:org', 'notifications'],
    key: 'github.session',
    apis: ['https://api.github.com'],
    commaScopes: true,
    endedErrors: ['bad_refresh_token'],
  });
}
const notConfigured = (e: unknown) => ({ signedIn: false, subject: '', expiresAt: 0, scopes: [], error: (e as Error).message, ended: false });

const FILE = 'app:/data/meat-proxy.json';
export const grants = [
  'net.fetch https://api.github.com',
  'net.fetch https://github.com',
  `net.fetch ${TOKEN_ORIGIN}`,
  'net.fetch http://127.0.0.1:8787',
  'auth.session https://github.com',
  'secret.keep github.session',
  'fs.read app:/data',
  'fs.write app:/data',
  'device.notifications notificationsPurpose',
].join('\n');


interface Saved {
  prs: PullRequest[];
  repos: RepoConfig[];
  rules: Rule[];
  snoozes: Record<string, number>;
  sent: string[];
  lastModified: string | null;
}
let saved: Saved | null = null;
let status: { state: 'checking' | 'connected' | 'needs-auth'; user: string; reason: string } = { state: 'checking', user: '', reason: '' };
let syncedGen = -1;
// What the last answer laid out, for Move up and Move down to move by.
const neighbours: Record<string, { ordered: RepoConfig[]; up: Record<string, number>; down: Record<string, number>; index: Record<string, number> }> = {};

async function load(storage: Storage): Promise<Saved> {
  if (saved) return saved;
  try {
    saved = JSON.parse(new TextDecoder().decode(await storage.fs.readFile(FILE))) as Saved;
  } catch (e) {
    const code = (e as { code?: string }).code;
    if (code !== 'ENOENT' && code !== 'bake') throw e;
    saved = { prs: [], repos: [], rules: [], snoozes: {}, sent: [], lastModified: null };
  }
  return saved;
}
const save = (storage: Storage) => storage.fs.atomicWriteFile(FILE, new TextEncoder().encode(JSON.stringify(saved)));

/** One GitHub API request, with the signed-in token. Throws a sentence for the person. */
async function api(store: Store, path: string, init: RequestInit = {}): Promise<Response> {
  const url = `${API}${path}`;
  const headers = { Accept: 'application/vnd.github+json', ...(init.headers as Record<string, string> ?? {}) };
  let res: Response;
  try {
    res = TEST_TOKEN
      ? await fetch(url, { ...init, headers: { ...headers, Authorization: `Bearer ${TEST_TOKEN}` } })
      : await github().fetch(store, url, { ...init, headers });
  } catch (e) {
    throw new Error((e as { code?: string }).code === 'signed_out' ? 'Sign in with GitHub to sync.' : `Could not reach GitHub. ${(e as Error).message}`);
  }
  if (res.status === 401) throw new Error('GitHub signed you out. Sign in again.');
  return res;
}

async function graphql(store: Store, query: string) {
  const res = await api(store, '/graphql', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query }) });
  const reply = await res.json();
  if (reply.errors?.[0]?.message) throw new Error(reply.errors[0].message);
  if (!res.ok || !reply.data) throw new Error(reply.message ?? `GitHub answered ${res.status}`);
  return reply.data;
}

/** Rust's `fetch_live`: the searches three to a request, every page of each. */
async function fetchLive(store: Store, repos: RepoConfig[], rules: Rule[]): Promise<{ viewer: string; prs: PullRequest[] }> {
  const all = searches(repos, rules);
  const prs: PullRequest[] = [];
  let viewer = '';
  for (let b = 0; b < all.length; b += 3) {
    let pending = all.slice(b, b + 3).map((s) => ({ s, cursor: null as string | null }));
    while (pending.length) {
      const parts = pending.map(({ s, cursor }, i) =>
        ` s${i}: search(query: ${JSON.stringify(s.query)}, type: ISSUE, first: 12, after: ${JSON.stringify(cursor)}) { nodes { ...PrFields } pageInfo { hasNextPage endCursor } }`);
      const data = await graphql(store, `query { viewer { login }${parts.join('')}}${PR_FIELDS}`);
      viewer = data.viewer?.login;
      if (!viewer) throw new Error('GitHub did not return the signed-in account');
      const next: typeof pending = [];
      pending.forEach(({ s, cursor }, i) => {
        const page = data[`s${i}`];
        if (!Array.isArray(page?.nodes)) throw new Error('GitHub did not return search results');
        mergePrs(prs, page.nodes.map((n: unknown) => parsePr(n, viewer, s.reason, s.label)).filter(Boolean) as PullRequest[]);
        if (page.pageInfo?.hasNextPage) {
          const end = page.pageInfo.endCursor;
          if (typeof end !== 'string' || end === cursor) throw new Error('GitHub returned an invalid search cursor');
          next.push({ s, cursor: end });
        }
      });
      pending = next;
    }
  }
  return { viewer, prs };
}

async function notify(events: Event[], storage: Storage, st: Saved): Promise<boolean> {
  let any = false;
  for (const e of events) {
    if (st.sent.includes(e.id)) continue;
    st.sent.push(e.id);
    any = true;
    // Not awaited: the first post waits for macOS's permission prompt, and a sync must not.
    showNotification({ title: e.title, body: e.body, tag: e.id, data: e.url }).catch(() => {});
  }
  if (any) await save(storage);
  return any;
}

// One sync per `gen`, shared by every source that asks for it.
let syncing: Promise<void> = Promise.resolve();
function sync(store: Store, gen: number, nowMs: number, open: number, draft: number, onboarded: boolean, alerts: boolean, storage: Storage): Promise<void> {
  if (gen !== syncedGen) {
    syncedGen = gen;
    syncing = runSync(store, nowMs, open, draft, onboarded, alerts, storage);
  }
  return syncing;
}

async function runSync(store: Store, nowMs: number, open: number, draft: number, onboarded: boolean, alerts: boolean, storage: Storage) {
  const st = await load(storage);
  try {
    const out = await fetchLive(store, st.repos, st.rules);
    const t: Thresholds = { draft, open, nowMs, snoozes: st.snoozes };
    const events = notificationEvents(st.prs, out.prs, new Set(st.sent), t);
    status = { state: 'connected', user: out.viewer, reason: '' };
    st.repos = inferRepos(out.prs, st.repos);
    st.prs = out.prs;
    if (onboarded) {
      if (alerts) await notify(events, storage, st);
      await save(storage);
    }
  } catch (e) {
    status = { state: 'needs-auth', user: '', reason: (e as Error).message };
  }
}

const parseFilter = (f: string): Filter => (['all', 'action', 'mine', 'drafts', 'theirs'].includes(f) ? f : 'all') as Filter;

function groupsOf(prs: PullRequest[], repos: RepoConfig[], filter: Filter, query: string, selected: string, smart: boolean, t: Thresholds) {
  const ordered = orderedRepos(repos, prs, smart, t);
  const groups = ordered
    .map((repo, index) => ({ repo, index, prs: prsForRepo(repo.name, prs, filter, query, t) }))
    .filter(({ repo }) => !selected || selected === repo.name)
    .filter(({ repo, prs }) => prs.length > 0 || ((repo.pinned || repo.watch) && filter === 'all' && !query.trim()));
  const up: Record<string, number> = {}, down: Record<string, number> = {}, index: Record<string, number> = {};
  groups.forEach((g, i) => {
    index[g.repo.name] = g.index;
    const prev = groups.slice(0, i).reverse().find((o) => o.repo.pinned === g.repo.pinned);
    const next = groups.slice(i + 1).find((o) => o.repo.pinned === g.repo.pinned);
    if (prev) up[g.repo.name] = prev.index;
    if (next) down[g.repo.name] = next.index;
  });
  neighbours.inbox = { ordered, up, down, index };
  return groups.map(({ repo, prs: list }) => ({
    name: repo.name, count: list.length, pinned: repo.pinned, watched: repo.watch,
    canUp: repo.name in up, canDown: repo.name in down,
    rows: stackRows(list, prs).map(({ pr, depth, parent }) => {
      const s = statusOf(pr, t);
      return {
        id: pr.id, number: pr.number, title: pr.title, url: pr.url, author: pr.author, mine: pr.mine,
        avatar: `https://avatars.githubusercontent.com/${pr.author.replace('[bot]', '%5Bbot%5D')}?s=48`,
        age: ageLabel(idleHours(pr, t.nowMs)), status: s.label, tone: s.tone, depth: Math.min(depth, 5),
        parent: parent ?? 0, ciFailing: pr.ci === 'FAILING', nudge: isNudge(attention(pr, t)), draft: pr.draft,
      };
    }),
  }));
}

const sources: Sources = {
  inbox: async ([gen, filter, query, selected, open, draft, smart, onboarded, alerts, sample, nowMs], store, storage) => {
    const t: Thresholds = { draft, open, nowMs, snoozes: {} };
    if (sample) {
      const prs = demoPrs(nowMs);
      return {
        state: 'connected', user: 'octocat', reason: '', total: prs.length,
        attention: prs.filter((p) => requiresAction(attention(p, t))).length,
        groups: groupsOf(prs, inferRepos(prs, []), parseFilter(filter), query, selected, true, t),
      } satisfies Result<'inbox'>;
    }
    await sync(store, gen, nowMs, open, draft, onboarded, alerts, storage);
    const st = await load(storage);
    t.snoozes = st.snoozes;
    return {
      state: status.state, user: status.user, reason: status.reason, total: st.prs.length,
      attention: st.prs.filter((p) => requiresAction(attention(p, t))).length,
      groups: groupsOf(st.prs, st.repos, parseFilter(filter), query, selected, smart, t),
    };
  },
  setupRepos: async ([gen, open, draft, smart, onboarded, alerts, nowMs], store, storage) => {
    await sync(store, gen, nowMs, open, draft, onboarded, alerts, storage);
    const st = await load(storage);
    const t: Thresholds = { draft, open, nowMs, snoozes: st.snoozes };
    const ordered = orderedRepos(st.repos, st.prs, smart, t);
    const visible = ordered.map((repo, index) => ({ repo, index }))
      .filter(({ repo }) => repo.pinned || repo.watch || st.prs.some((p) => p.repo === repo.name));
    const up: Record<string, number> = {}, down: Record<string, number> = {}, index: Record<string, number> = {};
    visible.forEach(({ repo, index: i }, pos) => {
      index[repo.name] = i;
      if (pos > 0) up[repo.name] = visible[pos - 1].index;
      if (pos + 1 < visible.length) down[repo.name] = visible[pos + 1].index;
    });
    neighbours.setup = { ordered, up, down, index };
    return visible.map(({ repo }) => ({ name: repo.name, watched: repo.watch, canUp: repo.name in up, canDown: repo.name in down }));
  },
  rules: async (_args, _store, storage) =>
    (await load(storage)).rules.map((r) => ({ id: r.id, name: r.name, query: ruleQueryText(r), enabled: r.enabled })),

  toggleRepo: async ([name, what], _store, storage) => {
    const st = await load(storage);
    const repo = st.repos.find((r) => r.name === name);
    if (repo) {
      if (what === 'pin') repo.pinned = !repo.pinned;
      else repo.watch = !repo.watch;
      await save(storage);
    }
    return { ok: true, message: '' };
  },
  moveRepo: async ([name, dir, scope], _store, storage) => {
    const st = await load(storage);
    const n = neighbours[scope];
    const to = n && (dir < 0 ? n.up[name] : n.down[name]);
    if (n && to !== undefined) {
      st.repos = moveRepo(n.ordered, n.index[name], to);
      await save(storage);
    }
    return { ok: to !== undefined, message: '' };
  },
  fixOrder: async ([nowMs, open, draft], _store, storage) => {
    // Switching to a fixed order keeps the order the person sees (Rust's `set_repository_order`).
    const st = await load(storage);
    st.repos = orderedRepos(st.repos, st.prs, true, { draft, open, nowMs, snoozes: st.snoozes }).map((r, i) => ({ ...r, rank: i }));
    await save(storage);
    return { ok: true, message: '' };
  },
  snooze: async ([id, nowMs], _store, storage) => {
    const st = await load(storage);
    st.snoozes[id] = Math.floor(nowMs / 3_600_000) + 24;
    await save(storage);
    return { ok: true, message: `Snoozed ${id} for 24h` };
  },
  addRule: async ([name, query], _store, storage) => {
    if (!name.trim() || !query.trim()) return { ok: false, message: 'Enter both a rule name and a GitHub filter query.' };
    const st = await load(storage);
    st.rules.push({ id: `rule-custom-${st.rules.length + 1}`, name: name.trim(), query: query.trim(), excludeDrafts: true, excludeMine: true, enabled: true });
    await save(storage);
    return { ok: true, message: `Added rule "${name.trim()}"` };
  },
  toggleRule: async ([id], _store, storage) => {
    const st = await load(storage);
    const rule = st.rules.find((r) => r.id === id);
    if (rule) rule.enabled = !rule.enabled;
    await save(storage);
    return { ok: true, message: '' };
  },
  removeRule: async ([id], _store, storage) => {
    const st = await load(storage);
    st.rules = st.rules.filter((r) => r.id !== id);
    await save(storage);
    return { ok: true, message: '' };
  },
  finishSetup: async (_args, _store, storage) => {
    await load(storage);
    await save(storage);
    return { ok: true, message: '' };
  },
  // A test token stands in for a session only against the harness's fake API.
  session: async (_args, store) => (TEST_TOKEN
    ? { signedIn: true, subject: '', expiresAt: 0, scopes: [], error: '', ended: false }
    : (() => { try { return github().session(store); } catch (e) { return notConfigured(e); } })()),
  signIn: (_args, store) => {
    try { return github().signIn(store); } catch (e) { return { ok: false, code: 'config', error: (e as Error).message, retryable: false, status: 0 }; }
  },
  signOut: (_args, store) => {
    syncedGen = -1;
    status = { state: 'checking', user: '', reason: '' };
    try { return github().signOut(store); } catch { return { ok: true, code: '', error: '', retryable: false, status: 0 }; }
  },
  /** Rust's `check_notifications`: GitHub's notifications since the last poll; true when one needs a sync. */
  pollNotifications: async ([alerts], store, storage) => {
    const st = await load(storage);
    if (!alerts || status.state !== 'connected') return { ok: false, message: '' };
    try {
      const res = await api(store, '/notifications?participating=true&per_page=50',
        st.lastModified ? { headers: { 'If-Modified-Since': st.lastModified } } : {});
      const modified = res.headers.get('last-modified');
      if (modified && modified !== st.lastModified) { st.lastModified = modified; await save(storage); }
      if (res.status === 304 || !res.ok) return { ok: false, message: '' };
      return { ok: await notify(notificationPoll(await res.json()), storage, st), message: '' };
    } catch {
      return { ok: false, message: '' };
    }
  },
};

function ruleQueryText(r: Rule): string {
  return ['is:pr', 'is:open', ...(r.excludeDrafts ? ['-is:draft'] : []), ...(r.excludeMine ? ['-author:@me'] : []), r.query.trim()].filter(Boolean).join(' ');
}

/** The sample inbox before GitHub is connected. */
function demoPrs(nowMs: number): PullRequest[] {
  const at = (h: number) => new Date(nowMs - h * 3_600_000).toISOString();
  const row = (repo: string, number: number, title: string, author: string, mine: boolean, draft: boolean, review: PullRequest['review'],
    ci: PullRequest['ci'], reason: PullRequest['reason'], reviewers: string[], idle: number, unreviewed: number, headRef: string, stackBase: string | null): PullRequest =>
    ({ id: `${repo}#${number}`, repo, number, title, url: `https://github.com/${repo}/pull/${number}`, author, mine, draft, review, ci, reason, reviewers,
      updatedAt: at(idle), reviewClock: at(unreviewed), headRef, headRepo: repo, stackBase, rule: null });
  return [
    row('anomalyco/rune', 418, 'metal-backend: coalesce offscreen Rgba16Float layer binds during pinch', 'thdxr', false, false, 'REVIEW_REQUESTED', 'PASSING', 'ASSIGNED', ['peelar'], 1, 1, 'metal-layer-coalesce', null),
    row('anomalyco/rune', 412, 'taffy: preserve subpixel baseline cache across color-only commits', 'peelar', true, false, 'NONE', 'PASSING', 'MINE', [], 54, 54, 'feat/baseline-cache', 'metal-layer-coalesce'),
    row('anomalyco/rune', 397, 'WIP: experimental CoreAudio low-latency spatial voice ring', 'peelar', true, true, 'NONE', 'PENDING', 'MINE', [], 120, 120, 'draft/coreaudio-ring', null),
    row('anomalyco/rune', 421, 'windfoil: tighten CJK autohint stem dilation at 1x scale', 'adamw', false, false, 'COMMENTED', 'PASSING', 'WATCHED', ['thdxr'], 5, 5, 'windfoil-cjk-1x', 'feat/baseline-cache'),
    row('anomalyco/opencode', 904, 'acp: stream tool call diffs over zero-copy IPC channel', 'rekram', false, false, 'REVIEW_REQUESTED', 'PASSING', 'ASSIGNED', ['peelar'], 3, 3, 'acp-zero-copy-diffs', null),
    row('anomalyco/opencode', 889, 'session: retry transient rate-limit 529 with jittered backoff', 'peelar', true, false, 'REVIEW_REQUESTED', 'PASSING', 'MINE', ['rekram'], 74, 74, 'fix/retry-529-backoff', null),
    row('anomalyco/opencode', 908, 'cli: add `--json-events` flag for headless subagent pipelines', 'peelar', true, false, 'APPROVED', 'PASSING', 'MINE', ['thdxr'], 2, 0, 'feat/json-events', null),
    row('peelar/meat-proxy', 14, 'sqlite: persist per-repo smart importance weights & snooze deadlines', 'peelar', true, false, 'CHANGES_REQUESTED', 'FAILING', 'MINE', ['alex'], 6, 6, 'sqlite-importance-weights', null),
    row('peelar/meat-proxy', 19, 'draft: webhook websocket bridge for sub-second PR assignment pushes', 'peelar', true, true, 'NONE', 'PENDING', 'MINE', [], 8, 8, 'draft/ws-webhook-bridge', null),
  ];
}

export const answer: Answer = (source, args, store, storage, native) => sources[source](args, store, storage, native);
