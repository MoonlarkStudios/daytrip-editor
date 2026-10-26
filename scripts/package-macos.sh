#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

bash scripts/bundle-macos.sh
python3 scripts/release.py licenses target/Dosty.app/Contents/Resources/licenses
# Adding notices changes the signed bundle resources.
codesign --force --sign - target/Dosty.app
codesign --verify --deep --strict target/Dosty.app

version="$(python3 -c 'import sys; sys.path.insert(0, "scripts"); from release import metadata; print(metadata()["version"])')"
architecture="$(uname -m)"
case "$architecture" in
  arm64|x86_64) ;;
  *) echo "Unsupported macOS architecture: $architecture" >&2; exit 1 ;;
esac
mkdir -p target/dist
staging="$(mktemp -d target/dmg.XXXXXX)"
trap 'rm -rf "$staging"' EXIT
ditto target/Dosty.app "$staging/Dosty.app"
ln -s /Applications "$staging/Applications"
image="target/dist/Dosty-$version-macos-$architecture.dmg"
hdiutil create -volname "Dosty $version" -srcfolder "$staging" -format UDZO -ov "$image"
hdiutil verify "$image"
