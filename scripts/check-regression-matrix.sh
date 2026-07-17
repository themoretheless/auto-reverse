#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
matrix="$repo_root/QA.md"
require_results=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --matrix)
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--matrix needs a path" >&2
        exit 2
      fi
      matrix="$1"
      ;;
    --require-results)
      require_results=true
      ;;
    *)
      echo "usage: $0 [--matrix path] [--require-results]" >&2
      exit 2
      ;;
  esac
  shift
done

if [[ ! -f "$matrix" ]]; then
  echo "platform regression matrix is missing: $matrix" >&2
  exit 1
fi

rows=(
  "PR-01|Safari zoom"
  "PR-02|Launchpad"
  "PR-03|Catalyst / iOS app"
  "PR-04|Universal Control"
  "PR-05|iPhone Mirroring"
  "PR-06|Remote desktop"
)

for expected in "${rows[@]}"; do
  id="${expected%%|*}"
  scenario="${expected#*|}"
  count="$(grep -Fc "| $id | $scenario |" "$matrix" || true)"
  if [[ "$count" -ne 1 ]]; then
    echo "QA platform regression matrix must contain exactly one $id / $scenario row" >&2
    exit 1
  fi

  if [[ "$require_results" == true ]]; then
    row="$(grep -F "| $id | $scenario |" "$matrix")"
    if ! awk -F '|' '
      function trim(value) {
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
        return value
      }
      {
        build = trim($5)
        result = trim($6)
        evidence = trim($7)
        exit !(build != "" && result == "Pass" && evidence != "")
      }
    ' <<< "$row"; then
      echo "$id requires macOS/app build, Result=Pass, and evidence/tester" >&2
      exit 1
    fi
  fi
done

if [[ "$require_results" == true ]]; then
  echo "Platform regression matrix release results passed"
else
  echo "Platform regression matrix structure check passed"
fi
