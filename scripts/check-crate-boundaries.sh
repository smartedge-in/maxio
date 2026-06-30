#!/usr/bin/env bash
# P3-23: Enforce workspace crate dependency boundaries.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

check_forbidden_dep() {
  local crate="$1"
  local forbidden="$2"
  local manifest="$ROOT/crates/$crate/Cargo.toml"
  if [[ ! -f "$manifest" ]]; then
    echo "error: missing manifest for $crate ($manifest)" >&2
    fail=1
    return
  fi
  if grep -Eq "^[[:space:]]*${forbidden}[[:space:]]*=" "$manifest"; then
    echo "error: $crate must not depend on $forbidden (see docs/plans/2026-06-29-shared-libraries.md)" >&2
    fail=1
  fi
}

echo "==> Checking crate dependency boundaries"

check_forbidden_dep maxio-admin maxio
check_forbidden_dep maxio-admin maxio-server
check_forbidden_dep maxio-common axum
check_forbidden_dep maxio-common reqwest
check_forbidden_dep maxio-common maxio-storage

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "Crate boundary checks passed."