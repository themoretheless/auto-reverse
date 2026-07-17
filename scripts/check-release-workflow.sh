#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

bash -n \
  "$script_dir/build-app-bundle.sh" \
  "$script_dir/check-app-bundle.sh" \
  "$script_dir/check-dynamics-release-gate.sh" \
  "$script_dir/check-regression-matrix.sh" \
  "$script_dir/release-app-bundle.sh"

"$script_dir/check-dynamics-release-gate.sh"
"$script_dir/check-regression-matrix.sh"

"$script_dir/build-app-bundle.sh" --debug --ad-hoc
"$script_dir/check-app-bundle.sh" --debug --require-hardened-runtime

# An ad-hoc hardened build is useful for local QA but must never satisfy the
# production release gate.
if "$script_dir/check-app-bundle.sh" --debug --require-release-signature \
  >/dev/null 2>&1; then
  echo "ad-hoc signature incorrectly passed the Developer ID release gate" >&2
  exit 1
fi
if "$script_dir/check-app-bundle.sh" --debug --require-apple-development \
  >/dev/null 2>&1; then
  echo "ad-hoc signature incorrectly passed the Apple Development gate" >&2
  exit 1
fi
if "$script_dir/build-app-bundle.sh" --debug --ad-hoc \
  --development-sign-identity "Apple Development: Test (TEAMID)" \
  >/dev/null 2>&1; then
  echo "bundle builder accepted two signing modes" >&2
  exit 1
fi
if "$script_dir/check-app-bundle.sh" --debug --identity-only \
  --require-notarized >/dev/null 2>&1; then
  echo "identity-only mode bypassed strict notarization checks" >&2
  exit 1
fi

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/auto-reverse-release.XXXXXX")"
trap 'rm -rf "$tmp_root"' EXIT

awk 'BEGIN { FS = OFS = "|" }
     $2 ~ /PR-0[1-6]/ { $5 = " 26.6 / test app "; $6 = " Pass "; $7 = " smoke / CI " }
     { print }' "$script_dir/../QA.md" > "$tmp_root/complete-regression-matrix.md"
"$script_dir/check-regression-matrix.sh" \
  --matrix "$tmp_root/complete-regression-matrix.md" --require-results >/dev/null

awk 'BEGIN { FS = OFS = "|" }
     $2 ~ /PR-01/ { $5 = " "; $6 = " "; $7 = " " }
     { print }' "$tmp_root/complete-regression-matrix.md" \
  > "$tmp_root/incomplete-regression-matrix.md"
if "$script_dir/check-regression-matrix.sh" \
  --matrix "$tmp_root/incomplete-regression-matrix.md" --require-results \
  >/dev/null 2>&1; then
  echo "incomplete platform regression evidence passed the release gate" >&2
  exit 1
fi

# Flipping only the default is never enough: incomplete physical evidence must
# fail before a production release can begin.
sed 's/enabled_by_default = false/enabled_by_default = true/' \
  "$script_dir/../packaging/dynamics-release-gate.toml" \
  > "$tmp_root/incomplete-dynamics-gate.toml"
sed 's/DYNAMICS_ENABLED_BY_DEFAULT: bool = false/DYNAMICS_ENABLED_BY_DEFAULT: bool = true/' \
  "$script_dir/../src/dynamics_gate.rs" \
  > "$tmp_root/default-on-dynamics-gate.rs"
if "$script_dir/check-dynamics-release-gate.sh" \
  --manifest "$tmp_root/incomplete-dynamics-gate.toml" \
  --source "$tmp_root/default-on-dynamics-gate.rs" >/dev/null 2>&1; then
  echo "incomplete dynamics evidence incorrectly passed the release gate" >&2
  exit 1
fi

sed -e 's/physical_classes = 0/physical_classes = 6/' \
    -e 's/min_completed_sessions_per_class = 0/min_completed_sessions_per_class = 30/' \
  "$tmp_root/incomplete-dynamics-gate.toml" \
  > "$tmp_root/accepted-dynamics-gate.toml"
"$script_dir/check-dynamics-release-gate.sh" \
  --manifest "$tmp_root/accepted-dynamics-gate.toml" \
  --source "$tmp_root/default-on-dynamics-gate.rs" >/dev/null

plan="$("$script_dir/release-app-bundle.sh" \
  --sign-identity "Developer ID Application: Auto Reverse Test (TEAMID)" \
  --keychain-profile auto-reverse-notary-test \
  --output-dir "$tmp_root/dist" \
  --plan)"

for expected in \
  "Developer ID Application" \
  "Dynamics gate: enabled_by_default=false" \
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
