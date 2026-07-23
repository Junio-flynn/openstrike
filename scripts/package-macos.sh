#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
app="$project_dir/dist/OpenStrike.app"
contents="$app/Contents"
macos="$contents/MacOS"
resources="$contents/Resources"

cd "$project_dir"
bun run build:desktop
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release -p openstrike --target aarch64-apple-darwin
cargo build --release -p openstrike --target x86_64-apple-darwin

rm -rf "$app"
mkdir -p "$macos" "$resources/assets" "$resources/ui"
lipo -create \
  target/aarch64-apple-darwin/release/openstrike \
  target/x86_64-apple-darwin/release/openstrike \
  -output "$macos/openstrike-bin"
cp -R assets/maps assets/support assets/models "$resources/assets/"
cp dist/pocket/desktop/openstrike.js dist/pocket/desktop/openstrike.pak "$resources/ui/"

cat > "$macos/openstrike" <<'LAUNCHER'
#!/usr/bin/env bash
set -euo pipefail
macos_dir="$(cd "$(dirname "$0")" && pwd)"
resources="$(cd "$macos_dir/../Resources" && pwd)"
cd "$resources"
export OPENSTRIKE_ASSETS="$resources/assets"
export OPENSTRIKE_UI_DIST="$resources/ui"
export RUST_LOG="${RUST_LOG:-openstrike=info,pocket3d=info}"
exec "$macos_dir/openstrike-bin" --maps-dir "$resources/assets" --map open_arena
LAUNCHER
chmod +x "$macos/openstrike" "$macos/openstrike-bin"

cat > "$contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleDisplayName</key><string>OpenStrike</string>
  <key>CFBundleExecutable</key><string>openstrike</string>
  <key>CFBundleIdentifier</key><string>dev.pocketstack.openstrike</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>OpenStrike</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSLocalNetworkUsageDescription</key><string>OpenStrike discovers and joins multiplayer games on your local network.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$app"
fi
rm -f "$project_dir/dist/OpenStrike-macOS.zip"
ditto -c -k --keepParent "$app" "$project_dir/dist/OpenStrike-macOS.zip"
echo "share: $project_dir/dist/OpenStrike-macOS.zip"
