#!/bin/sh
# Render the app to a PNG from a copy of your real saved data: no window, no GitHub calls,
# and the real database is never written to. The data is whatever the last sync saved.
# usage: scripts/snap-live.sh out.png [extra --snap flags, e.g. --theme dark --crop KEY]
set -eu
out=${1:?usage: snap-live.sh out.png [flags]}
shift
src="${MEAT_PROXY_DB:-$HOME/.config/meat-proxy/meat-proxy.sqlite3}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sqlite3 "$src" ".backup '$tmp/copy.sqlite3'"
cd "$(dirname "$0")/.."
MEAT_PROXY_DB="$tmp/copy.sqlite3" cargo run -q -- --scene live --snap "$out" --size 920x680 "$@"
