#!/usr/bin/env bash
# The parity drive, on macOS: both apps' screens in light and dark, and their inbox rows compared.
# Needs `cargo build --release` at the root and Exact set up as exact/AGENTS.md says. Writes .amp/in/parity/.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=$root/.amp/in/parity
mkdir -p "$out/rune" "$out/exact"
rune=$root/target/release/meat-proxy

python3 "$root/harness/api.py" 8787 >/tmp/meat-proxy-api.log 2>&1 &
api=$!
trap 'kill $api' EXIT
sleep 1

# Rune: its own preview scenes, which show github::demo_prs(), the data the fake API serves.
for s in onboarding preferences inbox settings action-needed; do
  for t in light dark; do "$rune" --scene "$s" --theme "$t" --size 920x680 --chrome --snap "$out/rune/$s-$t.png" >/dev/null 2>&1; done
done
"$rune" --scene inbox --size 920x2400 --tree "$out/rune-inbox.json" >/dev/null 2>&1
"$rune" --scene action-needed --size 920x2400 --tree "$out/rune-action.json" >/dev/null 2>&1

# Exact: the real flow against the fake API, signed out first, then connected, through setup into the inbox.
cd "$root/exact"
agent() { bun exact.mjs agent macos --epoch now --chrome platform --permission notifications=granted "$@"; }
onboard=("clock data" "clock settle" "tap onboard-continue" "clock data" "clock settle" "tap confirm-onboarding"
  "clock data" "clock settle" "tap repo-actions-anomalyco/rune" "tap menu-watch" "clock data" "clock settle")

agent --size 920x680 --reset-storage --storage signed-out "clock data" "clock settle" \
  "screenshot $out/exact/signed-out-light.png window" "tap preview-sample" "clock data" "clock settle" \
  "screenshot $out/exact/sample-light.png window" >/dev/null

export GITHUB_API=http://127.0.0.1:8787 MEAT_PROXY_TEST_TOKEN=fake
for t in light dark; do
  agent --size 920x680 --reset-storage --storage "drive-$t" "prefer color-scheme $t" "clock data" "clock settle" \
    "screenshot $out/exact/onboarding-$t.png window" "tap onboard-continue" "clock data" "clock settle" \
    "screenshot $out/exact/preferences-$t.png window" "tap confirm-onboarding" "clock data" "clock settle" \
    "tap repo-actions-anomalyco/rune" "tap menu-watch" "clock data" "clock settle" \
    "screenshot $out/exact/inbox-$t.png window" "tap pr-412 hover" "clock settle" \
    "screenshot $out/exact/hover-$t.png window" "tap filter-action" "clock data" "clock settle" \
    "screenshot $out/exact/action-needed-$t.png window" "tap open-settings" "clock data" "clock settle" \
    "type rule-name Core" "type rule-query 'repo:anomalyco/rune'" "tap add-rule" "clock data" "clock settle" \
    "screenshot $out/exact/settings-$t.png window" >/dev/null
done

agent --size 920x2400 --reset-storage --storage rows "${onboard[@]}" >/dev/null
agent --size 920x2400 --storage rows --json "clock data" "clock settle" "tree --ax" 2>/dev/null | tail -1 >"$out/exact-inbox.json"
agent --size 920x2400 --storage rows --json "clock data" "clock settle" "tap filter-action" "clock data" "clock settle" "tree --ax" 2>/dev/null | tail -1 >"$out/exact-action.json"

cd "$root"
status=0
for v in inbox action; do
  echo "== $v"
  python3 harness/compare.py "$out/rune-$v.json" "$out/exact-$v.json" || status=1
done
exit $status
