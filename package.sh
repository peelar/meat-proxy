#!/usr/bin/env bash
set -euo pipefail

# Builds meat-proxy.app and installs it in /Applications with a CLI launcher in ~/.cargo/bin:
#
#   ./package.sh
#
# Or, for a release (.github/workflows/release.yml), builds it as version <tag> and zips it into <out> instead:
#
#   ./package.sh --zip <tag> <out>     <out>/meat-proxy-macos.zip, the ad-hoc signed app, and install-mac.sh
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

ZIP_OUT=""
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
BUILD_NUMBER=1
if [[ "${1:-}" == "--zip" ]]; then
    [[ $# -eq 3 && "$2" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: ./package.sh --zip vX.Y.Z <out>" >&2; exit 1; }
    VERSION="${2#v}"
    # A build number that only grows: the version's parts, as one number.
    IFS=. read -r major minor patch <<<"$VERSION"
    BUILD_NUMBER=$(( major * 1000000 + minor * 1000 + patch ))
    mkdir -p "$3"
    ZIP_OUT="$(cd "$3" && pwd)"
fi

echo "Building meat-proxy (release)…"
cargo build --release --locked

BIN_PATH="$SCRIPT_DIR/target/release/meat-proxy"
APP_BUNDLE="$SCRIPT_DIR/target/release/meat-proxy.app"

mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"
cp "$BIN_PATH" "$APP_BUNDLE/Contents/MacOS/meat-proxy"
cp "$SCRIPT_DIR/assets/meat-proxy.icns" "$APP_BUNDLE/Contents/Resources/meat-proxy.icns"

cat > "$APP_BUNDLE/Contents/Info.plist" <<PLIST
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
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUILD_NUMBER</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP_BUNDLE"
codesign --verify --strict "$APP_BUNDLE"

if [[ -n "$ZIP_OUT" ]]; then
    rm -f "$ZIP_OUT/meat-proxy-macos.zip"
    # ditto, not zip, which breaks the bundle's symlinks and its signature.
    ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$ZIP_OUT/meat-proxy-macos.zip"
    cp "$SCRIPT_DIR/scripts/install-mac.sh" "$ZIP_OUT/install-mac.sh"
    echo "$ZIP_OUT/meat-proxy-macos.zip"
    exit 0
fi

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
