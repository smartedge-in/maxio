# Deployment targets — bare metal and Kubernetes (P3-18+)

## Status

Draft — architectural requirement. Docker and a minimal K8s YAML snippet exist in `docs/operations.md`; neither bare metal nor Kubernetes is a first-class, tested deployment pack today.

## Requirement

MaxIO **must** support **two production deployment models** as equal citizens:

1. **Bare metal** (or VM) — native binary on Linux hosts with local or mounted block storage.
2. **Kubernetes** — plain YAML manifests (`deploy/k8s/`) with probes, PVCs, Ingress, and optional multi-tier layouts (P1-14). Helm chart optional later (P3-19).

Docker remains a packaging format; bare metal and K8s are the operator-facing targets.

## Deployment matrix

| Profile | Bare metal | Kubernetes |
|---------|------------|------------|
| **Single-node** (today) | systemd + one host, one `data_dir` | Deployment `replicas: 1` + PVC |
| **Distributed** (P3-13+) | systemd per tier or role-specific units on distinct hosts | StatefulSet (storage) + Deployment (server, UI) + Ingress split |
| **TLS / LB** | Caddy or Traefik (Apache-2.0 / MIT); no keepalived | Ingress + MetalLB or cert-manager (P3-26) |
| **Ops CLI** | `maxio-admin` on jump host | `maxio-admin` Job/CronJob or out-of-cluster |

## Bare metal (P3-18)

### Target operator flow

```
┌─────────────────────────────────────────┐
│  Linux host (VM or physical)            │
│  ├─ maxio binary (release artifact)     │
│  ├─ systemd: maxio.service              │
│  ├─ /var/lib/maxio  → MAXIO_DATA_DIR    │
│  ├─ local SSD / LVM / optional SAN      │
│  └─ Caddy TLS → 127.0.0.1:9000          │
└─────────────────────────────────────────┘
```

### Deliverables

| Item | Detail |
|------|--------|
| Install guide | `docs/operations.md` § Bare metal — download binary, user/group, directories, env file |
| systemd units | `deploy/systemd/maxio.service` (+ optional `maxio-ui`, `maxio-storage` for P3-13) |
| Upgrade / rollback | Stop → backup `data_dir` + keyring → replace binary → start; document |
| Firewall | Document ports 9000 (internal), 443 (proxy only) |
| Health | `systemd` `ExecStartPost` curl `/readyz` or `maxio healthcheck` |
| Distributed BM | Multi-host layout: storage nodes with dedicated disks, server nodes behind LB, UI behind CDN/nginx |

### Non-goals (v1)

- Windows or macOS server support
- Automated bare-metal cluster bootstrap (Terraform/Ansible optional later)

## Kubernetes (plain YAML — P1-24; Helm optional — P3-19)

### Target operator flow

```
kubectl apply -f deploy/k8s/single-node/
# or for distributed tiers (P1-14):
kubectl apply -f deploy/k8s/distributed/
```

### Manifest layout (proposed)

```
deploy/k8s/
  single-node/
    deployment.yaml
    service.yaml
    ingress.yaml
    pvc.yaml
    secret.yaml
  distributed/          # P1-14 tiers
    storage-statefulset.yaml
    server-deployment.yaml
    ui-deployment.yaml
    service.yaml
    ingress.yaml
    pdb.yaml
  cilium/               # P3-45 optional overlay
```

### Deliverables

| Item | Detail |
|------|--------|
| Plain manifests | `deploy/k8s/` — single-node and distributed profiles |
| Single-node profile | `replicas: 1`, RWO PVC, liveness `/healthz`, readiness `/readyz` |
| Ingress | TLS, body size unlimited for uploads, separate paths for `/ui` and `/api` when P1-21 lands |
| Secrets | `access-key` / `secret-key` via Secret; optional `MAXIO_ADMIN_TOKEN` |
| CI | `kubectl apply --dry-run=client` + kubeconform on manifests |
| Kind smoke test | P1-24 harness: `kind` bootstrap → PUT/GET via port-forward |
| Distributed profile | Documented YAML when P1-14 tiers ship |

### Future: Helm chart (P3-19)

Optional convenience wrapper around the same manifests — not required for P1-14 or GA.

### Kubernetes constraints (documented)

- **Today:** `replicas: 1` required — shared RWO PVC or independent data per pod is wrong for multi-replica Deployment.
- **P3-13+:** storage tier uses StatefulSet + per-pod PVC; server/UI use Deployment.

## Shared requirements (both targets)

- `/healthz` liveness, `/readyz` readiness
- `MAXIO_TRUSTED_PROXIES` when behind LB/Ingress
- Backup procedure for `data_dir` and `.maxio-keys.json`
- `maxio-admin` remote ops against API URL (not in-cluster requirement)

## Backlog

| ID | Scope |
|----|-------|
| **P3-18** | Bare metal deployment pack |
| **P3-19** | Kubernetes Helm chart (future improvement) |
| **P3-20** | Epic — first-class deployment targets (closes when P3-18 + plain K8s manifests done) |

## Acceptance (epic P3-20)

- [ ] Bare metal install documented and systemd units shipped
- [ ] Plain K8s manifests install single-node MaxIO on K8s 1.28+
- [ ] CI validates manifests (kubeconform or `kubectl apply --dry-run`)
- [ ] Both paths listed in README deployment section
- [ ] Helm chart (P3-19) optional follow-on