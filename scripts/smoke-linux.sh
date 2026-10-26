#!/usr/bin/env bash
# Run only in disposable CI/container desktops, with isolated settings and no files.
set -euo pipefail
configuration="$(mktemp -d)"
trap 'rm -rf "$configuration"' EXIT
if DOSTY_CONFIG_DIR="$configuration" ICED_BACKEND=tiny-skia xvfb-run -a timeout 10s dosty; then
  echo "Dosty exited before the startup smoke check completed" >&2
  exit 1
else
  result=$?
  [[ "$result" -eq 124 ]]
fi
