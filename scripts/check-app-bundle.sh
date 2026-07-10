#!/usr/bin/env bash
set -euo pipefail

profile="${1:-debug}"
app="target/$profile/Auto Reverse.app"
contents="$app/Contents"
executable="$contents/MacOS/auto-reverse"
icon="$contents/Resources/AutoReverse.icns"

if [[ ! -d "$app" ]]; then
  echo "app bundle is missing: $app" >&2
  exit 1
fi
if [[ ! -x "$executable" ]]; then
  echo "bundle executable is missing or not executable: $executable" >&2
  exit 1
fi

file_description="$(file "$executable")"
if [[ "$file_description" != *"Mach-O"* ]]; then
  echo "bundle executable is not Mach-O: $file_description" >&2
  exit 1
fi

plutil -lint "$contents/Info.plist" >/dev/null
bundle_executable="$(plutil -extract CFBundleExecutable raw "$contents/Info.plist")"
if [[ "$bundle_executable" != "auto-reverse" ]]; then
  echo "unexpected CFBundleExecutable: $bundle_executable" >&2
  exit 1
fi
if [[ ! -f "$icon" ]]; then
  echo "bundle icon is missing: $icon" >&2
  exit 1
fi

if command -v codesign >/dev/null 2>&1; then
  codesign --verify --deep --strict "$app"
fi
echo "Bundle check passed: $app"
