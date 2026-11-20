#!/bin/sh
# Builds one part of a release of the Exact app (exact/) into <out>, as .github/workflows/release.yml does:
#
#   scripts/package.sh web <tag> <out>   meat-proxy-web-<tag>.tar.gz, the web app to run on this computer: the web build
#                                        (its sign-in goes through http://127.0.0.1:8741), exact/server.mjs and VERSION,
#                                        plus install-web.sh
#   scripts/package.sh mac <tag> <out>   meat-proxy-macos.zip, the ad-hoc signed Mac app, plus install-mac.sh; on macOS
#
# Both read GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET, the OAuth app whose callback is http://127.0.0.1/. A desktop
# build carries both inside it, and the web part keeps the secret in oauth.env for its server, so either is as public
# as the release. Needs Exact set up (scripts/setup.sh, or EXACT).
set -eu

fail() { echo "package: $*" >&2; exit 1; }
[ $# -eq 3 ] || fail "usage: scripts/package.sh web|mac <tag> <out>"
what=$1 tag=$2
case "$tag" in v[0-9]*.[0-9]*.[0-9]*) ;; *) fail "the tag is vX.Y.Z, not $tag" ;; esac
mkdir -p "$3"
out=$(cd "$3" && pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
# The version Exact writes into the Mac app's Info.plist and the web build's app.version.json.
export EXACT_VERSION="${tag#v}"

case "$what" in
web)
	stage=$(mktemp -d)
	trap 'rm -rf "$stage"' EXIT
	# The secret stays out of the page: only the server reads it.
	(cd "$root/exact" && GITHUB_CLIENT_SECRET= GITHUB_TOKEN_ORIGIN=http://127.0.0.1:8741 bun exact.mjs web-build --out "$stage/dist")
	# The build's lock and earlier generations, which only its own server reads.
	rm -rf "$stage"/dist.*
	cp "$root/exact/server.mjs" "$root/exact/github-token.mjs" "$stage/"
	echo "$tag" >"$stage/VERSION"
	printf 'GITHUB_CLIENT_ID=%s\nGITHUB_CLIENT_SECRET=%s\n' "${GITHUB_CLIENT_ID:-}" "${GITHUB_CLIENT_SECRET:-}" >"$stage/oauth.env"
	tar -czf "$out/meat-proxy-web-$tag.tar.gz" -C "$stage" .
	cp "$root/scripts/install-web.sh" "$out/install-web.sh"
	echo "$out/meat-proxy-web-$tag.tar.gz"
	;;
mac)
	[ "$(uname -s)" = Darwin ] || fail "the Mac app builds on macOS"
	built=$(cd "$root/exact" && bun exact.mjs mac --release --json | tail -1)
	app=$(printf '%s' "$built" | sed -n 's/.*"path":"\([^"]*\)".*/\1/p')
	[ -d "$app" ] || fail "the Mac build made no app: $built"
	rm -f "$out/meat-proxy-macos.zip"
	# ditto, not zip, which breaks the bundle's symlinks and its signature. The zip holds meat-proxy.app whatever the
	# build named it, as install-mac.sh expects.
	work=$(mktemp -d)
	ditto "$app" "$work/meat-proxy.app"
	ditto -c -k --sequesterRsrc --keepParent "$work/meat-proxy.app" "$out/meat-proxy-macos.zip"
	rm -rf "$work"
	cp "$root/scripts/install-mac.sh" "$out/install-mac.sh"
	echo "$out/meat-proxy-macos.zip"
	;;
*) fail "usage: scripts/package.sh web|mac <tag> <out>" ;;
esac
