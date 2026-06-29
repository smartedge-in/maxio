# Security audit and hardening checklist (P3-50)

Pre-production and penetration-test preparation for MaxIO in enterprise and airgapped
environments. Pair with the runtime egress matrix in [`operations.md`](operations.md)
(P3-59) and the CycloneDX SBOM from [`scripts/build-offline-bundle.sh`](../scripts/build-offline-bundle.sh)
(P3-54).

## Threat model (summary)

| Asset | Risk | Mitigations |
|-------|------|-------------|
| Object data (`data_dir/buckets/`) | Unauthorized read/write | SigV4 auth, TLS at edge, network segmentation |
| SSE-S3 keyring (`.maxio-keys.json`) | Permanent data loss or key theft | Offline backup (P3-48), `chmod 600`, dedicated secrets store |
| Bootstrap / secondary credentials | Account takeover | Strong keys, no `--allow-insecure-dev`, rotate and restart |
| Console sessions | Session hijack | `MAXIO_SECURE_COOKIES`, HTTPS, short-lived cookies + fingerprint |
| Admin API (`/api/admin/v1/*`) | Privileged remote control | `MAXIO_ADMIN_TOKEN`, TLS, IP allowlists, rate limits |
| Supply chain (binaries, images) | Trojaned artifacts | `SHA256SUMS` verify, SBOM review, private registry only (P3-54/55) |

MaxIO has **no built-in telemetry, license phone-home, or automatic update checks**.
Outbound connections are opt-in only — see P3-59 in `docs/operations.md`.

## Pre-deployment checklist

### Network and exposure

- [ ] MaxIO not published directly to the internet; TLS terminated at Caddy/Traefik/Ingress (P3-26, P3-57)
- [ ] `MAXIO_ADDRESS` bound to loopback or private interface on bare metal; Service/Ingress controls external access on K8s
- [ ] `MAXIO_TRUSTED_PROXIES` lists only operator-controlled proxy CIDRs
- [ ] Firewall allows only required ports (443 at edge; 9000 internal)
- [ ] Admin API and storage Raft ports (`9100`) reachable only from cluster networks

### Authentication and secrets

- [ ] `MAXIO_ACCESS_KEY` / `MAXIO_SECRET_KEY` are strong, unique, not defaults
- [ ] `MAXIO_ALLOW_INSECURE_DEV` is **unset** or `false`
- [ ] `MAXIO_ADMIN_TOKEN` set for production admin API use
- [ ] `.maxio-credentials.json` permissions `600` if secondary keys are used
- [ ] `MAXIO_MASTER_KEY` stored in org secrets manager when used (keyring still backed up)

### TLS and PKI (airgap)

- [ ] Production certs issued by internal CA (P3-57) — no public ACME on classified hosts
- [ ] `MAXIO_SECURE_COOKIES=true` when console served over HTTPS
- [ ] Keycloak (`MAXIO_KEYCLOAK_*`) points to **internal** IdP URLs only (P3-08)

### Storage and availability

- [ ] Data volume on durable block storage (SSD / SAN RWO)
- [ ] `MAXIO_MIN_FREE_DISK_BYTES` aligned with monitoring alerts
- [ ] `MAXIO_MAX_OBJECT_BYTES` set when upload caps are required
- [ ] Scheduled backup with checksum verify (`scripts/backup-maxio.sh`, P3-48)
- [ ] Restore drill completed at least once per quarter (P3-49)

### Observability and audit

- [ ] `MAXIO_AUDIT_LOG=true` with log shipping to SIEM
- [ ] `MAXIO_METRICS_ENABLED=true` scraped by on-prem Prometheus (P3-37)
- [ ] Alerts on `/readyz` 503, low `maxio_disk_free_bytes`, elevated 5xx rate

### Supply chain (airgap)

- [ ] Offline bundle checksum verified before install (`SHA256SUMS`)
- [ ] `sbom.json` reviewed; no unexpected components (Trivy `trivy sbom`)
- [ ] Container images loaded from P3-55 pack only — no `docker pull` from public registries on target
- [ ] `cargo deny check licenses` / `make deny` passed on release build host
- [ ] Optional: cosign/sigstore signatures verified when published by your release pipeline

## Hardening review

### Application

- [ ] Rate limits tuned for expected client population (`MAXIO_S3_RATE_*`, admin rate limits)
- [ ] Redis login rate limit (`MAXIO_LOGIN_RATE_LIMIT_REDIS_URL`) used when console replicas > 1
- [ ] Bucket policies reviewed — v1 allows only Allow + `Principal:*` + limited actions
- [ ] CSP reviewed after UI changes (`CONTENT_SECURITY_POLICY` in `maxio-server`)
- [ ] Erasure coding parity configured when bitrot recovery is required (`--parity-shards`)

### Host / container

- [ ] Dedicated `maxio` user; data directory not world-readable (`deploy/systemd/maxio.service`)
- [ ] systemd `ProtectSystem`, `NoNewPrivileges`, and related hardening enabled
- [ ] Container runs as non-root (`USER maxio` in `Dockerfile`)
- [ ] Image scanned with Trivy (`make trivy-image`) before promotion

### Kubernetes

- [ ] `imagePullSecrets` configured for private registry (P3-60)
- [ ] Image tags pinned to `REGISTRY/maxio:VERSION` — no `:latest` in production
- [ ] Secrets via Kubernetes Secret or external secrets operator — not plain env in manifests
- [ ] NetworkPolicy restricts storage tier to server tier only (site-specific)
- [ ] Pod security standards: read-only root filesystem where compatible, drop capabilities

## Penetration test focus areas

1. **S3 auth bypass** — unsigned requests, signature tampering, clock skew, presigned URL expiry
2. **Console auth** — login brute force, session fixation, CSRF on mutating routes
3. **Admin API** — token leakage, Basic auth fallback, rate-limit bypass via IP rotation
4. **Path traversal / bucket isolation** — cross-bucket access via virtual-host or encoded paths
5. **Multipart abuse** — incomplete uploads, quota exhaustion, slowloris on large PUTs
6. **Policy gaps** — public-read policy scope, listing exposure
7. **Supply chain** — substitute bundle or image in sneakernet path; verify detection via checksums

## Egress verification (P3-59)

Confirm in staging with egress firewall **deny-all outbound**:

- [ ] PUT/GET/LIST/DELETE succeed for local clients
- [ ] `/healthz`, `/readyz`, `/metrics` respond
- [ ] Console login works without external IdP (unless Keycloak enabled with internal URL)
- [ ] No DNS queries or TCP connections to public SaaS from MaxIO process (packet capture)

Document any optional outbound deps (Redis, Keycloak, future webhooks/KMS) in the operations egress matrix.

## Sign-off

| Role | Name | Date | Bundle version / image digest |
|------|------|------|-------------------------------|
| Platform owner | | | |
| Security reviewer | | | |
| Operations lead | | | |

Store signed checklist with change-management records. Re-run after every major upgrade (P3-58).