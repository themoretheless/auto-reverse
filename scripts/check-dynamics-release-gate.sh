#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
manifest="$repo_root/packaging/dynamics-release-gate.toml"
gate_source="$repo_root/src/dynamics_gate.rs"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --manifest)
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--manifest needs a path" >&2
        exit 2
      fi
      manifest="$1"
      ;;
    --source)
      shift
      if [[ $# -eq 0 || -z "$1" ]]; then
        echo "--source needs a path" >&2
        exit 2
      fi
      gate_source="$1"
      ;;
    *)
      echo "usage: $0 [--manifest path] [--source path]" >&2
      exit 2
      ;;
  esac
  shift
done

if [[ ! -f "$manifest" ]]; then
  echo "dynamics release gate manifest is missing: $manifest" >&2
  exit 1
fi
if [[ ! -f "$gate_source" ]]; then
  echo "dynamics gate source is missing: $gate_source" >&2
  exit 1
fi

read_value() {
  local key="$1"
  local value
  value="$(sed -nE "s/^${key}[[:space:]]*=[[:space:]]*(.*)$/\\1/p" "$manifest")"
  if [[ -z "$value" ]]; then
    echo "dynamics release gate is missing $key" >&2
    exit 1
  fi
  printf '%s' "$value"
}

expect_exact() {
  local key="$1"
  local expected="$2"
  local actual
  actual="$(read_value "$key")"
  if [[ "$actual" != "$expected" ]]; then
    echo "dynamics release gate $key must be $expected, found $actual" >&2
    exit 1
  fi
}

expect_exact schema_version 1
expect_exact kill_switch_env '"AUTO_REVERSE_DISABLE_DYNAMICS"'
expect_exact config_rollback '"smooth_only"'

enabled="$(read_value enabled_by_default)"
if [[ "$enabled" != "true" && "$enabled" != "false" ]]; then
  echo "dynamics release gate enabled_by_default must be true or false" >&2
  exit 1
fi

source_enabled="$(sed -nE \
  's/^pub const DYNAMICS_ENABLED_BY_DEFAULT: bool = (true|false);$/\1/p' \
  "$gate_source")"
if [[ "$source_enabled" != "$enabled" ]]; then
  echo "dynamics source default ($source_enabled) differs from manifest ($enabled)" >&2
  exit 1
fi

source_integer() {
  local key="$1"
  local value
  value="$(sed -nE \
    "s/^pub const ${key}: [^=]+ = ([0-9_]+);$/\\1/p" \
    "$gate_source")"
  value="${value//_/}"
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    echo "could not read dynamics threshold $key from src/dynamics_gate.rs" >&2
    exit 1
  fi
  printf '%s' "$value"
}

min_physical_classes="$(source_integer MIN_PHYSICAL_CLASSES)"
min_sessions="$(source_integer MIN_COMPLETED_SESSIONS_PER_CLASS)"
max_movement_regression="$(source_integer MAX_P95_MOVEMENT_REGRESSION_BPS)"
max_scheduler_tail="$(source_integer MAX_SCHEDULER_TAIL_US)"
max_fail_open_violations="$(source_integer MAX_FAIL_OPEN_VIOLATIONS)"

integer_value() {
  local key="$1"
  local value
  value="$(read_value "$key")"
  if [[ ! "$value" =~ ^-?[0-9]+$ ]]; then
    echo "dynamics release gate $key must be an integer" >&2
    exit 1
  fi
  printf '%s' "$value"
}

physical_classes="$(integer_value physical_classes)"
sessions="$(integer_value min_completed_sessions_per_class)"
movement_regression="$(integer_value p95_movement_regression_bps)"
scheduler_tail="$(integer_value worst_scheduler_tail_us)"
fail_open_violations="$(integer_value fail_open_violations)"

if [[ "$physical_classes" -lt 0 || "$sessions" -lt 0 || "$scheduler_tail" -lt 0 || "$fail_open_violations" -lt 0 ]]; then
  echo "dynamics release evidence counts and latency must be non-negative" >&2
  exit 1
fi

if [[ "$enabled" == "true" ]]; then
  if [[ "$physical_classes" -lt "$min_physical_classes" ]]; then
    echo "dynamics default requires all $min_physical_classes physical classes" >&2
    exit 1
  fi
  if [[ "$sessions" -lt "$min_sessions" ]]; then
    echo "dynamics default requires at least $min_sessions completed sessions per class" >&2
    exit 1
  fi
  if [[ "$movement_regression" -gt "$max_movement_regression" ]]; then
    echo "dynamics p95 movement regression exceeds $max_movement_regression basis points" >&2
    exit 1
  fi
  if [[ "$scheduler_tail" -gt "$max_scheduler_tail" ]]; then
    echo "dynamics scheduler tail exceeds $max_scheduler_tail us" >&2
    exit 1
  fi
  if [[ "$fail_open_violations" -gt "$max_fail_open_violations" ]]; then
    echo "dynamics default allows at most $max_fail_open_violations fail-open violations" >&2
    exit 1
  fi
fi

echo "Dynamics release gate passed (enabled_by_default=$enabled)"
