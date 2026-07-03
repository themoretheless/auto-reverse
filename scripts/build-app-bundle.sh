#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/build-app-bundle.sh [--debug|--release]

Builds a local macOS app bundle:
  target/debug/Auto Reverse.app
  target/release/Auto Reverse.app

Double-clicking the bundle opens the settings window (`auto-reverse ui`).
It also gives macOS Privacy & Security a stable .app target for granting
Accessibility and Input Monitoring. The settings window also owns the live
scroll event tap when enabled; the legacy `run` command is still available
for headless diagnostics.
USAGE
}

profile="debug"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      profile="debug"
      ;;
    --release)
      profile="release"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if [[ "$profile" == "release" ]]; then
  cargo build --release
else
  cargo build
fi

version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -n 1)"
binary="target/$profile/auto-reverse"
app="target/$profile/Auto Reverse.app"
contents="$app/Contents"
macos="$contents/MacOS"
resources="$contents/Resources"

if [[ ! -x "$binary" ]]; then
  echo "built binary is missing or not executable: $binary" >&2
  exit 1
fi

rm -rf "$app"
mkdir -p "$macos" "$resources"
cp "$binary" "$macos/auto-reverse-bin"
chmod 0755 "$macos/auto-reverse-bin"

# CFBundleExecutable is this launcher, not the real binary. A CGEventTap
# daemon has no AppKit/NSApplication run loop, so Finder can't reactivate it
# via Apple Events - double-clicking (or re-opening) a running headless
# process reliably shows "is not responding" even though it's healthy. The
# settings window (`ui`) does run a real AppKit loop via eframe/winit, so
# routing bare double-clicks there instead makes the bundle Finder-safe.
# `exec` replaces this shell's process image, so TCC/Accessibility trust
# still keys off auto-reverse-bin's own signed identity, not this script.
cat > "$macos/auto-reverse" <<'LAUNCHER'
#!/bin/sh
dir="$(cd -- "$(dirname -- "$0")" && pwd)"
if [ "$#" -eq 0 ]; then
  exec "$dir/auto-reverse-bin" ui
fi
exec "$dir/auto-reverse-bin" "$@"
LAUNCHER
chmod 0755 "$macos/auto-reverse"

cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Auto Reverse</string>
  <key>CFBundleExecutable</key>
  <string>auto-reverse</string>
  <key>CFBundleIdentifier</key>
  <string>com.auto-reverse.app</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Auto Reverse</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key>
  <string>10.13</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$app" >/dev/null
  sign_note="ad-hoc signed"
else
  sign_note="not signed: codesign not found"
fi

echo "Built $app ($sign_note)"
echo
echo "Add this app in:"
echo "  System Settings > Privacy & Security > Accessibility"
echo "  System Settings > Privacy & Security > Input Monitoring"
echo
echo "Open the settings window with:"
echo "  open \"$repo_root/$app\""
echo
echo "Run the background daemon instead (same identity, no window):"
echo "  \"$repo_root/$macos/auto-reverse\" run"
echo
echo "For terminal diagnostics through the bundled executable:"
echo "  \"$repo_root/$macos/auto-reverse\" doctor --no-create"
