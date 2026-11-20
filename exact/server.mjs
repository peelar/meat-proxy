#!/usr/bin/env node
// meat-proxy's web server, in plain Node (Bun runs it too) and with no dependencies, so a release runs it without Exact:
//
//   node server.mjs [dist]    serves dist/ (default: the one beside this file) on HOST:PORT (default 127.0.0.1:8741)
//
// It does three things:
// - Serves the web build by the rule `bun exact.mjs web-build` prints for a static host: a file at the path; else, for
//   a browser asking for a page, shell.html (its client draws the route, or its notfound page); else 404.
// - Answers POST /api/github-token with github-token.mjs, which adds GITHUB_CLIENT_SECRET to the page's sign-in.
// - When an installer runs it (MEAT_PROXY_HOME, scripts/install-web.sh), keeps itself at the latest GitHub release: every
//   hour it downloads a newer release beside this one, links it as `current` and exits, and the service manager starts
//   it again from there. MEAT_PROXY_AUTO_UPDATE=0 turns that off.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, symlinkSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { githubToken } from './github-token.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(process.argv[2] ?? join(here, 'dist'));
const host = process.env.HOST ?? '127.0.0.1';
const port = Number(process.env.PORT ?? 8741);

if (!existsSync(join(dist, 'shell.html'))) {
  console.error(`server: no web build in ${dist} (bun exact.mjs web-build --out dist writes one)`);
  process.exit(1);
}
if (!process.env.GITHUB_CLIENT_SECRET) console.warn('server: GITHUB_CLIENT_SECRET is unset, so Sign in with GitHub fails');

const TYPES = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.webmanifest': 'application/manifest+json',
  '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.ico': 'image/x-icon',
  '.webp': 'image/webp', '.woff2': 'font/woff2', '.woff': 'font/woff', '.ttf': 'font/ttf', '.txt': 'text/plain; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8', '.xml': 'application/xml',
};
// routes.json names the files whose names carry their content's digest, which a browser may keep for good.
const immutable = (() => {
  try { return new Set(JSON.parse(readFileSync(join(dist, 'routes.json'), 'utf8')).files ?? []); } catch { return new Set(); }
})();

/** The file under dist/ that `pathname` names, or null. */
function fileAt(pathname) {
  let path;
  try { path = normalize(join(dist, decodeURIComponent(pathname))); } catch { return null; }
  if (path !== dist && !path.startsWith(dist + sep)) return null;
  for (const candidate of [path, join(path, 'index.html')]) {
    try { if (statSync(candidate).isFile()) return candidate; } catch {}
  }
  return null;
}

async function sendFile(res, path, status, method) {
  const name = path.slice(dist.length + 1).split(sep).join('/');
  res.writeHead(status, {
    // A file with no extension is a page, such as the sign-in callback at /.exact/auth/callback.
    'content-type': extname(path) ? TYPES[extname(path)] ?? 'application/octet-stream' : TYPES['.html'],
    'cache-control': immutable.has(name) || immutable.has(`/${name}`) ? 'public, max-age=31536000, immutable' : 'no-cache',
  });
  res.end(method === 'HEAD' ? undefined : await readFile(path));
}

const server = createServer(async (req, res) => {
  const { pathname } = new URL(req.url ?? '/', 'http://localhost');
  if (pathname === '/api/github-token') {
    if (req.method === 'POST') {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const reply = await githubToken(new Request('http://localhost/api/github-token', { method: 'POST', body: Buffer.concat(chunks) }));
      res.writeHead(reply.status, Object.fromEntries(reply.headers));
      return res.end(await reply.text());
    }
    res.writeHead(405, { allow: 'POST' });
    return res.end();
  }
  if (pathname === '/healthz') {
    res.writeHead(200, { 'content-type': 'text/plain' });
    return res.end(`${version()}\n`);
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405, { allow: 'GET, HEAD' });
    return res.end();
  }
  const file = fileAt(pathname);
  if (file) return sendFile(res, file, 200, req.method);
  if ((req.headers.accept ?? '').includes('text/html')) return sendFile(res, join(dist, 'shell.html'), 200, req.method);
  const missing = join(dist, '404.html');
  if (existsSync(missing)) return sendFile(res, missing, 404, req.method);
  res.writeHead(404, { 'cache-control': 'no-store' });
  res.end();
});
server.listen(port, host, () => console.log(`meat-proxy ${version()} on http://${host === '0.0.0.0' ? '127.0.0.1' : host}:${port}`));

function version() {
  try { return readFileSync(join(here, 'VERSION'), 'utf8').trim(); } catch { return 'dev'; }
}

// ---------------------------------------------------------------- updates, for an installed release
// The installer unpacks each release into MEAT_PROXY_HOME/releases/<tag> and runs MEAT_PROXY_HOME/current, a link to one.
const home = process.env.MEAT_PROXY_HOME;
const repo = process.env.MEAT_PROXY_REPO ?? 'peelar/meat-proxy';
// Where releases are listed: GitHub's, or a test's stand-in that answers /latest and /download/<tag>/<asset> as it does.
const releasesURL = process.env.MEAT_PROXY_RELEASES ?? `https://github.com/${repo}/releases`;
const TAG = /^v(\d+)\.(\d+)\.(\d+)$/;
const newer = (a, b) => {
  const x = TAG.exec(a)?.slice(1).map(Number), y = TAG.exec(b)?.slice(1).map(Number) ?? [0, 0, 0];
  if (!x) return false;
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
};

async function update() {
  // GitHub sends /releases/latest on to the latest release's tag page; that address names the tag.
  const latest = (await fetch(`${releasesURL}/latest`, { method: 'HEAD', redirect: 'follow' })).url.split('/').pop() ?? '';
  if (!newer(latest, version())) return;
  const releases = join(home, 'releases'), work = join(releases, `.${latest}.download`);
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  const asset = await fetch(`${releasesURL}/download/${latest}/meat-proxy-web-${latest}.tar.gz`);
  if (!asset.ok) throw new Error(`no web asset in ${latest} (${asset.status})`);
  const archive = join(work, 'web.tar.gz');
  await writeFile(archive, Buffer.from(await asset.arrayBuffer()));
  mkdirSync(join(work, 'release'));
  execFileSync('tar', ['-xzf', archive, '-C', join(work, 'release')]);
  if (readFileSync(join(work, 'release', 'VERSION'), 'utf8').trim() !== latest) throw new Error(`the web asset of ${latest} is another release`);
  rmSync(join(releases, latest), { recursive: true, force: true });
  renameSync(join(work, 'release'), join(releases, latest));
  rmSync(work, { recursive: true, force: true });
  // The link is replaced in one step: a new link beside it, renamed over it.
  const link = join(home, 'current'), next = join(home, '.current.next');
  rmSync(next, { force: true });
  symlinkSync(join('releases', latest), next);
  renameSync(next, link);
  console.log(`meat-proxy: updated to ${latest}; restarting`);
  server.close();
  process.exit(0);
}

if (home && process.env.MEAT_PROXY_AUTO_UPDATE !== '0' && TAG.test(version())) {
  const check = () => update().catch((error) => console.error(`meat-proxy: update check failed: ${error.message}`));
  setTimeout(check, Number(process.env.MEAT_PROXY_FIRST_CHECK_MS ?? 60_000));
  setInterval(check, 60 * 60_000);
}
