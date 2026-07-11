#!/usr/bin/env bash

# Shared bundle identity and process helpers. Callers own `set -euo pipefail`.
AUTO_REVERSE_APP_BASENAME="Auto Reverse.app"
AUTO_REVERSE_BUNDLE_IDENTIFIER="com.auto-reverse.app"
AUTO_REVERSE_EXECUTABLE_NAME="auto-reverse"

auto_reverse_bundle_executable() {
  printf '%s/Contents/MacOS/%s\n' "$1" "$AUTO_REVERSE_EXECUTABLE_NAME"
}

# Print PIDs whose command begins with this exact executable path. Matching
# the full path avoids terminating a development build or unrelated process
# that merely happens to contain "auto-reverse" in its arguments.
auto_reverse_matching_pids() {
  local executable="$1"
  ps -axo pid=,command= | awk -v executable="$executable" '
    {
      pid = $1
      $1 = ""
      sub(/^[[:space:]]+/, "", $0)
      if ($0 == executable || index($0, executable " ") == 1) {
        print pid
      }
    }
  '
}

auto_reverse_wait_until_stopped() {
  local executable="$1"
  local attempts="${2:-50}"
  local remaining
  local count=0

  while (( count < attempts )); do
    remaining="$(auto_reverse_matching_pids "$executable")"
    if [[ -z "$remaining" ]]; then
      return 0
    fi
    sleep 0.1
    ((count += 1))
  done
  return 1
}

auto_reverse_stop_bundle_processes() {
  local app="$1"
  local executable
  local pids
  local pid

  executable="$(auto_reverse_bundle_executable "$app")"
  pids="$(auto_reverse_matching_pids "$executable")"
  if [[ -z "$pids" ]]; then
    return 0
  fi

  echo "Stopping running Auto Reverse process(es): ${pids//$'\n'/ }"
  for pid in $pids; do
    kill -TERM "$pid" 2>/dev/null || true
  done

  if auto_reverse_wait_until_stopped "$executable" 50; then
    return 0
  fi

  pids="$(auto_reverse_matching_pids "$executable")"
  echo "Auto Reverse did not stop after 5 seconds; forcing exact PID(s): ${pids//$'\n'/ }" >&2
  for pid in $pids; do
    kill -KILL "$pid" 2>/dev/null || true
  done

  if ! auto_reverse_wait_until_stopped "$executable" 20; then
    echo "could not stop the process at $executable" >&2
    return 1
  fi
}
