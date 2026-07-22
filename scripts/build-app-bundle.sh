#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/lib/app-bundle.sh"

usage() {
  cat <<'USAGE'
Usage:
  scripts/build-app-bundle.sh [--debug|--release]
      [--ad-hoc|--development-sign-identity IDENTITY|--sign-identity IDENTITY]

Builds a local macOS app bundle:
  target/debug/Auto Reverse.app
  target/release/Auto Reverse.app

The bundle requires macOS 13.0 or newer because its GUI uses SMAppService.
MACOSX_DEPLOYMENT_TARGET may raise that minimum, but may not lower it.

Double-clicking the bundle opens the settings window (`auto-reverse ui`).
It also gives macOS Privacy & Security a stable .app target for granting
Accessibility. The settings window also owns the live
scroll event tap when enabled; the legacy `run` command is still available
for headless diagnostics.

Local builds use an ad-hoc signature by default. A development identity keeps
TCC identity stable across local rebuilds but can never pass the public release
gate. --sign-identity requires Developer ID Application authority, a secure
timestamp, and hardened runtime suitable for the notarization workflow.
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
        echo "choose only one signing mode" >&2
        exit 2
      fi
      sign_identity="-"
      sign_selection="ad-hoc"
      ;;
    --development-sign-identity)
      if [[ -n "$sign_selection" ]]; then
        echo "choose only one signing mode" >&2
        exit 2
      fi
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--development-sign-identity needs a non-empty identity" >&2
        exit 2
      fi
      sign_identity="$1"
      sign_selection="development"
      ;;
    --sign-identity)
      if [[ -n "$sign_selection" ]]; then
        echo "choose only one signing mode" >&2
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

readonly required_macos_version="13.0"
deployment_target="${MACOSX_DEPLOYMENT_TARGET:-$required_macos_version}"

if [[ ! "$deployment_target" =~ ^[0-9]+([.][0-9]+){0,2}$ ]]; then
  echo "invalid MACOSX_DEPLOYMENT_TARGET: $deployment_target (expected N, N.N, or N.N.N)" >&2
  exit 2
fi
if ! awk -v actual="$deployment_target" -v required="$required_macos_version" '
  BEGIN {
    split(actual, a, ".")
    split(required, r, ".")
    for (i = 1; i <= 3; i++) {
      av = (a[i] == "" ? 0 : a[i]) + 0
      rv = (r[i] == "" ? 0 : r[i]) + 0
      if (av > rv) exit 0
      if (av < rv) exit 1
    }
    exit 0
  }
'; then
  echo "MACOSX_DEPLOYMENT_TARGET $deployment_target is below macOS $required_macos_version required by SMAppService" >&2
  exit 2
fi

# One value drives both rustc's linker target and LSMinimumSystemVersion. The
# bundle smoke check independently verifies the resulting Mach-O load command.
export MACOSX_DEPLOYMENT_TARGET="$deployment_target"

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
# Keep editable SVG/PNG sources beside a checked-in multi-resolution ICNS.
# Runtime iconset conversion is not stable across macOS tool versions.
icon_source="assets/AutoReverse.icns"
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

if [[ "$(file "$icon_source")" != *"Mac OS X icon"* ]]; then
  echo "app icon source is not a valid ICNS file: $icon_source" >&2
  exit 1
fi
cp "$icon_source" "$resources/AutoReverse.icns"

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
  <string>$deployment_target</string>
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
if [[ "$sign_selection" == "development" ]]; then
  sign_note="Apple Development signed with hardened runtime (local use only)"
elif [[ "$sign_identity" == "-" ]]; then
  sign_note="ad-hoc signed with hardened runtime"
else
  codesign_args+=(--timestamp)
  sign_note="Developer ID signed with hardened runtime and secure timestamp"
fi
codesign "${codesign_args[@]}" "$app" >/dev/null

check_args=("$profile" --require-hardened-runtime)
if [[ "$sign_selection" == "development" ]]; then
  check_args+=(--require-apple-development)
elif [[ "$sign_identity" != "-" ]]; then
  check_args+=(--require-developer-id --require-secure-timestamp)
fi
scripts/check-app-bundle.sh "${check_args[@]}"

echo "Built $app ($sign_note)"
echo
echo "Add this app in:"
echo "  System Settings > Privacy & Security > Accessibility"
echo
echo "Open the settings window with:"
echo "  open \"$repo_root/$app\""
echo
echo "Run the background daemon instead (same identity, no window):"
echo "  \"$repo_root/$macos/$AUTO_REVERSE_EXECUTABLE_NAME\" run"
echo
echo "For terminal diagnostics through the bundled executable:"
echo "  \"$repo_root/$macos/$AUTO_REVERSE_EXECUTABLE_NAME\" doctor --no-create"
