#!/bin/sh
# Installs the meat-proxy Mac app (the Rune app, which reads GitHub through the GitHub CLI) from the latest GitHub
# release:
#
#   curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-mac.sh | sh
#
# It puts Meat Proxy.app in ~/Applications. The app keeps itself up to date from then on. The app is ad-hoc signed
# (no Apple Developer ID), which macOS opens because curl does not quarantine what it downloads.
# `sh install-mac.sh --uninstall` takes it all away.
set -eu

REPO="${MEAT_PROXY_REPO:-peelar/meat-proxy}"
APP="$HOME/Applications/Meat Proxy.app"
SUPPORT="$HOME/Library/Application Support/meat-proxy"
# Earlier versions ran a launchd agent through sh, which Login Items listed as "sh".
LABEL="co.anomaly.meat-proxy.updater"
AGENT="$HOME/Library/LaunchAgents/$LABEL.plist"

[ "$(uname -s)" = Darwin ] || { echo "meat-proxy for macOS installs on a Mac." >&2; exit 1; }

launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
rm -f "$AGENT"

if [ "${1:-}" = "--uninstall" ]; then
	pkill -f "$APP/Contents/MacOS/" 2>/dev/null || true
	rm -rf "$APP" "$SUPPORT"
	echo "meat-proxy is uninstalled."
	exit 0
fi

mkdir -p "$HOME/Applications" "$SUPPORT"

STAGED="$SUPPORT/staged"
echo "Installing meat-proxy from ${REPO}…"

# GitHub sends /releases/latest on to the latest release's tag page; that address names the tag.
latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
latest=${latest##*/}
case "$latest" in v[0-9]*) ;; *) echo "$REPO has no release yet" >&2; exit 1 ;; esac

if [ "$(cat "$SUPPORT/version" 2>/dev/null || true)" != "$latest" ] || [ ! -d "$APP" ]; then
	# An open app is never quit from here.
	if pgrep -f "$APP/Contents/MacOS/" >/dev/null; then
		echo "Meat Proxy is open; quit it and run this again to update." >&2
		exit 1
	fi
	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT
	curl -fsSL -o "$work/app.zip" "https://github.com/$REPO/releases/download/$latest/meat-proxy-macos.zip"
	ditto -x -k "$work/app.zip" "$work/unpacked"
	[ -d "$work/unpacked/Meat Proxy.app" ] || { echo "meat-proxy-macos.zip of $latest has no Meat Proxy.app" >&2; exit 1; }
	xattr -dr com.apple.quarantine "$work/unpacked/Meat Proxy.app" 2>/dev/null || true
	codesign --verify --deep --strict "$work/unpacked/Meat Proxy.app"
	rm -rf "$APP" "$STAGED"
	mv "$work/unpacked/Meat Proxy.app" "$APP"
	# Until now the app was called meat-proxy.app.
	rm -rf "$HOME/Applications/meat-proxy.app"
	echo "$latest" >"$SUPPORT/version"
fi

echo "meat-proxy $(cat "$SUPPORT/version") is in ~/Applications; it keeps itself up to date while it runs."
# The app reads GitHub through the GitHub CLI, as you are signed in to it.
if ! command -v gh >/dev/null && [ ! -x /opt/homebrew/bin/gh ] && [ ! -x /usr/local/bin/gh ]; then
	echo "It reads GitHub through the GitHub CLI, which is not installed: brew install gh && gh auth login"
fi
open "$APP"
