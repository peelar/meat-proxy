#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building meat-proxy (release)…"
cargo build --release --locked

BIN_PATH="$SCRIPT_DIR/target/release/meat-proxy"
APP_BUNDLE="$SCRIPT_DIR/target/release/meat-proxy.app"

mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"
cp "$BIN_PATH" "$APP_BUNDLE/Contents/MacOS/meat-proxy"
cp "$SCRIPT_DIR/assets/meat-proxy.icns" "$APP_BUNDLE/Contents/Resources/meat-proxy.icns"

cat > "$APP_BUNDLE/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>meat-proxy</string>
    <key>CFBundleIdentifier</key>
    <string>co.anomaly.meat-proxy</string>
    <key>CFBundleName</key>
    <string>meat-proxy</string>
    <key>CFBundleDisplayName</key>
    <string>meat-proxy</string>
    <key>CFBundleIconFile</key>
    <string>meat-proxy.icns</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP_BUNDLE"
codesign --verify --strict "$APP_BUNDLE"

# macOS permits notification authorization for this local ad-hoc build in Applications.
INSTALLED_APP="/Applications/meat-proxy.app"
if [[ -e "$INSTALLED_APP" ]]; then
    installed_id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$INSTALLED_APP/Contents/Info.plist")
    if [[ "$installed_id" != "co.anomaly.meat-proxy" ]]; then
        echo "Another app already uses $INSTALLED_APP" >&2
        exit 1
    fi
fi
ditto "$APP_BUNDLE" "$INSTALLED_APP"
codesign --verify --strict "$INSTALLED_APP"

CLI_DIR="$HOME/.cargo/bin"
mkdir -p "$CLI_DIR"
LAUNCHER="$(mktemp "$CLI_DIR/.meat-proxy.XXXXXX")"
{
    echo '#!/bin/bash'
    printf 'app=%q\n' "$INSTALLED_APP"
    cat <<'LAUNCHER_SCRIPT'
if (( $# > 0 )); then
    exec "$app/Contents/MacOS/meat-proxy" "$@"
fi

launch_env=(--env "PATH=$PATH")
for name in MEAT_PROXY_DB MEAT_PROXY_SILENT_OS_NOTIFY GH_CONFIG_DIR; do
    if [[ -n "${!name+x}" ]]; then
        launch_env+=(--env "$name=${!name}")
    fi
done
exec /usr/bin/open -a "$app" "${launch_env[@]}"
LAUNCHER_SCRIPT
} > "$LAUNCHER"
chmod 755 "$LAUNCHER"
mv -f "$LAUNCHER" "$CLI_DIR/meat-proxy"

echo "Installed CLI launcher -> $CLI_DIR/meat-proxy"
echo "Installed macOS app -> $INSTALLED_APP"
