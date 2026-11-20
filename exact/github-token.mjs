// The web app's sign-in step that the browser cannot do itself: GitHub's token endpoint does not answer a browser
// (no CORS) and wants the OAuth app's secret. A web build built with GITHUB_TOKEN_ORIGIN posts its code exchange and
// refreshes to <that origin>/api/github-token, which answers with this: the request with GITHUB_CLIENT_SECRET added,
// sent on to GitHub. server.mjs serves it.
const GITHUB_TOKEN = 'https://github.com/login/oauth/access_token';

/** Answers a POST of the token request's form, as GitHub would. Only the code exchange and refresh pass. */
export async function githubToken(request) {
  const json = (status, body) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json', 'cache-control': 'no-store' } });
  const body = new URLSearchParams(await request.text());
  if (!['authorization_code', 'refresh_token'].includes(body.get('grant_type') ?? '')) return json(400, { error: 'unsupported_grant_type' });
  // The OAuth app's ID when set, else the one the page sent, which only this secret's app accepts.
  if (process.env.GITHUB_CLIENT_ID) body.set('client_id', process.env.GITHUB_CLIENT_ID);
  body.set('client_secret', process.env.GITHUB_CLIENT_SECRET ?? '');
  try {
    const reply = await fetch(GITHUB_TOKEN, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded', accept: 'application/json' },
      body,
    });
    return new Response(await reply.text(), {
      status: reply.status,
      headers: { 'content-type': reply.headers.get('content-type') ?? 'application/json', 'cache-control': 'no-store' },
    });
  } catch (error) {
    return json(502, { error: 'temporarily_unavailable', error_description: `GitHub did not answer: ${error.message}` });
  }
}
