#!/usr/bin/env bash
# Build dist/Stripline.app: a universal (arm64 + x86_64) menu-bar app bundle,
# ad-hoc signed so macOS can remember its Accessibility permission.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP=dist/Stripline.app

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "bundle.sh must run on macOS" >&2
    exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
lipo -create -output "$APP/Contents/MacOS/stripline" \
    target/aarch64-apple-darwin/release/stripline \
    target/x86_64-apple-darwin/release/stripline

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>io.github.stripline</string>
    <key>CFBundleName</key>
    <string>Stripline</string>
    <key>CFBundleExecutable</key>
    <string>stripline</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>MIT License</string>
</dict>
</plist>
EOF

codesign --force --sign - --identifier io.github.stripline "$APP"
echo "Built $APP (version $VERSION)"
echo "Install:  cp -R $APP /Applications/ && /Applications/Stripline.app/Contents/MacOS/stripline service install"
