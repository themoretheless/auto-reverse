#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/lib/app-bundle.sh"

usage() {
  cat <<'USAGE'
Usage:
  scripts/release-app-bundle.sh \
    --sign-identity "Developer ID Application: Name (TEAMID)" \
    --keychain-profile auto-reverse-notary \
    [--output-dir target/dist] [--plan]

Environment alternatives:
  AUTO_REVERSE_SIGN_IDENTITY
  AUTO_REVERSE_NOTARY_PROFILE
  AUTO_REVERSE_DIST_DIR

The real workflow builds a release app, signs it with hardened runtime and a
secure timestamp, verifies Developer ID authority, submits a ZIP with
notarytool, requires Accepted status, downloads the audit log, staples and
validates the ticket, runs Gatekeeper assessment, and creates a final stapled
ZIP plus SHA-256 file. Credentials are read only from a notarytool Keychain
profile; this script never accepts an Apple ID password.

--plan validates arguments and prints the side-effect-free release sequence.
USAGE
}

sign_identity="${AUTO_REVERSE_SIGN_IDENTITY:-}"
notary_profile="${AUTO_REVERSE_NOTARY_PROFILE:-}"
dist_dir="${AUTO_REVERSE_DIST_DIR:-target/dist}"
plan_only=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign-identity)
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--sign-identity needs a non-empty identity" >&2
        exit 2
      fi
      sign_identity="$1"
      ;;
    --keychain-profile)
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--keychain-profile needs a non-empty profile" >&2
        exit 2
      fi
      notary_profile="$1"
      ;;
    --output-dir)
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--output-dir needs a path" >&2
        exit 2
      fi
      dist_dir="$1"
      ;;
    --plan)
      plan_only=true
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

if [[ -z "$sign_identity" ]]; then
  echo "a Developer ID Application identity is required" >&2
  echo "pass --sign-identity or AUTO_REVERSE_SIGN_IDENTITY" >&2
  exit 2
fi
if [[ -z "$notary_profile" ]]; then
  echo "a notarytool Keychain profile is required" >&2
  echo "pass --keychain-profile or AUTO_REVERSE_NOTARY_PROFILE" >&2
  exit 2
fi
if [[ "$dist_dir" != /* ]]; then
  dist_dir="$repo_root/$dist_dir"
fi

version="$(sed -nE 's/^version = "([^"]+)"/\1/p' "$repo_root/Cargo.toml" | head -n 1)"
if [[ -z "$version" ]]; then
  echo "could not read package version from Cargo.toml" >&2
  exit 1
fi

app="$repo_root/target/release/$AUTO_REVERSE_APP_BASENAME"
artifact_stem="Auto-Reverse-$version-macOS"
submission_archive="$dist_dir/$artifact_stem.notarization.$$.zip"
notary_result="$dist_dir/$artifact_stem.notary.plist"
notary_log="$dist_dir/$artifact_stem.notary.json"
final_archive="$dist_dir/$artifact_stem.zip"
checksum_file="$final_archive.sha256"
staged_archive="$final_archive.tmp.$$"
staged_checksum="$checksum_file.tmp.$$"

print_plan() {
  cat <<PLAN
Auto Reverse production release plan
  App: $app
  Signing identity: $sign_identity
  Notary Keychain profile: $notary_profile
  Output: $final_archive

1. Build release Mach-O and sign the app with Developer ID, hardened runtime,
   least-privilege entitlements, and a secure timestamp.
2. Strictly verify the signature before upload.
3. Create a ditto ZIP and run notarytool submit --wait.
4. Require Accepted status and download the notarization log.
5. Run stapler staple + validate and Gatekeeper assessment.
6. Rebuild the final ZIP from the stapled app and write its SHA-256.

Plan only: no files changed and no Apple service was contacted.
PLAN
}

if [[ "$plan_only" == true ]]; then
  print_plan
  exit 0
fi

for tool in codesign ditto plutil shasum spctl xcrun; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required release tool is missing: $tool" >&2
    exit 1
  fi
done
xcrun --find notarytool >/dev/null
xcrun --find stapler >/dev/null

cd "$repo_root"
"$script_dir/build-app-bundle.sh" --release --sign-identity "$sign_identity"
"$script_dir/check-app-bundle.sh" --app "$app" --require-release-signature

mkdir -p "$dist_dir"
cleanup_staged_artifacts() {
  rm -f "$submission_archive" "$staged_archive" "$staged_checksum"
}
trap cleanup_staged_artifacts EXIT
rm -f "$submission_archive" "$notary_result" "$notary_log" \
  "$staged_archive" "$staged_checksum"
ditto -c -k --keepParent "$app" "$submission_archive"

submission_succeeded=true
if ! xcrun notarytool submit "$submission_archive" \
  --keychain-profile "$notary_profile" \
  --wait \
  --timeout 1h \
  --no-progress \
  --output-format plist > "$notary_result"; then
  submission_succeeded=false
fi

notary_status=""
submission_id=""
if [[ -s "$notary_result" ]] && plutil -lint "$notary_result" >/dev/null 2>&1; then
  notary_status="$(plutil -extract status raw "$notary_result" 2>/dev/null || true)"
  submission_id="$(plutil -extract id raw "$notary_result" 2>/dev/null || true)"
fi

download_notary_log() {
  if [[ -z "$submission_id" ]]; then
    return 0
  fi
  if ! xcrun notarytool log "$submission_id" "$notary_log" \
    --keychain-profile "$notary_profile"; then
    echo "warning: could not download notarization log for $submission_id" >&2
  fi
}

if [[ "$submission_succeeded" != true || "$notary_status" != "Accepted" ]]; then
  download_notary_log
  echo "notarization was not accepted (status: ${notary_status:-unavailable})" >&2
  echo "result: $notary_result" >&2
  if [[ -f "$notary_log" ]]; then
    echo "log: $notary_log" >&2
  fi
  exit 1
fi

download_notary_log
xcrun stapler staple "$app"
"$script_dir/check-app-bundle.sh" --app "$app" --require-notarized
spctl --assess --type execute --verbose=4 "$app"

ditto -c -k --keepParent "$app" "$staged_archive"
checksum="$(shasum -a 256 "$staged_archive" | awk '{print $1}')"
printf '%s  %s\n' "$checksum" "$(basename -- "$final_archive")" > "$staged_checksum"
mv -f "$staged_archive" "$final_archive"
mv -f "$staged_checksum" "$checksum_file"
rm -f "$submission_archive"

echo "Production release completed"
echo "  App: $app"
echo "  Archive: $final_archive"
echo "  Checksum: $checksum_file"
echo "  Notary result: $notary_result"
if [[ -f "$notary_log" ]]; then
  echo "  Notary log: $notary_log"
fi
