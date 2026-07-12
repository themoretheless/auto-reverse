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
  scripts/check-app-bundle.sh --require-hardened-runtime [debug|release]
  scripts/check-app-bundle.sh --require-release-signature [debug|release]
  scripts/check-app-bundle.sh --require-notarized --app /path/to/app

Validates the real Mach-O executable, bundle identity, version, icon, plist,
LSUIElement mode, and code signature. Identity-only mode recognizes an
old/damaged installation before repair or removal without requiring its
resources or signature to still be intact.

Release-signature mode requires Developer ID Application authority, hardened
runtime, and a secure timestamp. Notarized mode adds stapled-ticket validation.
USAGE
}

profile="debug"
app=""
identity_only=false
require_hardened_runtime=false
require_developer_id=false
require_secure_timestamp=false
require_notarized=false
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
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--app needs a non-empty path" >&2
        exit 2
      fi
      app="$1"
      ;;
    --identity-only)
      identity_only=true
      ;;
    --require-hardened-runtime)
      require_hardened_runtime=true
      ;;
    --require-developer-id)
      require_developer_id=true
      ;;
    --require-secure-timestamp)
      require_secure_timestamp=true
      ;;
    --require-release-signature)
      require_hardened_runtime=true
      require_developer_id=true
      require_secure_timestamp=true
      ;;
    --require-notarized)
      require_hardened_runtime=true
      require_developer_id=true
      require_secure_timestamp=true
      require_notarized=true
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

if [[ "$identity_only" == true ]] && {
  [[ "$require_hardened_runtime" == true ]] ||
    [[ "$require_developer_id" == true ]] ||
    [[ "$require_secure_timestamp" == true ]] ||
    [[ "$require_notarized" == true ]]
}; then
  echo "--identity-only cannot be combined with strict signature checks" >&2
  exit 2
fi

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

if ! command -v codesign >/dev/null 2>&1; then
  echo "codesign is required for strict bundle validation" >&2
  exit 1
fi

codesign --verify --deep --strict "$app"
signature_details="$(codesign --display --verbose=4 "$app" 2>&1)"
if [[ "$signature_details" != *"Identifier=$AUTO_REVERSE_BUNDLE_IDENTIFIER"* ]]; then
  echo "code-signing identifier does not match $AUTO_REVERSE_BUNDLE_IDENTIFIER" >&2
  exit 1
fi
if [[ "$require_hardened_runtime" == true && "$signature_details" != *"flags="*"runtime"* ]]; then
  echo "bundle signature does not enable hardened runtime" >&2
  exit 1
fi
if [[ "$require_developer_id" == true && "$signature_details" != *"Authority=Developer ID Application:"* ]]; then
  echo "bundle is not signed by a Developer ID Application certificate" >&2
  exit 1
fi
if [[ "$require_secure_timestamp" == true && "$signature_details" != *"Timestamp="* ]]; then
  echo "bundle signature has no secure timestamp" >&2
  exit 1
fi
if [[ "$require_notarized" == true ]]; then
  if ! command -v xcrun >/dev/null 2>&1; then
    echo "xcrun is required to validate a stapled notarization ticket" >&2
    exit 1
  fi
  xcrun stapler validate "$app"
fi

echo "Bundle check passed: $app ($bundle_identifier, version $bundle_version)"
