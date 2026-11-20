#!/bin/sh
# Installs the meat-proxy Mac app (the Rune app, which reads GitHub through the GitHub CLI) from the latest GitHub
# release and keeps it there:
#
#   curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-mac.sh | sh
#
# It puts meat-proxy.app in ~/Applications and adds a launchd agent that checks for a newer release at login and every
# hour, as imp's install-mac.sh does. While the app is closed, a new release replaces it at once. While it is open, the
# release is downloaded and waits in ~/Library/Application Support/meat-proxy/staged, and goes in at the first check
# after you quit it. The app is ad-hoc signed (no Apple Developer ID), which macOS opens because curl does not
# quarantine what it downloads. `sh install-mac.sh --uninstall` takes it all away.
set -eu

REPO="${MEAT_PROXY_REPO:-peelar/meat-proxy}"
APP="$HOME/Applications/meat-proxy.app"
SUPPORT="$HOME/Library/Application Support/meat-proxy"
LABEL="co.anomaly.meat-proxy.updater"
AGENT="$HOME/Library/LaunchAgents/$LABEL.plist"

[ "$(uname -s)" = Darwin ] || { echo "meat-proxy for macOS installs on a Mac." >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
	launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
	pkill -f "$APP/Contents/MacOS/" 2>/dev/null || true
	rm -rf "$APP" "$SUPPORT" "$AGENT"
	echo "meat-proxy is uninstalled."
	exit 0
fi

mkdir -p "$HOME/Applications" "$SUPPORT" "$HOME/Library/LaunchAgents"

# The updater, which launchd runs and which installs the app the first time, below.
cat >"$SUPPORT/update.sh" <<EOF
#!/bin/sh
# Keeps $APP at the latest release of $REPO. Written by install-mac.sh; launchd ($LABEL) runs it at login and hourly.
set -eu
REPO="$REPO"
APP="$APP"
SUPPORT="$SUPPORT"
EOF
cat >>"$SUPPORT/update.sh" <<'EOF'
STAGED="$SUPPORT/staged"
running() { pgrep -f "$APP/Contents/MacOS/" >/dev/null; }

installed=$(cat "$SUPPORT/version" 2>/dev/null || echo none)
# GitHub sends /releases/latest on to the latest release's tag page; that address names the tag.
latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
latest=${latest##*/}
case "$latest" in v[0-9]*) ;; *) echo "$(date): $REPO has no release yet" >&2; exit 1 ;; esac
if [ "$latest" = "$installed" ] && [ -d "$APP" ]; then
	rm -rf "$STAGED"
	exit 0
fi

if [ "$(cat "$STAGED/version" 2>/dev/null)" != "$latest" ] || [ ! -d "$STAGED/meat-proxy.app" ]; then
	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT
	curl -fsSL -o "$work/app.zip" "https://github.com/$REPO/releases/download/$latest/meat-proxy-macos.zip"
	ditto -x -k "$work/app.zip" "$work/unpacked"
	[ -d "$work/unpacked/meat-proxy.app" ] || { echo "$(date): meat-proxy-macos.zip of $latest has no meat-proxy.app" >&2; exit 1; }
	xattr -dr com.apple.quarantine "$work/unpacked/meat-proxy.app" 2>/dev/null || true
	codesign --verify --deep --strict "$work/unpacked/meat-proxy.app"
	rm -rf "$STAGED"
	mkdir -p "$STAGED"
	mv "$work/unpacked/meat-proxy.app" "$STAGED/meat-proxy.app"
	echo "$latest" >"$STAGED/version"
fi

# An open app is never quit from here; its release goes in at the first check after it closes.
running && exit 0
rm -rf "$APP.old"
[ -d "$APP" ] && mv "$APP" "$APP.old"
mv "$STAGED/meat-proxy.app" "$APP"
rm -rf "$APP.old" "$STAGED"
echo "$latest" >"$SUPPORT/version"
echo "$(date): installed meat-proxy $latest (was $installed)"
EOF
chmod +x "$SUPPORT/update.sh"

cat >"$AGENT" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key><string>$LABEL</string>
	<key>ProgramArguments</key><array><string>/bin/sh</string><string>$SUPPORT/update.sh</string></array>
	<key>RunAtLoad</key><true/>
	<key>StartInterval</key><integer>3600</integer>
	<key>StandardOutPath</key><string>$SUPPORT/update.log</string>
	<key>StandardErrorPath</key><string>$SUPPORT/update.log</string>
</dict>
</plist>
EOF

echo "Installing meat-proxy from ${REPO}…"
sh "$SUPPORT/update.sh"
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$AGENT"

echo "meat-proxy $(cat "$SUPPORT/version") is in ~/Applications and keeps itself up to date."
# The app reads GitHub through the GitHub CLI, as you are signed in to it.
if ! command -v gh >/dev/null && [ ! -x /opt/homebrew/bin/gh ] && [ ! -x /usr/local/bin/gh ]; then
	echo "It reads GitHub through the GitHub CLI, which is not installed: brew install gh && gh auth login"
fi
open "$APP"
