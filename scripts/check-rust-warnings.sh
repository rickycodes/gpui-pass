#!/usr/bin/env bash
# Check compiler warnings across the full dependency graph, including Cargo's
# future-incompatibility summary for registry dependencies.
set -euo pipefail

check_log=$(mktemp)
trap 'rm -f "$check_log"' EXIT

cargo check --all-targets --locked --future-incompat-report --color never "$@" \
  2>&1 | tee "$check_log"

if grep -q '^warning:' "$check_log"; then
  echo "Rust check emitted warnings; fix them before merging." >&2
  exit 1
fi
