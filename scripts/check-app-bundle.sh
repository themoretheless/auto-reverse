#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/lib/app-bundle.sh"
cd "$repo_root"

usage() {
  cat <<'USAGE'
Usage:
  scripts/check-app-bundle.sh [debug|release]
  scripts/check-app-bundle.sh --app /path/to/Auto\ Reverse.app
  scripts/check-app-bundle.sh --identity-only --app /path/to/Auto\ Reverse.app

Validates the real Mach-O executable, bundle identity, version, icon, plist,
LSUIElement mode, and code signature when codesign is available. Identity-only
mode recognizes an old/damaged installation before repair or removal without
requiring its resources or signature to still be intact.
USAGE
}

profile="debug"
app=""
identity_only=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    debug|--debug)
      profile="debug"
      ;;
    release|--release)
      profile="release"
      ;;
    --app)
      shift
      if [[ $# -eq 0 ]]; then
        echo "--app needs a path" >&2
        exit 2
      fi
      app="$1"
      ;;
    --identity-only)
      identity_only=true
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

if [[ -z "$app" ]]; then
  app="target/$profile/$AUTO_REVERSE_APP_BASENAME"
elif [[ "$app" != /* ]]; then
  app="$repo_root/$app"
fi

contents="$app/Contents"
plist="$contents/Info.plist"
executable="$(auto_reverse_bundle_executable "$app")"
icon="$contents/Resources/AutoReverse.icns"

if [[ ! -d "$app" ]]; then
  echo "app bundle is missing: $app" >&2
  exit 1
fi
if [[ ! -f "$plist" ]]; then
  echo "bundle Info.plist is missing: $plist" >&2
  exit 1
fi

plutil -lint "$plist" >/dev/null
bundle_executable="$(plutil -extract CFBundleExecutable raw "$plist")"
bundle_identifier="$(plutil -extract CFBundleIdentifier raw "$plist")"
bundle_type="$(plutil -extract CFBundlePackageType raw "$plist")"
bundle_name="$(plutil -extract CFBundleName raw "$plist")"

if [[ "$bundle_executable" != "$AUTO_REVERSE_EXECUTABLE_NAME" ]]; then
  echo "unexpected CFBundleExecutable: $bundle_executable" >&2
  exit 1
fi
if [[ "$bundle_identifier" != "$AUTO_REVERSE_BUNDLE_IDENTIFIER" ]]; then
  echo "unexpected CFBundleIdentifier: $bundle_identifier" >&2
  exit 1
fi
if [[ "$bundle_type" != "APPL" ]]; then
  echo "unexpected CFBundlePackageType: $bundle_type" >&2
  exit 1
fi
if [[ "$bundle_name" != "Auto Reverse" ]]; then
  echo "unexpected CFBundleName: $bundle_name" >&2
  exit 1
fi

if [[ "$identity_only" == true ]]; then
  echo "Bundle identity check passed: $app ($bundle_identifier)"
  exit 0
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

bundle_version="$(plutil -extract CFBundleShortVersionString raw "$plist")"
ui_element="$(plutil -extract LSUIElement raw "$plist")"
if [[ -z "$bundle_version" ]]; then
  echo "CFBundleShortVersionString is empty" >&2
  exit 1
fi
if [[ "$ui_element" != "true" && "$ui_element" != "1" ]]; then
  echo "LSUIElement must be true, got: $ui_element" >&2
  exit 1
fi
if [[ ! -f "$icon" ]]; then
  echo "bundle icon is missing: $icon" >&2
  exit 1
fi

if command -v codesign >/dev/null 2>&1; then
  codesign --verify --deep --strict "$app"
fi
echo "Bundle check passed: $app ($bundle_identifier, version $bundle_version)"
