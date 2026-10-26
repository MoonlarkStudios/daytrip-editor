#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"

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

bundle="target/Dosty.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
executable_tmp="$(mktemp "$bundle/Contents/MacOS/.dosty.XXXXXX")"
trap 'rm -f "$executable_tmp"' EXIT
cp "target/$profile/dosty" "$executable_tmp"
chmod 755 "$executable_tmp"
mv -f "$executable_tmp" "$bundle/Contents/MacOS/Dosty"
trap - EXIT
cp "branding/dosty/icons/dosty.icns" "$bundle/Contents/Resources/dosty.icns"
python3 - <<'PY'
import os
from pathlib import Path
import plistlib
import sys

sys.path.insert(0, "scripts")
from release import metadata

info = metadata(build_number=int(os.environ.get("DOSTY_BUILD_NUMBER", "1")))
plist = {
    "CFBundleName": "Dosty",
    "CFBundleDisplayName": "Dosty",
    "CFBundleIdentifier": "app.dosty.editor",
    "CFBundleExecutable": "Dosty",
    "CFBundleIconFile": "dosty.icns",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": info["base_version"],
    "CFBundleVersion": info["build_number"],
    "LSMinimumSystemVersion": os.environ["MACOSX_DEPLOYMENT_TARGET"],
    "NSHighResolutionCapable": True,
    "DostyVersion": info["version"],
}
with Path("target/Dosty.app/Contents/Info.plist").open("wb") as output:
    plistlib.dump(plist, output)
PY
codesign --force --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
echo "Built $bundle"
