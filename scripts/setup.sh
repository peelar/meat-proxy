#!/bin/sh
# Sets up a checkout to build and run the Exact app in exact/, as imp's scripts/setup.sh does for its client:
#
#   scripts/setup.sh        the web build: Bun, Rust, Exact and what its web build reads
#   scripts/setup.sh mac    that, and what the Mac app builds with (on macOS)
#
# Exact sits at the commit in exact/exact-rev, in ../../../tools/exact from exact/ (where exact/exact.mjs looks), or in
# EXACT. Only git and curl must be there first. Run it again after a pull: it moves Exact to a new pin.
# Then, in exact/: `bun exact.mjs web` (the web dev loop) or `bun exact.mjs mac --run` (the Mac app).
set -eu

fail() {
	echo "setup: $*" >&2
	exit 1
}
step() { printf '\n==> %s\n' "$*"; }

case "${1:-web}" in
web) platform=web ;;
mac)
	[ "$(uname -s)" = Darwin ] || fail "the Mac app builds on macOS"
	platform=macos
	;;
*) fail "usage: scripts/setup.sh [web|mac]" ;;
esac
root=$(cd "$(dirname "$0")/.." && pwd)
rev=$(tr -d '[:space:]' <"$root/exact/exact-rev")
exact=${EXACT:-$(cd "$root/../.." && pwd)/tools/exact}

step "Bun"
[ -x "$HOME/.bun/bin/bun" ] && PATH="$HOME/.bun/bin:$PATH"
if ! command -v bun >/dev/null; then
	# The version Exact pins (exact/exact.mjs); its installer adds ~/.bun/bin to the shell's profile.
	curl -fsSL https://bun.sh/install | bash -s bun-v1.4.2
	PATH="$HOME/.bun/bin:$PATH"
fi
echo "bun $(bun -v)"

step "Rust"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v rustup >/dev/null; then
	# No default toolchain: Exact's rust-toolchain.toml picks the ones it builds with.
	curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
	. "$HOME/.cargo/env"
fi

step "Exact $rev in $exact"
if [ ! -d "$exact/.git" ]; then
	[ ! -e "$exact" ] || fail "$exact exists and is not a git checkout: move it, or set EXACT to another path"
	mkdir -p "$exact" 2>/dev/null ||
		fail "cannot create $exact: set EXACT to a directory you own (and export it in your shell profile)"
	git init -q "$exact"
fi
if [ "$(git -C "$exact" rev-parse HEAD 2>/dev/null)" != "$rev" ]; then
	[ -z "$(git -C "$exact" status --porcelain)" ] ||
		fail "$exact has uncommitted changes: commit or stash them, and run this again to move it to $rev"
	git -C "$exact" fetch -q --depth 1 https://github.com/expo/exact "$rev"
	git -C "$exact" -c advice.detachedHead=false checkout -q "$rev"
fi
cd "$exact"
bun install --frozen-lockfile
if [ "$platform" = web ]; then
	bun scripts/exact.mjs setup --platform web
else
	bun scripts/exact.mjs setup
fi
bun scripts/exact.mjs setup --check --platform "$platform"

# The Exact modules the app uses (each `use … from "@exact/<name>"` in exact/app.contract), linked into
# exact/node_modules from this checkout, as imp's link-exact.sh does: `exact add` would write a fixed path to Exact
# into package.json, and Exact sits elsewhere in CI and releases.
step "Exact modules"
modules="$root/exact/node_modules/@exact"
mkdir -p "$modules"
for name in $(sed -n 's/^use .* from "@exact\/\([a-z0-9-]*\)"$/\1/p' "$root/exact/app.contract" | sort -u); do
	[ -d "$exact/packages/$name" ] || fail "no module $name in $exact/packages"
	ln -sfn "$exact/packages/$name" "$modules/$name"
	echo "@exact/$name"
done

cat <<EOF

Ready. In exact/: \`bun exact.mjs web\` runs the web app, \`bun exact.mjs mac --run\` the Mac app.
EOF
if [ -n "${EXACT:-}" ]; then
	echo "Exact is not in its default place, so keep EXACT=$exact exported for exact/'s commands."
fi
