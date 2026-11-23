#!/usr/bin/env bash
set -euo pipefail

# Runs the development copy of meat proxy and restarts it whenever you save a file in src/:
#
#   scripts/dev.sh            hot reload (needs `cargo install cargo-watch`)
#   scripts/dev.sh --once     build and run once, no watching
#
# The dev copy is "Meat Proxy Dev" (bundle id co.anomaly.meat-proxy.dev). It keeps its data in
# ~/.config/meat-proxy-dev and its caches in ~/Library/Caches/meat-proxy-dev, so the installed
# app is never touched. It lives in target/ and is never put in /Applications.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

APP="$PWD/target/debug/Meat Proxy Dev.app"
export MEAT_PROXY_PROFILE=dev

build_and_run() {
    cargo build
    mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
    cp target/debug/meat-proxy "$APP/Contents/MacOS/meat-proxy-dev"
    cp assets/meat-proxy.icns "$APP/Contents/Resources/meat-proxy.icns"
    cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>meat-proxy-dev</string>
    <key>CFBundleIdentifier</key><string>co.anomaly.meat-proxy.dev</string>
    <key>CFBundleName</key><string>Meat Proxy Dev</string>
    <key>CFBundleDisplayName</key><string>Meat Proxy Dev</string>
    <key>CFBundleIconFile</key><string>meat-proxy.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.0.0-dev</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSEnvironment</key><dict><key>MEAT_PROXY_PROFILE</key><string>dev</string></dict>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    codesign --force --sign - "$APP" >/dev/null
    # Run the binary inside the bundle (not `open`) so the log stays here and a restart kills it.
    exec "$APP/Contents/MacOS/meat-proxy-dev"
}

case "${1:-}" in
--once | --run) build_and_run ;; # --run is what cargo-watch re-runs on every change
esac

command -v cargo-watch >/dev/null || { echo "Hot reload needs cargo-watch: cargo install cargo-watch" >&2; exit 1; }
exec cargo watch --why -w src -w Cargo.toml -w assets -s "$SCRIPT_DIR/dev.sh --run"
