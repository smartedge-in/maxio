# Deployment targets — bare metal and Kubernetes (P3-18+)

## Status

Draft — architectural requirement. Docker and a minimal K8s YAML snippet exist in `docs/operations.md`; neither bare metal nor Kubernetes is a first-class, tested deployment pack today.

## Requirement

MaxIO **must** support **two production deployment models** as equal citizens:

1. **Bare metal** (or VM) — native binary on Linux hosts with local or mounted block storage.
2. **Kubernetes** — Helm-based install with probes, PVCs, Ingress, and optional multi-tier layouts (P3-13+).

Docker remains a packaging format; bare metal and K8s are the operator-facing targets.

## Deployment matrix

| Profile | Bare metal | Kubernetes |
|---------|------------|------------|
| **Single-node** (today) | systemd + one host, one `data_dir` | Deployment `replicas: 1` + PVC |
| **Distributed** (P3-13+) | systemd per tier or role-specific units on distinct hosts | StatefulSet (storage) + Deployment (server, UI) + Ingress split |
| **TLS** | nginx/Caddy on host | Ingress + cert-manager |
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
│  └─ nginx/Caddy TLS → 127.0.0.1:9000    │
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

## Kubernetes (P3-19)

### Target operator flow

```
helm install maxio ./charts/maxio \
  --set credentials.existingSecret=maxio-credentials \
  --set persistence.size=500Gi
```

### Chart structure (proposed)

```
deploy/helm/maxio/
  Chart.yaml
  values.yaml          # single-node default
  values-distributed.yaml   # UI + server + storage tiers (P3-13)
  templates/
    deployment.yaml    # single-node OR server Deployment
    statefulset.yaml   # storage tier (optional)
    ui-deployment.yaml # P3-16
    service.yaml
    ingress.yaml
    pvc.yaml
    secret.yaml
    servicemonitor.yaml   # optional Prometheus
    pdb.yaml
```

### Deliverables

| Item | Detail |
|------|--------|
| Helm chart | Official chart under `deploy/helm/maxio` |
| Single-node profile | `replicas: 1`, RWO PVC, liveness `/healthz`, readiness `/readyz` |
| Ingress | TLS, body size unlimited for uploads, separate paths for `/ui` and `/api` when P3-16 lands |
| Secrets | `access-key` / `secret-key` via Secret; optional `MAXIO_ADMIN_TOKEN` |
| CI | `helm template` + kubeconform or `helm lint` in GitHub Actions |
| Kind smoke test | Optional CI job: helm install → PUT/GET via port-forward |
| Distributed values | Documented values overlay when P3-13 tiers ship |

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
| **P3-19** | Kubernetes Helm chart |
| **P3-20** | Epic — first-class deployment targets (closes when P3-18 + P3-19 done) |

## Acceptance (epic P3-20)

- [ ] Bare metal install documented and systemd units shipped
- [ ] Helm chart installs single-node MaxIO on K8s 1.28+
- [ ] CI validates chart templates
- [ ] Both paths listed in README deployment section