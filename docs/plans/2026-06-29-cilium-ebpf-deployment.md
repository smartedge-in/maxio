# Cilium eBPF deployment for MaxIO (P3-45+)

## Status

Draft — backlog P3-45, P3-46. Uses plain K8s manifests under `deploy/k8s/` (Helm optional later — P3-19).

## Goal

Run MaxIO on Kubernetes with **Cilium** as the CNI and use **eBPF** where it improves throughput and operability, while keeping **storage correctness** in MaxIO (replication P3-09–12 or distributed tiers P3-13–16).

## What eBPF helps

| Path | Cilium feature | Benefit |
|------|----------------|---------|
| Clients → MaxIO Service | kube-proxy replacement, socket LB | Lower latency vs iptables; better scale at high connection counts |
| Ingress / Gateway API | Cilium Ingress / Gateway | TLS termination with eBPF datapath; document streaming-friendly settings |
| Server ↔ storage RPC (P3-13+) | Host routing, Bandwidth Manager | Efficient pod-to-pod for Raft and object proxy paths |
| Replication agent (P3-11) | WireGuard encryption, Hubble | Encrypted cross-node replication traffic; flow debugging |
| Large multipart uploads | Big TCP (where enabled) | Marginal TCP improvements; disk remains primary bottleneck |

## What eBPF does not do

- Share one `data_dir` across multiple MaxIO pods
- Replace bucket replication or Raft consensus
- Make `Deployment replicas: 2` safe on a single RWO PVC

## Deployment patterns

### Pattern A — Single-node (today)

```
S3 clients → Cilium Gateway/Ingress (TLS) → Service → maxio Pod (replicas: 1) + PVC
```

### Pattern B — Active-passive (P3-09–12)

```
S3 clients → Gateway → Service maxio-primary (Endpoints: primary only)
                              │
              primary Pod ────┼──── standby Pod (no client Endpoints)
                              │
                    maxio-replicate / rsync traffic (internal Service, Cilium mesh)
```

Failover: update Endpoints or Service selector to standby; document in operations guide.

### Pattern C — Distributed tiers (P3-13–16)

```
S3 clients → Gateway → Service → maxio-server Deployment (N replicas, eBPF LB)
                                      │
                                      ▼
                               maxio-storage StatefulSet (M replicas, Raft)
```

UI tier (`maxio-ui`, P3-16) is a separate Deployment behind its own Service.

## Plain K8s manifests (proposed)

`deploy/k8s/cilium/` (or annotations on existing manifests):

- Document required Cilium cluster settings (`kubeProxyReplacement`, socket LB)
- `MAXIO_TRUSTED_PROXIES`: cluster Pod CIDR + ingress CIDR
- `MAXIO_LOGIN_RATE_LIMIT_REDIS_URL` when server replicas > 1
- `service.publishNotReadyAddresses: false` for primary-only pattern
- Optional: Cilium NetworkPolicy manifests for replication agent

Helm overlay (`values-cilium.yaml`) may follow later under P3-19 — not required for P3-45.

## Observability

- MaxIO: `MAXIO_METRICS_ENABLED`, `/metrics` (P2-07)
- Cilium: Hubble UI/CLI for drops, latency, policy verdicts
- Compare ingress vs in-cluster baseline before tuning eBPF

## Related backlog

| ID | Relationship |
|----|--------------|
| P3-19 | Optional Helm chart (future) |
| P3-45 | This document + plain K8s Cilium examples |
| P3-46 | Service topology for primary/standby/replication |
| P3-09–12 | Replication correctness |
| P3-13–16 | Server/storage scale-out |
| P3-26 | Permissive ingress (Cilium Gateway fits policy) |

## Acceptance (P3-45)

- [ ] Plan published (this file) and linked from `docs/BACKLOG.md`
- [ ] Cilium-oriented manifests or annotations under `deploy/k8s/`
- [ ] `docs/operations.md` § Kubernetes + Cilium quickstart
- [ ] Primary-only and future server-LB patterns documented
- [ ] Trusted proxy and Redis login-limit notes for multi-replica server tier