#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/app-bundle.sh"

usage() {
  cat <<'USAGE'
Usage:
  scripts/uninstall-app-bundle.sh [options]

Options:
  --destination PATH       Installed app (default: /Applications/Auto Reverse.app)
  --remove-user-data       Also remove the default config/locks and local log
  --skip-startup-cleanup   Testing only: do not touch login registrations/config
  -h, --help               Show this help

By default the app and both startup registrations are removed, while settings
remain in ~/Library/Application Support/Auto Reverse for a future reinstall.
USAGE
}

destination="/Applications/$AUTO_REVERSE_APP_BASENAME"
remove_user_data=false
skip_startup_cleanup=false

remove_cli_launch_agent_fallback() {
  launchctl bootout "gui/$(id -u)/com.auto-reverse.agent" >/dev/null 2>&1 || true
  rm -f "$HOME/Library/LaunchAgents/com.auto-reverse.agent.plist"
  echo "Removed the CLI LaunchAgent fallback if it existed."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --destination)
      shift
      if [[ $# -eq 0 ]]; then
        echo "--destination needs a path" >&2
        exit 2
      fi
      destination="$1"
      ;;
    --remove-user-data)
      remove_user_data=true
      ;;
    --skip-startup-cleanup)
      skip_startup_cleanup=true
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

if [[ "$destination" != /* ]]; then
  destination="$PWD/$destination"
fi
if [[ "$(basename -- "$destination")" != "$AUTO_REVERSE_APP_BASENAME" ]]; then
  echo "destination must end with /$AUTO_REVERSE_APP_BASENAME: $destination" >&2
  exit 2
fi
if [[ -L "$destination" ]]; then
  echo "refusing to remove a symlink destination: $destination" >&2
  exit 1
fi

if [[ -e "$destination" ]]; then
  "$script_dir/check-app-bundle.sh" --identity-only --app "$destination"
  executable="$(auto_reverse_bundle_executable "$destination")"

  if [[ "$skip_startup_cleanup" == false ]]; then
    help_output=""
    if [[ -x "$executable" ]]; then
      help_output="$("$executable" help 2>/dev/null || true)"
    fi
    if [[ "$help_output" == *"prepare-uninstall"* ]]; then
      (
        unset AUTO_REVERSE_CONFIG XDG_CONFIG_HOME
        "$executable" prepare-uninstall
      )
    else
      echo "warning: this legacy/damaged bundle cannot unregister its GUI login item automatically; check System Settings > General > Login Items" >&2
      remove_cli_launch_agent_fallback
    fi
  fi

  auto_reverse_stop_bundle_processes "$destination"
  rm -rf "$destination"
  echo "Removed app: $destination"
else
  echo "App is not installed at $destination"
  if [[ "$skip_startup_cleanup" == false ]]; then
    echo "warning: GUI login-item cleanup requires the installed bundle; check System Settings > General > Login Items" >&2
    remove_cli_launch_agent_fallback
  fi
fi

if [[ "$remove_user_data" == true ]]; then
  if [[ -z "${HOME:-}" || "$HOME" != /* || "$HOME" == "/" ]]; then
    echo "refusing to remove user data with an unsafe HOME: ${HOME:-<unset>}" >&2
    exit 1
  fi
  expected_data_parent="$HOME/Library/Application Support"
  expected_data_dir="$expected_data_parent/Auto Reverse"
  expected_log="$HOME/Library/Logs/auto-reverse.log"

  if [[ "$(dirname -- "$expected_data_dir")" != "$expected_data_parent" ]] || \
     [[ "$(basename -- "$expected_data_dir")" != "Auto Reverse" ]]; then
    echo "refusing unexpected user-data path: $expected_data_dir" >&2
    exit 1
  fi
  rm -rf "$expected_data_dir"
  rm -f "$expected_log"
  echo "Removed user data: $expected_data_dir"
  echo "Removed local log if present: $expected_log"
else
  echo "Settings retained at: $HOME/Library/Application Support/Auto Reverse/config.toml"
fi
