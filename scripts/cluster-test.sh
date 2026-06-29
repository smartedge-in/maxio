#!/usr/bin/env bash
# P1-24: multi-node cluster acceptance harness (in-process Raft + EC smoke).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> P1-24 cluster acceptance (maxio-cluster)"
cargo test -p maxio-cluster --features cluster-tests --test cluster_p14

echo "==> Storage Raft unit tests"
cargo test -p maxio-storage --features raft raft::

echo "==> maxio-ui binary smoke"
cargo build -p maxio-ui --release
"$ROOT/target/release/maxio-ui" --help >/dev/null

echo "==> Kubernetes manifest dry-run (distributed profile)"
if command -v kubectl >/dev/null 2>&1; then
  kubectl apply --dry-run=client -f deploy/k8s/distributed/
else
  echo "kubectl not installed; skipping manifest dry-run"
fi

echo "==> HTTP storage Raft multi-process smoke (production wiring)"
cargo test -p maxio-cluster --features http-raft-tests --test storage_raft_http

if command -v kubectl >/dev/null 2>&1; then
  echo "==> Kubernetes manifest dry-run (distributed profile)"
  kubectl apply --dry-run=client -f deploy/k8s/distributed/
fi

echo "P1-24 cluster harness: OK"