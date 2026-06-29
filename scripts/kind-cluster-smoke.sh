#!/usr/bin/env bash
# Production cluster wiring smoke: 3-process HTTP storage Raft on localhost.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> HTTP storage Raft integration (multi-process)"
cargo test -p maxio-cluster --features http-raft-tests --test storage_raft_http

if command -v kind >/dev/null 2>&1 && command -v kubectl >/dev/null 2>&1; then
  echo "==> kind + kubectl detected — dry-run distributed manifests"
  kubectl apply --dry-run=client -f deploy/k8s/distributed/
else
  echo "kind/kubectl not installed; skipping K8s dry-run"
fi

echo "Production cluster wiring smoke: OK"