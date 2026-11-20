#!/bin/sh
# Installs the meat-proxy web app on this computer from the latest GitHub release, as a service on
# http://127.0.0.1:8741 that keeps itself up to date, then opens it:
#
#   curl -fsSL https://github.com/peelar/meat-proxy/releases/latest/download/install-web.sh | sh
#
# On macOS the service is a launchd agent, on Linux a systemd user unit. Everything lives in MEAT_PROXY_HOME (default
# ~/.meat-proxy): releases/<tag> holds each unpacked release, `current` links the one that runs, `env` holds your own
# settings. The server (exact/server.mjs) checks for a newer release every hour, unpacks it beside this one, moves
# `current` and exits, and the service starts it again. MEAT_PROXY_AUTO_UPDATE=0 in `env` stops that. Run this again
# at any time to update and restart it; `sh install-web.sh --uninstall` removes it. Needs Node 20 or newer.
set -eu

REPO="${MEAT_PROXY_REPO:-peelar/meat-proxy}"
# Where releases are listed: GitHub's, or a test's stand-in.
RELEASES="${MEAT_PROXY_RELEASES:-https://github.com/$REPO/releases}"
HOME_DIR="${MEAT_PROXY_HOME:-$HOME/.meat-proxy}"
PORT=8741
LABEL="co.anomaly.meat-proxy.web"
AGENT="$HOME/Library/LaunchAgents/$LABEL.plist"
UNIT="$HOME/.config/systemd/user/meat-proxy.service"

fail() { echo "install-web: $*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
	if [ "$(uname -s)" = Darwin ]; then
		launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
		rm -f "$AGENT"
	else
		systemctl --user disable --now meat-proxy 2>/dev/null || true
		rm -f "$UNIT"
	fi
	rm -rf "$HOME_DIR"
	echo "meat-proxy's web app is uninstalled."
	exit 0
fi

command -v curl >/dev/null || fail "needs curl"
command -v tar >/dev/null || fail "needs tar"
NODE=$(command -v node) || fail "needs Node 20 or newer on PATH (https://nodejs.org)"
[ "$("$NODE" -p 'process.versions.node.split(".")[0]')" -ge 20 ] || fail "needs Node 20 or newer; $NODE is $("$NODE" --version)"

# GitHub sends /releases/latest on to the latest release's tag page; that address names the tag.
tag=${MEAT_PROXY_VERSION:-$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$RELEASES/latest")}
tag=${tag##*/}
case "$tag" in v[0-9]*) ;; *) fail "$REPO has no release yet" ;; esac

mkdir -p "$HOME_DIR/releases"
if [ ! -d "$HOME_DIR/releases/$tag" ]; then
	echo "Downloading meat-proxy ${tag}…"
	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT
	curl -fsSL -o "$work/web.tar.gz" "$RELEASES/download/$tag/meat-proxy-web-$tag.tar.gz" || fail "$REPO has no web app in $tag"
	mkdir "$work/release"
	tar -xzf "$work/web.tar.gz" -C "$work/release"
	[ "$(cat "$work/release/VERSION")" = "$tag" ] || fail "meat-proxy-web-$tag.tar.gz is not release $tag"
	mv "$work/release" "$HOME_DIR/releases/$tag"
fi
ln -sfn "releases/$tag" "$HOME_DIR/current"

[ -f "$HOME_DIR/env" ] || cat >"$HOME_DIR/env" <<'EOF'
# meat-proxy's web server settings, read at every start, after the release's own oauth.env (exact/server.mjs).
# MEAT_PROXY_AUTO_UPDATE=0
EOF

# What the service runs: the current release's server, with the release's OAuth app and then your settings. Node's
# path is the one found now, since a service does not see this shell's PATH.
cat >"$HOME_DIR/run.sh" <<EOF
#!/bin/sh
set -a
. "$HOME_DIR/current/oauth.env"
. "$HOME_DIR/env"
set +a
export MEAT_PROXY_HOME="$HOME_DIR" MEAT_PROXY_REPO="$REPO" MEAT_PROXY_RELEASES="$RELEASES" PORT=$PORT
exec "$NODE" "$HOME_DIR/current/server.mjs"
EOF
chmod +x "$HOME_DIR/run.sh"

if [ "${MEAT_PROXY_NO_SERVICE:-}" = 1 ]; then
	echo "meat-proxy $tag is in $HOME_DIR; start it with $HOME_DIR/run.sh"
	exit 0
fi

if [ "$(uname -s)" = Darwin ]; then
	mkdir -p "$(dirname "$AGENT")"
	cat >"$AGENT" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key><string>$LABEL</string>
	<key>ProgramArguments</key><array><string>/bin/sh</string><string>$HOME_DIR/run.sh</string></array>
	<key>RunAtLoad</key><true/>
	<key>KeepAlive</key><true/>
	<key>StandardOutPath</key><string>$HOME_DIR/server.log</string>
	<key>StandardErrorPath</key><string>$HOME_DIR/server.log</string>
</dict>
</plist>
EOF
	launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
	launchctl bootstrap "gui/$(id -u)" "$AGENT"
	logs="tail -f $HOME_DIR/server.log"
else
	command -v systemctl >/dev/null || fail "needs systemd to run as a service; MEAT_PROXY_NO_SERVICE=1 installs without one"
	mkdir -p "$(dirname "$UNIT")"
	cat >"$UNIT" <<EOF
[Unit]
Description=meat-proxy web app

[Service]
ExecStart=/bin/sh $HOME_DIR/run.sh
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
EOF
	systemctl --user daemon-reload
	systemctl --user enable meat-proxy >/dev/null 2>&1
	systemctl --user restart meat-proxy
	logs="journalctl --user -u meat-proxy -f"
fi

url="http://127.0.0.1:$PORT"
for _ in 1 2 3 4 5 6 7 8 9 10; do
	curl -fs "$url/healthz" >/dev/null 2>&1 && break
	sleep 1
done
curl -fs "$url/healthz" >/dev/null 2>&1 || fail "the server did not start; see: $logs"

cat <<EOF
meat-proxy $tag is running and keeps itself up to date.

  Web app:  $url
  Logs:     $logs
EOF
if [ "$(uname -s)" = Darwin ]; then open "$url"; elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && command -v xdg-open >/dev/null; then xdg-open "$url" >/dev/null 2>&1 || true; fi
