#!/usr/bin/env bash
# P3-06: Playwright console smoke (login → bucket → upload → download → delete).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PORT="${MAXIO_E2E_PORT:-19010}"
DATA_DIR="$(mktemp -d)"
export MAXIO_E2E_BASE_URL="http://127.0.0.1:${PORT}"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$DATA_DIR"
}
trap cleanup EXIT

if [[ ! -x target/debug/maxio ]]; then
  cargo build -p maxio
fi

./target/debug/maxio \
  --data-dir "$DATA_DIR" \
  --port "$PORT" \
  --address 127.0.0.1 \
  --allow-insecure-dev &
SERVER_PID=$!

for _ in $(seq 1 30); do
  if curl -sf "${MAXIO_E2E_BASE_URL}/healthz" >/dev/null; then
    break
  fi
  sleep 1
done
curl -sf "${MAXIO_E2E_BASE_URL}/healthz" >/dev/null

command -v bun >/dev/null 2>&1 || { echo "error: bun required for e2e" >&2; exit 1; }
cd ui && bun install --frozen-lockfile && bun run build
cd "$ROOT/e2e"
bun install --frozen-lockfile
bunx playwright install chromium --with-deps 2>/dev/null || bunx playwright install chromium
MAXIO_E2E_BASE_URL="$MAXIO_E2E_BASE_URL" bun run test

echo "P3-06 console e2e: OK"