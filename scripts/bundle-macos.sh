#!/usr/bin/env bash
set -euo pipefail

profile="${1:-release}"
if [[ "$profile" != "release" && "$profile" != "debug" ]]; then
  echo "Usage: $0 [release|debug]" >&2
  exit 2
fi

if [[ "$profile" == "release" ]]; then
  cargo build --locked --release
else
  cargo build --locked
fi

bundle="target/Still.app"
mkdir -p "$bundle/Contents/MacOS"
executable_tmp="$(mktemp "$bundle/Contents/MacOS/.still.XXXXXX")"
trap 'rm -f "$executable_tmp"' EXIT
cp "target/$profile/still" "$executable_tmp"
chmod 755 "$executable_tmp"
mv -f "$executable_tmp" "$bundle/Contents/MacOS/still"
trap - EXIT
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Still</string>
  <key>CFBundleDisplayName</key><string>Still</string>
  <key>CFBundleIdentifier</key><string>app.still.editor</string>
  <key>CFBundleExecutable</key><string>still</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
echo "Built $bundle"
