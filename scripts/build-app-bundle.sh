#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/lib/app-bundle.sh"

usage() {
  cat <<'USAGE'
Usage:
  scripts/build-app-bundle.sh [--debug|--release]
      [--ad-hoc|--sign-identity IDENTITY]

Builds a local macOS app bundle:
  target/debug/Auto Reverse.app
  target/release/Auto Reverse.app

Double-clicking the bundle opens the settings window (`auto-reverse ui`).
It also gives macOS Privacy & Security a stable .app target for granting
Accessibility and Input Monitoring. The settings window also owns the live
scroll event tap when enabled; the legacy `run` command is still available
for headless diagnostics.

Local builds use an ad-hoc signature by default. Passing a signing identity
requires a Developer ID Application signature, secure timestamp, and hardened
runtime suitable for the notarization workflow.
USAGE
}

profile="debug"
sign_identity="-"
sign_selection=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      profile="debug"
      ;;
    --release)
      profile="release"
      ;;
    --ad-hoc)
      if [[ -n "$sign_selection" ]]; then
        echo "choose only one of --ad-hoc or --sign-identity" >&2
        exit 2
      fi
      sign_identity="-"
      sign_selection="ad-hoc"
      ;;
    --sign-identity)
      if [[ -n "$sign_selection" ]]; then
        echo "choose only one of --ad-hoc or --sign-identity" >&2
        exit 2
      fi
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--sign-identity needs a non-empty identity" >&2
        exit 2
      fi
      sign_identity="$1"
      sign_selection="developer-id"
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

cd "$repo_root"

if [[ "$profile" == "release" ]]; then
  cargo build --release
else
  cargo build
fi

version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -n 1)"
binary="target/$profile/$AUTO_REVERSE_EXECUTABLE_NAME"
app="target/$profile/$AUTO_REVERSE_APP_BASENAME"
contents="$app/Contents"
macos="$contents/MacOS"
resources="$contents/Resources"
icon_source="assets/AppIcon.svg"
entitlements="packaging/AutoReverse.entitlements"

if [[ ! -x "$binary" ]]; then
  echo "built binary is missing or not executable: $binary" >&2
  exit 1
fi

rm -rf "$app"
mkdir -p "$macos" "$resources"
cp "$binary" "$macos/$AUTO_REVERSE_EXECUTABLE_NAME"
chmod 0755 "$macos/$AUTO_REVERSE_EXECUTABLE_NAME"

if [[ ! -f "$icon_source" ]]; then
  echo "app icon source is missing: $icon_source" >&2
  exit 1
fi
if [[ ! -f "$entitlements" ]]; then
  echo "entitlements file is missing: $entitlements" >&2
  exit 1
fi
plutil -lint "$entitlements" >/dev/null

iconset="$resources/AppIcon.iconset"
icon_base="$resources/AppIcon-1024.png"
mkdir -p "$iconset"
sips -s format png "$icon_source" --out "$icon_base" >/dev/null
while read -r filename pixels; do
  sips -z "$pixels" "$pixels" "$icon_base" --out "$iconset/$filename" >/dev/null
done <<'ICON_SIZES'
icon_16x16.png 16
icon_16x16@2x.png 32
icon_32x32.png 32
icon_32x32@2x.png 64
icon_128x128.png 128
icon_128x128@2x.png 256
icon_256x256.png 256
icon_256x256@2x.png 512
icon_512x512.png 512
icon_512x512@2x.png 1024
ICON_SIZES
iconutil -c icns "$iconset" -o "$resources/AutoReverse.icns"
rm -rf "$iconset"
rm -f "$icon_base"

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
  <string>$AUTO_REVERSE_EXECUTABLE_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$AUTO_REVERSE_BUNDLE_IDENTIFIER</string>
  <key>CFBundleIconFile</key>
  <string>AutoReverse</string>
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

if ! command -v codesign >/dev/null 2>&1; then
  echo "codesign is required to build a verifiable app bundle" >&2
  exit 1
fi

codesign_args=(
  --force
  --sign "$sign_identity"
  --options runtime
  --entitlements "$entitlements"
)
if [[ "$sign_identity" == "-" ]]; then
  sign_note="ad-hoc signed with hardened runtime"
else
  codesign_args+=(--timestamp)
  sign_note="Developer ID signed with hardened runtime and secure timestamp"
fi
codesign "${codesign_args[@]}" "$app" >/dev/null

check_args=("$profile" --require-hardened-runtime)
if [[ "$sign_identity" != "-" ]]; then
  check_args+=(--require-developer-id --require-secure-timestamp)
fi
scripts/check-app-bundle.sh "${check_args[@]}"

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
echo "  \"$repo_root/$macos/$AUTO_REVERSE_EXECUTABLE_NAME\" run"
echo
echo "For terminal diagnostics through the bundled executable:"
echo "  \"$repo_root/$macos/$AUTO_REVERSE_EXECUTABLE_NAME\" doctor --no-create"
