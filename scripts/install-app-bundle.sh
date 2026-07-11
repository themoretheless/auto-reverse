#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/lib/app-bundle.sh"

usage() {
  cat <<'USAGE'
Usage:
  scripts/install-app-bundle.sh [options]

Options:
  --release              Build/install release profile (default)
  --debug                Build/install debug profile
  --destination PATH     Install path (default: /Applications/Auto Reverse.app)
  --no-build             Use an already-built target/<profile> bundle
  --no-open              Do not launch the installed app
  -h, --help             Show this help

The destination must be named "Auto Reverse.app". Updates are staged beside
the destination, checked, swapped atomically on the same volume, and rolled
back if the installed bundle fails validation.
USAGE
}

profile="release"
destination="/Applications/$AUTO_REVERSE_APP_BASENAME"
build_bundle=true
open_after_install=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      profile="release"
      ;;
    --debug)
      profile="debug"
      ;;
    --destination)
      shift
      if [[ $# -eq 0 ]]; then
        echo "--destination needs a path" >&2
        exit 2
      fi
      destination="$1"
      ;;
    --no-build)
      build_bundle=false
      ;;
    --no-open)
      open_after_install=false
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
  echo "refusing to replace a symlink destination: $destination" >&2
  exit 1
fi

source_app="$repo_root/target/$profile/$AUTO_REVERSE_APP_BASENAME"
if [[ "$build_bundle" == true ]]; then
  "$script_dir/build-app-bundle.sh" "--$profile"
fi
"$script_dir/check-app-bundle.sh" --app "$source_app"

destination_parent="$(dirname -- "$destination")"
if ! mkdir -p "$destination_parent"; then
  echo "cannot create install directory: $destination_parent" >&2
  echo "choose a writable --destination or run from an account allowed to write /Applications" >&2
  exit 1
fi

if [[ -e "$destination" ]]; then
  "$script_dir/check-app-bundle.sh" --identity-only --app "$destination"
  auto_reverse_stop_bundle_processes "$destination"
fi

stage="$destination_parent/.${AUTO_REVERSE_APP_BASENAME}.install.$$"
backup="$destination_parent/.${AUTO_REVERSE_APP_BASENAME}.backup.$$"
if [[ -e "$stage" || -e "$backup" ]]; then
  echo "refusing unexpected existing install transaction path: $stage or $backup" >&2
  exit 1
fi
install_committed=false
destination_written=false

cleanup() {
  if [[ -e "$stage" ]]; then
    rm -rf "$stage"
  fi
  if [[ "$install_committed" == false && "$destination_written" == true ]]; then
    rm -rf "$destination"
  fi
  if [[ -e "$backup" ]]; then
    if [[ "$install_committed" == true ]]; then
      rm -rf "$backup"
    else
      echo "install failed; restoring previous bundle" >&2
      mv "$backup" "$destination"
    fi
  fi
}
trap cleanup EXIT

ditto "$source_app" "$stage"
"$script_dir/check-app-bundle.sh" --app "$stage"

if [[ -e "$destination" ]]; then
  mv "$destination" "$backup"
fi
if ! mv "$stage" "$destination"; then
  echo "could not move staged bundle into place: $destination" >&2
  exit 1
fi
destination_written=true
if ! "$script_dir/check-app-bundle.sh" --app "$destination"; then
  echo "installed bundle failed validation: $destination" >&2
  exit 1
fi

install_committed=true
if [[ -e "$backup" ]]; then
  rm -rf "$backup"
fi
trap - EXIT

version="$(plutil -extract CFBundleShortVersionString raw "$destination/Contents/Info.plist")"
echo "Installed Auto Reverse $version at $destination"
echo "Bundle identifier: $AUTO_REVERSE_BUNDLE_IDENTIFIER"
echo "Note: this local build is ad-hoc signed; stable public TCC identity still requires Developer ID signing."

if [[ "$open_after_install" == true ]]; then
  open -n "$destination"
  executable="$(auto_reverse_bundle_executable "$destination")"
  count=0
  launched=false
  while (( count < 50 )); do
    if [[ -n "$(auto_reverse_matching_pids "$executable")" ]]; then
      launched=true
      break
    fi
    sleep 0.1
    ((count += 1))
  done
  if [[ "$launched" == true ]]; then
    sleep 1
    if [[ -n "$(auto_reverse_matching_pids "$executable")" ]]; then
      echo "Launched $destination"
      exit 0
    fi
  fi
  echo "installed successfully, but the app did not remain running: $destination" >&2
  exit 1
fi
