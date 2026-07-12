#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

bash -n \
  "$script_dir/build-app-bundle.sh" \
  "$script_dir/check-app-bundle.sh" \
  "$script_dir/release-app-bundle.sh"

"$script_dir/build-app-bundle.sh" --debug --ad-hoc
"$script_dir/check-app-bundle.sh" --debug --require-hardened-runtime

# An ad-hoc hardened build is useful for local QA but must never satisfy the
# production release gate.
if "$script_dir/check-app-bundle.sh" --debug --require-release-signature \
  >/dev/null 2>&1; then
  echo "ad-hoc signature incorrectly passed the Developer ID release gate" >&2
  exit 1
fi
if "$script_dir/check-app-bundle.sh" --debug --identity-only \
  --require-notarized >/dev/null 2>&1; then
  echo "identity-only mode bypassed strict notarization checks" >&2
  exit 1
fi

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/auto-reverse-release.XXXXXX")"
trap 'rm -rf "$tmp_root"' EXIT
plan="$("$script_dir/release-app-bundle.sh" \
  --sign-identity "Developer ID Application: Auto Reverse Test (TEAMID)" \
  --keychain-profile auto-reverse-notary-test \
  --output-dir "$tmp_root/dist" \
  --plan)"

for expected in \
  "Developer ID Application" \
  "notarytool submit --wait" \
  "stapler staple + validate" \
  "Gatekeeper assessment" \
  "no files changed"; do
  if [[ "$plan" != *"$expected"* ]]; then
    echo "release plan is missing: $expected" >&2
    exit 1
  fi
done
if [[ -e "$tmp_root/dist" ]]; then
  echo "--plan created its output directory" >&2
  exit 1
fi

echo "Release signing/notarization workflow check passed"
