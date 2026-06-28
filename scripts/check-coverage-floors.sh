#!/usr/bin/env bash
set -euo pipefail

# Usage: check-coverage-floors.sh <summary-file>
# Parses `cargo llvm-cov --summary-only` output and enforces line-coverage floors.

summary="${1:?usage: check-coverage-floors.sh <summary-file>}"

line_cover() {
  local file="$1"
  # Summary columns: ... Lines Missed-Lines Line-Cover% Branches ...
  awk -v f="$file" '$1 == f { gsub(/%/, "", $10); print $10; exit }' "$summary"
}

require_min() {
  local file="$1"
  local min="$2"
  local actual
  actual="$(line_cover "$file")"
  if [[ -z "$actual" ]]; then
    echo "coverage floor check: missing entry for $file" >&2
    exit 1
  fi
  awk -v a="$actual" -v m="$min" 'BEGIN { if (a+0 < m+0) exit 1 }' || {
    echo "coverage floor failed: $file ${actual}% < ${min}% lines" >&2
    exit 1
  }
  echo "coverage floor ok: $file ${actual}% >= ${min}% lines"
}

require_min storage/crypto.rs 80
require_min auth/signature_v4.rs 25
require_min api/virtual_host.rs 80
require_min auth/credentials.rs 80
require_min storage/policy.rs 80