#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/app-bundle.sh"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/auto-reverse-install.XXXXXX")"
destination="$tmp_root/$AUTO_REVERSE_APP_BASENAME"
fixture_pid=""

cleanup() {
  if [[ -n "$fixture_pid" ]] && kill -0 "$fixture_pid" 2>/dev/null; then
    kill -KILL "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  rm -rf "$tmp_root"
}
trap cleanup EXIT

if "$script_dir/install-app-bundle.sh" \
  --debug \
  --no-build \
  --no-open \
  --development-sign-identity "Apple Development: Test (TEAMID)" \
  --destination "$destination" >/dev/null 2>&1; then
  echo "installer accepted a signing request without a build" >&2
  exit 1
fi

"$script_dir/install-app-bundle.sh" \
  --debug \
  --no-build \
  --no-open \
  --destination "$destination"
"$script_dir/check-app-bundle.sh" --app "$destination"

# Prove an update replaces rather than merges bundle contents.
touch "$destination/Contents/Resources/stale-install-sentinel"
"$script_dir/install-app-bundle.sh" \
  --debug \
  --no-build \
  --no-open \
  --destination "$destination"
if [[ -e "$destination/Contents/Resources/stale-install-sentinel" ]]; then
  echo "update merged bundle contents instead of replacing them" >&2
  exit 1
fi

# Exercise exact-path process discovery with spaces without touching any real
# Auto Reverse process or requiring TCC permissions.
fixture_app="$tmp_root/Process Fixture/$AUTO_REVERSE_APP_BASENAME"
fixture_executable="$(auto_reverse_bundle_executable "$fixture_app")"
mkdir -p "$(dirname -- "$fixture_executable")"
ln -s /bin/sleep "$fixture_executable"
"$fixture_executable" 30 &
fixture_pid=$!
sleep 0.1
if [[ "$(auto_reverse_matching_pids "$fixture_executable")" != *"$fixture_pid"* ]]; then
  echo "exact-path process lookup did not find fixture PID $fixture_pid" >&2
  exit 1
fi
auto_reverse_stop_bundle_processes "$fixture_app"
wait "$fixture_pid" 2>/dev/null || true
if [[ -n "$(auto_reverse_matching_pids "$fixture_executable")" ]]; then
  echo "exact-path process stop left fixture PID running: $fixture_pid" >&2
  exit 1
fi

# A damaged installation with a missing executable must still be removable by
# exact bundle identity (startup cleanup is intentionally skipped in this
# isolated test).
rm "$destination/Contents/MacOS/$AUTO_REVERSE_EXECUTABLE_NAME"
"$script_dir/uninstall-app-bundle.sh" \
  --skip-startup-cleanup \
  --destination "$destination"
if [[ -e "$destination" ]]; then
  echo "uninstall left the app bundle behind: $destination" >&2
  exit 1
fi

echo "Install/update/uninstall workflow check passed"
