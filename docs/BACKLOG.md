# MaxIO Backlog

Actionable backlog derived from codebase review (2026-06-28). Items are ordered by priority within each tier.

**Legend**

| Field | Meaning |
|-------|---------|
| **Priority** | P0 = production blocker · P1 = security/reliability · **P1-MR = multi-replica & EC (product #1)** · **P1-GA = enterprise GA gate (post-cluster)** · **P1-ENT = enterprise GA+ (regulated / multi-tenant)** · P2 = maintainability · P3 = deferred / future |
| **Effort** | S (< 1 day) · M (1–3 days) · L (3–7 days) · XL (> 1 week) |
| **Area** | ci · ops · security · storage · api · auth · ui · docs |

**Deployment focus:** Enterprise production assumes **airgapped** (or strictly egress-controlled) environments — offline artifacts, private registries, internal CA, no runtime internet dependency. Internet-connected install paths are optional convenience only.

---

## Priority 1 — Multi-replica architecture & erasure coding (Raft-first)

**Product direction:** Live multi-node cluster via **dual independent Raft** (storage + server) and **distributed erasure coding** across storage nodes. No operator-sync detour on the critical path. Plan: `docs/plans/2026-06-29-multi-replica-raft-priority.md`.

**Do not:** scale `Deployment.replicas` on one RWO PVC; round-robin LB across uncordinated MaxIO instances.

| Order | ID | Title | Area | Effort | Description | Acceptance criteria |
|-------|-----|-------|------|--------|-------------|-------------------|
| ~~—~~ | ~~**P1-14**~~ | ~~**Multi-replica epic (Raft-first)**~~ | storage | XL | **Done** — `maxio-cluster` crate: storage Raft, distributed EC, server routing snapshot, harness tests; `maxio-ui` stateless tier; `deploy/k8s/distributed/`. Single-node colocated mode unchanged (`MAXIO_CLUSTER_MODE=false`). | 3-node storage quorum + failover test; distributed EC rebuild test; 2+ server + 2+ UI replicas; single-node colocated mode preserved; epic doc acceptance checklist complete. |
| ~~1~~ | ~~P1-15~~ | ~~`StorageBackend` trait~~ | storage | L | **Done** — `FilesystemStorage` behind `StorageBackend`; server uses `DynStorage` (`Arc<dyn StorageBackend>`). | Trait in `maxio-storage/src/backend.rs`; integration tests pass. |
| ~~2~~ | ~~P1-16~~ | ~~Raft library spike & license gate~~ | storage | S | **Done** — OpenRaft `0.9` selected; `raft-spike` feature + CI smoke test. | `docs/plans/2026-06-29-raft-library-spike.md`; `cargo deny` clean. |
| ~~3~~ | ~~P1-22~~ | ~~`maxio-common` crate~~ | storage | M | **Done** — `VERSION`, admin DTOs, cluster routing types; no `axum`/`reqwest`/storage I/O. | `crates/maxio-common`; server + storage + admin import shared types. |
| ~~4~~ | ~~P1-17~~ | ~~Storage tier Raft~~ | storage | XL | **Done** — OpenRaft 0.9 in `maxio-cluster`; `StorageMutation` via leader; metrics `raft_storage_leader`, `raft_storage_commit_lag`. *Was P3-14.* | 3-node bootstrap/join; writes via leader; follower failover integration test; metrics `raft_storage_leader`, `raft_storage_commit_lag`. |
| ~~5~~ | ~~P1-18~~ | ~~Distributed erasure coding~~ | storage | XL | **Done** — shard placement map in Raft; physical shards under `.cluster-shards/`; round-robin across storage nodes. | Placement policy (e.g. K data + M parity across distinct nodes); PUT stripes shards; shard map in Raft; integration test 3 storage nodes + EC object. |
| ~~6~~ | ~~P1-19~~ | ~~Multi-node EC read & rebuild~~ | storage | L | **Done** — peer shard fetch when local copy missing (`maxio-cluster/src/ec/`). | Read path pulls missing shard from peer; rebuild after one node down (with sufficient parity); tests with induced shard loss. |
| ~~7~~ | ~~P1-20~~ | ~~Server tier Raft~~ | api | XL | **Done** — `RoutingSnapshot` + `ClusterState`; `MAXIO_CLUSTER_MODE` gates `/readyz` on storage quorum; Prometheus `maxio_cluster_*` gauges. *Was P3-15.* | 2+ server quorum; storage leader change reflected without manual config; `/readyz` reflects storage quorum; integration test with P1-17. |
| ~~8~~ | ~~P1-21~~ | ~~Stateless UI tier (`maxio-ui`)~~ | ui | L | **Done** — `crates/maxio-ui` binary; `MAXIO_SERVE_UI=false` on server when UI tier is separate. *Was P3-16.* | `crates/maxio-ui`; distributed deploy; Ingress split `/ui` vs S3; ≥2 UI replicas in test manifest. |
| ~~9~~ | ~~P1-24~~ | ~~Multi-node CI / dev harness~~ | ci | M | **Done** — `scripts/cluster-test.sh`; CI `cluster` job; `deploy/k8s/distributed/` (3 storage, 2 server, 2 UI). | Script or CI job: bootstrap Raft, PUT/GET, kill leader, EC shard placement smoke; runs on main or nightly; documented airgap recipe uses private registry + no `cargo build` on target. |
| ~~10~~ | ~~P1-25~~ | ~~EC bitrot scanner (cluster-aware)~~ | storage | L | **Done** — `maxio-cluster/src/ec/bitrot.rs`; storage `MAXIO_BITROT_SCAN_*`; HTTP `/internal/shard`; Prometheus `maxio_ec_bitrot_*`; cluster test. | Background scanner; cross-node heal when local shard corrupt; Prometheus counters; ops tuning doc. |

**P1-MR dependency graph**

```
P1-15 → P1-16 → P1-22 → P1-17 → P1-18 → P1-19
                              ↘ P1-20 → P1-21
P1-24 (parallel once P1-17 alpha)
P1-25 (after P1-18)
```

**Sprint order (Priority 1):** P1-15 → P1-16 → P1-22 → P1-17 → P1-18 → P1-19 → P1-20 → P1-21 → P1-24 → P1-25 → **P1-14 closes**

**Supporting (parallel, not blocking Raft core):** P3-24 (license gate for Raft dep), P3-45/46 (Cilium docs + plain K8s YAML when on K8s). **Helm (P3-19) is future improvement — not on Priority 1 path.**

---

## Enterprise GA — production gate (post P1-14)

**Goal:** Remove the README production warning and ship a supportable enterprise **single-region HA** cluster in **airgapped** environments. Requires **P1-14 closed** first. Plain K8s YAML + bare metal (not Helm); **no internet on install or steady-state** for core MaxIO. Closes via **P3-53** (airgap) + **P3-52** → **P3-44**.

### Airgap deployment (primary enterprise path)

| Order | ID | Title | Area | Effort | Description | Acceptance criteria |
|-------|-----|-------|------|--------|-------------|-------------------|
| ~~—~~ | ~~**P3-53**~~ | ~~**Airgap deployment epic**~~ | ops | L | **Done** — P3-54–P3-60; `docs/operations.md` airgap sections. | Airgap acceptance checklist in runbook; CI/release ships offline artifacts; no undocumented egress. |
| ~~1~~ | ~~P3-54~~ | ~~Offline release bundle~~ | ci | M | **Done** — `scripts/build-offline-bundle.sh`. | Release tarball: `maxio` + `maxio-admin` + `maxio-ui`, `SHA256SUMS`, SBOM, `LICENSES.txt`. |
| ~~2~~ | ~~P3-55~~ | ~~Offline container image pack~~ | ci | M | **Done** — `scripts/build-offline-images.sh`, `scripts/load-images.sh`. | `docker save` tarballs; `images.txt`; private registry load script. |
| ~~3~~ | ~~P3-56~~ | ~~Airgap deployment runbook~~ | docs | M | **Done** — `docs/operations.md` § Airgap. | Sneakernet bundle ingest, private registry, systemd/K8s paths. |
| ~~4~~ | ~~P3-57~~ | ~~Internal CA & TLS~~ | ops | S | **Done** — `docs/operations.md` § Internal CA/TLS. | Org-issued certs; no ACME on production hosts. |
| ~~5~~ | ~~P3-58~~ | ~~Offline upgrade & rollback~~ | ops | M | **Done** — `docs/operations.md` § Offline upgrade. | Versioned upgrade; rollback N-1; checksum verify. |
| ~~6~~ | ~~P3-59~~ | ~~Runtime egress matrix~~ | docs | S | **Done** — `docs/operations.md` § Runtime egress. | Required (none) vs optional deps table. |
| ~~7~~ | ~~P3-60~~ | ~~Private-registry K8s manifests~~ | ops | M | **Done** — `REGISTRY/` placeholders + `imagePullSecrets` + `registry-secret.example.yaml`. | No public registry defaults. |

**Airgap sprint order:** P3-54 → P3-55 → P3-56 → P3-57 → P3-58 → P3-59 → P3-60 → **P3-53 close**

### Enterprise GA (general)

| Order | ID | Title | Area | Effort | Description | Acceptance criteria |
|-------|-----|-------|------|--------|-------------|-------------------|
| ~~—~~ | ~~**P3-52**~~ | ~~**Enterprise GA epic**~~ | ops | L | **Done** — P3-44 milestone; README GA notice; CHANGELOG entry. | GA checklist complete; README warning removed; CHANGELOG GA entry. |
| ~~1~~ | ~~P3-18~~ | ~~Bare metal deployment pack~~ | ops | M | **Done** — `deploy/systemd/maxio.service`; airgap install in `docs/operations.md`. | systemd unit; offline install; `maxio healthcheck`. |
| ~~2~~ | ~~P3-26~~ | ~~Permissive ingress & HA runbook~~ | ops | S | **Done** — `docs/operations.md` + `docs/plans/2026-06-29-permissive-ingress-ha.md`. | Caddy/Traefik file-cert; no GPL edge default. |
| ~~3~~ | ~~P3-36~~ | ~~Published S3 compatibility matrix~~ | docs | S | **Done** — `docs/s3-compatibility.md` with CI references. | Matrix in repo; linked from README. |
| ~~4~~ | ~~P3-37~~ | ~~Observability reference stack~~ | ops | M | **Done** — `deploy/compose/observability.yml` + Grafana dashboard JSON. | Prometheus + Grafana compose; on-prem images via P3-55. |
| ~~5~~ | ~~P3-08~~ | ~~Keycloak console UI login~~ | ui | M | **Done** — `MAXIO_KEYCLOAK_*`; `/api/auth/keycloak-*`; internal-URL docs in `docs/operations.md`. | Keycloak UI path; silent refresh; airgap internal IdP only. |
| ~~6~~ | ~~P3-06~~ | ~~UI E2E tests (Playwright)~~ | ui | M | **Done** — `e2e/` + `scripts/e2e-console.sh`; CI `e2e` job. | Login → bucket → upload → download → delete; CI on main. |
| ~~7~~ | ~~P3-48~~ | ~~Backup automation & verified restore~~ | ops | M | **Done** — `scripts/backup-maxio.sh`; ops doc. | Checksum verify; restore drill documented. |
| ~~8~~ | ~~P3-49~~ | ~~Disaster recovery runbook~~ | docs | M | **Done** — `docs/operations.md` § DR. | RPO/RTO; cluster + BM; offline restore. |
| ~~9~~ | ~~P3-50~~ | ~~Security audit checklist~~ | security | M | **Done** — `docs/security-audit.md`. | Threat model; egress; SBOM review. |
| ~~10~~ | ~~P3-51~~ | ~~Production SLA & incident response~~ | ops | S | **Done** — `docs/operations.md` § SLA. | Severity levels; on-prem metrics. |
| ~~11~~ | ~~P3-24~~ | ~~Permissive-only license policy~~ | ci | S | **Done** — Rust `cargo deny` + npm runtime dep audit. | `deny.toml`; `make deny`; `make npm-licenses`; CI licenses job. |
| ~~12~~ | ~~P3-05~~ | ~~ARM64 release binaries~~ | ci | S | **Done** — `.github/workflows/release.yml` multi-arch; per-arch P3-54 bundles. | amd64 + arm64 in release CI and offline bundle script. |
| ~~—~~ | ~~**P3-44**~~ | ~~**Production GA milestone**~~ | ops | M | **Done** — P1-14 + P3-52 + P3-53 closed. | GA checklist; README updated; Helm not required. |

**GA sprint order:** P1-14 close → **P3-54 → P3-55 → P3-56 → P3-57 → P3-58 → P3-59 → P3-60 → P3-53** → P3-18 → P3-26 → P3-36 → P3-37 → P3-08 → P3-06 → P3-48 → P3-49 → P3-50 → P3-51 → P3-24 → P3-05 → **P3-44 / P3-52 close**

**Parallel after GA cluster work starts:** P3-17 (admin CLI boundary), P3-23 (crate boundary CI).

---

## Enterprise GA+ — regulated & multi-tenant (post GA)

**Goal:** Features enterprise buyers in finance, healthcare, and SaaS typically require **after** initial GA. Subordinate to P1-14; does not block P3-44 unless pilot demands it.

| Order | ID | Title | Area | Effort | Description | Acceptance criteria |
|-------|-----|-------|------|--------|-------------|-------------------|
| — | **P3-43** | **RustFS / enterprise parity epic** | storage | XL | **Epic** — competitive and compliance gaps. Closes when Tier 1–3 complete. | See tier table below. |
| 1 | P3-28 | IAM bucket policies v2 | api | L | Deny, conditions, expanded actions — least-privilege. | Deny precedence; `Condition` operators; integration tests; `docs/s3-compatibility.md` updated. |
| 2 | P3-35 | External KMS (SSE-KMS compatible) | security | L | SSE-S3/SSE-C only today; regulated workloads need KMS. **Airgap:** on-prem Vault/HSM on internal network only — no cloud KMS. | Pluggable KMS trait; Vault transit or equivalent; `aws:kms` path; deny.toml-clean deps; offline KMS bootstrap doc. |
| 3 | P3-29 | Multi-tenancy | auth | L | Static credentials without tenant boundary. | Tenant ID on buckets/credentials; scoped requests; admin lists tenant only; default tenant migration. |
| 4 | P3-38 | OIDC claims in bucket policies | auth | L | Keycloak console (P3-08) does not feed IAM evaluation. | Policy conditions on `jwt:groups` / `jwt:roles`; Entra/Keycloak example; mock OIDC tests. |
| 5 | P3-39 | S3 server access logging | api | M | stderr audit (P2-08) ≠ per-bucket delivery to target bucket. | `?logging` config; log delivery to target bucket; integration test. |
| 6 | P3-27 | S3 event notifications | api | L | Audit log only; no webhook/subscriber integrations. **Airgap:** webhook targets must be internal URLs only; no SaaS endpoints. | Webhook target minimum; durable spool; `ObjectCreated` / `ObjectRemoved`; integration test; egress doc updated (P3-59). |
| 7 | P3-47 | Object lock / WORM / legal hold | storage | L | Immutability for compliance — not in backlog before. | Governance retention; legal hold API subset; tests; docs vs versioning. |
| 8 | P3-33 | Lifecycle transitions & non-current expiry | storage | M | P3-01 expiration only. | `transition_days`; `noncurrent_expiration_days`; versioned purge tests. |

**GA+ sprint order:** P3-28 → P3-35 → P3-29 → P3-38 → P3-39 → P3-27 → P3-47 → P3-33 → **P3-43 Tier 1–3 close**

### Geo-DR & replication (deferred — after Raft)

| ID | Title | Notes |
|----|-------|-------|
| P3-34 | S3 bucket replication API (CRR) | Builds on mutation log; primary→standby; extends deferred P3-09–12 |
| P3-09 | Operator sync runbook | Geo-DR tooling revisit **after** P1-14 — not Raft substitute |

### RustFS parity tiers (P3-43)

| Tier | IDs | When |
|------|-----|------|
| Tier 1 (ops/docs) | P3-36, P3-37, P3-05, **P3-53–P3-60** | **Pulled into Enterprise GA** (airgap + ops/docs) |
| Tier 2 (S3 product) | P3-27, P3-28, P3-33, P3-39 | Enterprise GA+ |
| Tier 3 (enterprise) | P3-29, P3-35, P3-38, **P3-47** | Enterprise GA+ |
| Tier 4 (protocol) | P3-30, P3-31, P3-34 | Deferred unless OpenStack / CRR required |

---

## Future / deferred

| ID | Title | Notes |
|----|-------|-------|
| P3-19 | Kubernetes Helm chart | Future improvement — plain `deploy/k8s/` for P1-14 |
| P3-45 | Cilium eBPF deployment guide | After P1-24 harness |
| P3-46 | Multi-node Service topology (K8s) | Plain YAML per tier |
| P3-30 | OpenStack Swift API | XL — only if OpenStack required |
| P3-31 | OpenStack Keystone auth | L — pairs with P3-30 |
| P3-40 | Storage API fuzz testing in CI | Nightly acceptable |
| P3-41 | Offline bare-metal install helper | Post P3-54 — local bundle only |
| P3-42 | Optional native TLS termination | Edge/single-node convenience |
| P3-20 | Deployment targets epic | Closes P3-18 + plain K8s; Helm optional |

---

## P0 — Production blockers

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P0-01~~ | ~~Re-enable `cargo test` in CI~~ | ci | S | Done — `cargo test --all --all-features` in `.github/workflows/ci.yml`. | — |
| ~~P0-02~~ | ~~Implement storage-aware `/readyz`~~ | ops | M | Done — `check_readiness()` probes data dir + keyring; documented in README and `docs/operations.md`. | — |
| ~~P0-03~~ | ~~Global upload / disk quota~~ | storage | L | Done — `MAXIO_MAX_OBJECT_BYTES`, `MAXIO_MIN_FREE_DISK_BYTES`, `QuotaReader` enforcement. | — |
| ~~P0-04~~ | ~~Production deployment guide~~ | docs | M | Done — `docs/operations.md`. | — |
| ~~P0-05~~ | ~~Run `aws_cli_test.sh` in CI (main)~~ | ci | M | Done — `aws-cli` job on push to `main`. | — |

---

## P1 — Security & reliability

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P1-01~~ | ~~S3 API rate limiting~~ | security | M | Done — `MAXIO_S3_RATE_AUTH_*` and `MAXIO_S3_RATE_PUT_*`; per-IP sliding window; `429` + `Retry-After` + S3 `SlowDown`. | — |
| ~~P1-02~~ | ~~Tighten console CSP~~ | ui | M | Done — theme bootstrap moved to `/ui/theme-init.js`; `script-src 'self'` only; `style-src` keeps `'unsafe-inline'` for Svelte (documented). | — |
| ~~P1-03~~ | ~~Trusted proxy configuration~~ | security | S | Done — `MAXIO_TRUSTED_PROXIES` CIDR list; `X-Forwarded-For` honored only from trusted peers for console login + S3/admin rate limits. | — |
| ~~P1-04~~ | ~~Secure bind defaults documentation~~ | ops | S | Done — README and `docs/operations.md` warn about `0.0.0.0` exposure; recommend `127.0.0.1` for dev and ingress-only for prod. | — |
| ~~P1-05~~ | ~~Session invalidation on credential rotate~~ | auth | M | Done — console session tokens include credential fingerprint; old tokens rejected after access/secret change. | — |
| ~~P1-06~~ | ~~Distributed login rate limit~~ | security | M | Done — optional `MAXIO_LOGIN_RATE_LIMIT_REDIS_URL` Redis backend; in-memory default documented for single-replica console. | — |
| ~~P1-07~~ | ~~Presigned URL detection hardening~~ | auth | S | Done — `query_has_presigned_signature()` and case-insensitive `parse_presigned_query`; unit + integration tests. | — |
| ~~P1-08~~ | ~~Deep health metrics~~ | ops | M | Done — `/healthz?verbose=1` JSON: uptime, readyz, disk free %, active multipart uploads, housekeeping lag. | — |

---

## P1 — S3 compatibility & product

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P1-09~~ | ~~Virtual-hosted-style requests~~ | api | XL | Done — `Host: bucket.{server_host}` dispatch; `MAXIO_SERVER_HOST`; SigV4 uses client path; integration test with explicit Host. | — |
| ~~P1-10~~ | ~~Multi-user / IAM-style credentials~~ | auth | XL | Done — phase 1: `CredentialStore` + `.maxio-credentials.json`; design doc `docs/plans/2026-06-28-multi-user-credentials.md`. | — |
| ~~P1-11~~ | ~~Bucket policy engine~~ | api | XL | Done — v1 JSON policy subset (`Allow`, `Principal:*`, GetObject/ListBucket); `docs/plans/2026-06-28-bucket-policy-evaluation.md`. | — |

---

## P1 — Erasure coding (reliability & coverage)

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P1-12~~ | ~~Clean S3 errors on EC read failure~~ | storage | M | Done — `VerifiedChunkReader::preflight()` before streaming; `IntegrityError` → HTTP 500 `InternalError`; tests assert XML body. | — |
| ~~P1-13~~ | ~~EC corruption tests in CI~~ | ci | S | Done — `aws-cli` job starts MaxIO with `--erasure-coding`; corruption checks in `aws_cli_test.sh` run, not skip. | — |

---

## P2 — Maintainability & code health

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P2-01~~ | ~~Split `filesystem.rs`~~ | storage | L | Done — `src/storage/filesystem/` with `mod.rs`, `common.rs`, `object_io`, `multipart`, `encryption_io`, `listing`, `housekeeping`; behavior unchanged. | — |
| ~~P2-02~~ | ~~Reduce crate-level clippy allows~~ | storage | M | Done — removed crate-level `#![allow(clippy::…)]` from `main.rs` / `lib.rs`; fixed mechanical lints; `too_many_arguments` allowed on 3 storage entry points with comments. | — |
| ~~P2-03~~ | ~~Add `bun run check` to CI~~ | ci | S | Done — `bun run check` step in `.github/workflows/ci.yml` before frontend build. | — |
| ~~P2-04~~ | ~~Unit test coverage report in CI~~ | ci | S | Done — `coverage` CI job with `cargo llvm-cov --summary-only`; floors: `storage/crypto.rs` ≥80% lines, `auth/signature_v4.rs` ≥25% lines. | — |
| ~~P2-05~~ | ~~Replace `unwrap()` in hot paths~~ | storage | M | Done — `auth/hmac` helper; storage listing/object/multipart/encryption paths return `StorageError`/`IntegrityError` instead of panicking; mutex poison handled in rate limiter. | — |
| ~~P2-06~~ | ~~Console API integration tests~~ | api | M | Done — integration tests for login failure, login rate limit, auth check/logout, list buckets, versioning/public settings, protected-route auth gate (presign/upload/settings covered by existing tests). | — |
| ~~P2-09~~ | ~~Multipart + EC integration tests~~ | storage | M | Done — `test_multipart_complete_ec` and `test_multipart_complete_ec_sse_s3` under `start_server_ec()`. | — |
| ~~P2-10~~ | ~~CopyObject + EC integration tests~~ | storage | S | Done — same-bucket, cross-bucket, and SSE-S3 copy tests with EC enabled; `.ec` dir + GET roundtrip verified. | — |
| ~~P2-11~~ | ~~Document EC operational limits~~ | docs | S | Done — `docs/operations.md` erasure coding section (flags, parity, 255-shard cap, single-node scope, read errors). | — |

---

## P2 — Operations tooling

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P2-13~~ | ~~Authenticated admin API (remote ops)~~ | api | L | Done — `/api/admin/v1/*` with Bearer token or Basic access/secret auth, per-IP rate limiting, JSON handlers for status/info/doctor/buckets/keyring/housekeeping; integration tests. | — |
| ~~P2-12~~ | ~~MaxIO admin / ops CLI (remote-first)~~ | ops | XL | Done — `maxio-admin` remote commands via P2-13 API; local `doctor --data-dir` and `keyring rotate`; profiles, `--json`, docs in `docs/operations.md`. | — |

---

## P2 — Observability

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P2-07~~ | ~~Prometheus `/metrics` endpoint~~ | ops | M | Done — `MAXIO_METRICS_ENABLED` exposes `GET /metrics`; optional `MAXIO_METRICS_PORT` dedicated listener; counters for requests, latency, SlowDown, upload bytes; gauges for uptime, disk, multipart uploads. | — |
| ~~P2-08~~ | ~~Structured audit log~~ | security | M | Done — `MAXIO_AUDIT_LOG` emits JSON lines (`target=maxio_audit`) for mutating S3/console/admin requests: principal, bucket, key, status, outcome. | — |

---

## P3 — Full item registry

Canonical list of all P3 IDs. **Priority order** is in sections above: **P1-MR** → **Enterprise GA** → **Enterprise GA+** → **Future / deferred**.

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P3-01~~ | ~~Lifecycle rules (expiration)~~ | storage | L | Done — prefix-based `LifecycleRule` on `BucketMeta`; `PUT/GET/DELETE ?lifecycle`; hourly housekeeping sweep expires non-versioned objects. | — |
| ~~P3-02~~ | ~~Replication / federation~~ | storage | XL | Done (RFC) — `docs/plans/2026-06-29-replication-federation.md`; implementation deferred to Phase 1+ runbook. | — |
| ~~P3-03~~ | ~~SQLite metadata index~~ | storage | L | Done — `MAXIO_METADATA_INDEX` enables `{data_dir}/.maxio-metadata.db`; upsert on write/delete; rebuild on startup; walk fallback. | — |
| ~~P3-04~~ | ~~Workspace crate split~~ | storage | L | Done — `crates/maxio-storage` (filesystem, crypto, keys, policy, quota) and `crates/maxio-server` (HTTP/S3 API, auth, embedded UI); root `maxio` is a facade binary + re-exports; `map_storage_upload_error` lives in server `error` module. | — |
| P3-05 | ARM64 release binaries | ci | S | **P1-GA** — multi-arch for edge and ARM servers. | Multi-arch Docker image and GitHub release assets. |
| P3-06 | UI E2E tests (Playwright) | ui | M | **P1-GA** — browser-level console regression. | Smoke test: login → create bucket → upload → download → delete; CI on main. |
| ~~P3-07~~ | ~~Per-bucket erasure coding toggle~~ | storage | L | Done — `BucketMeta.erasure_coding` override; `PUT/GET ?erasure`; writes use `effective_erasure_coding()`; reads layout-based; existing flat objects unchanged. | — |
| P3-08 | Keycloak console UI login | ui | M | **P1-GA** — internal Keycloak URL only (airgap). | Keycloak UI path; silent refresh; Playwright smoke; no external IdP. |
| ~~P3-09~~ | ~~Replication Phase 1 — operator sync runbook~~ | ops | M | **Deferred** — not on Raft-first critical path (see Priority 1). May revisit for geo-DR tooling after P1-14. | — |
| ~~P3-10~~ | ~~Replication Phase 2 — mutation event log~~ | storage | XL | **Superseded by P1-15** — `StorageBackend` trait is the Raft-first entry; optional append-only log deferred unless needed for audit. | — |
| ~~P3-11~~ | ~~Replication Phase 3 — replication agent~~ | storage | XL | **Deferred** — `maxio-replicate` sidecar not on critical path; Raft + distributed EC is Priority 1. | — |
| ~~P3-12~~ | ~~Multiple replicas support (operator epic)~~ | storage | XL | **Superseded by P1-14** — multi-replica epic is Raft-first, not rsync/agent. | — |
| ~~P3-13~~ | ~~Asymmetric scale-out with dual Raft (epic)~~ | storage | XL | **Superseded by P1-14** — same architecture, now Priority 1. | — |
| ~~P3-14~~ | ~~Storage tier Raft consensus~~ | storage | XL | **Superseded by P1-17**. | — |
| ~~P3-15~~ | ~~Server tier Raft consensus~~ | api | XL | **Superseded by P1-20**. | — |
| ~~P3-16~~ | ~~UI crate — stateless scale-out~~ | ui | L | **Superseded by P1-21**. | — |
| ~~P3-17~~ | ~~Admin CLI crate boundary~~ | ops | M | **Done** — `maxio-admin` depends on `maxio-common` + `maxio-storage` only (no root `maxio` / `maxio-server`). | No `maxio` or `maxio-server` path dep in `maxio-admin/Cargo.toml`; local doctor/keyring use `maxio-storage` directly; separate release binary/artifact; docs in `docs/operations.md`. |
| P3-18 | Bare metal deployment pack | ops | M | **P1-GA** — offline install from P3-54 bundle; permissive edge (P3-26, P3-57). See `docs/plans/2026-06-29-deployment-targets.md`. | `deploy/systemd/maxio.service`; airgap install path; smoke via `maxio healthcheck`. |
| P3-19 | Kubernetes Helm chart | ops | L | **Future** — not required for P1-14 or GA. Plain `deploy/k8s/` for cluster. | `deploy/helm/maxio`; `helm lint` + `helm template` in CI; README section. |
| P3-20 | Deployment targets epic (bare metal + K8s) | ops | L | **Future** — closes when P3-18 + plain K8s done; Helm optional (P3-19). | P3-18 complete; distributed BM + plain K8s documented. |
| P3-21 | Shared library strategy (epic) | storage | M | **Epic** — thin shared types without a monolithic “god crate”. `maxio-storage` remains storage SSOT; new `maxio-common` for cross-component contracts; root facade not a sibling dependency (P3-17). UI stays npm-only. See `docs/plans/2026-06-29-shared-libraries.md`. | P3-22 + P3-23 + P3-17 complete; dependency graph documented; no `axum`/`reqwest` in `maxio-common`. |
| ~~P3-22~~ | ~~`maxio-common` crate~~ | storage | M | **Promoted to P1-22** on Priority 1 critical path. | — |
| P3-23 | Crate boundary CI enforcement | ci | S | Automate dependency rules from P3-04 / shared-library plan: e.g. `maxio-admin` must not depend on `maxio` or `maxio-server`; `maxio-common` must not depend on `axum` or `maxio-storage`. | `cargo deny` bans or CI script fails on forbidden edges; documented in `docs/plans/2026-06-29-shared-libraries.md`; passes on current graph after P3-17/P3-22. |
| ~~P3-24~~ | ~~Permissive-only license policy (mandatory)~~ | ci | S | **Done** — no copyleft in production artifacts; Raft deps pass. See `docs/licensing.md`. | `deny.toml`; `make deny`; `make npm-licenses`; CI licenses job. |
| ~~P3-25~~ | ~~Optional edge LB (`maxio-edge` / Pingora)~~ | ops | L | **Moved to [knx-edge](https://github.com/smartedge-in/knx-edge)** — out of tree. See `docs/out-of-tree/knx-edge.md`. | — |
| P3-26 | Permissive ingress & HA runbook | ops | S | **P1-GA** — no GPL edge; **airgap:** internal CA (P3-57), no ACME. | Caddy file-cert examples; GPL tools not recommended. |
| P3-27 | S3 event notifications | api | L | **P1-ENT** — internal webhook targets only (airgap). | Webhook target; durable spool; `ObjectCreated` / `ObjectRemoved`; P3-59 egress doc. |
| P3-28 | IAM bucket policies v2 | api | L | **P1-ENT** — Deny, conditions, expanded actions. | Deny precedence; `Condition` operators; `docs/s3-compatibility.md` updated. |
| P3-29 | Multi-tenancy | auth | L | **P1-ENT** — tenant boundary on buckets and credentials. | Tenant-scoped requests; admin lists tenant only; default tenant migration. |
| P3-30 | OpenStack Swift API | api | XL | **Future** — only if OpenStack required. | Swift object paths; container listing; smoke test. |
| P3-31 | OpenStack Keystone authentication | auth | L | **Future** — pairs with P3-30. | Keystone token validation; optional feature flag. |
| ~~P3-32~~ | ~~Bitrot scanner & healing~~ | storage | L | **Promoted to P1-25**. | — |
| P3-33 | Lifecycle transitions & non-current expiry | storage | M | **P1-ENT** — extend P3-01 expiration. | `transition_days`; `noncurrent_expiration_days`; versioned purge tests. |
| P3-34 | S3 bucket replication API (CRR) | storage | XL | **Deferred** — geo-DR after P1-14; builds on mutation log. | `PutBucketReplication` / `Get` / `Delete`; primary→standby test; lag metrics. |
| P3-35 | External KMS (SSE-KMS compatible) | security | L | **P1-ENT** — on-prem Vault/HSM only in airgap. | Pluggable KMS; Vault transit; `aws:kms` path; offline KMS bootstrap doc. |
| P3-36 | Published S3 compatibility matrix | docs | S | **P1-GA** — procurement / integration gate. | Matrix in repo; CI sync with tests; linked from README. |
| P3-37 | Observability reference stack | ops | M | **P1-GA** — on-prem only; images via P3-55 private registry. | `deploy/compose/observability.yml`; offline image manifest; Grafana JSON in repo. |
| P3-38 | OIDC claims in bucket policies | auth | L | **P1-ENT** — after P3-08 + P3-28. | `jwt:groups` / `jwt:roles` conditions; Entra/Keycloak example. |
| P3-39 | S3 server access logging | api | M | **P1-ENT** — per-bucket log delivery. | `?logging` config; deliver to target bucket; integration test. |
| P3-40 | Storage API fuzz testing in CI | ci | M | **Future** — nightly acceptable. | Fuzz harness for SigV4, paths, policy; seed corpus. |
| P3-41 | Offline bare-metal install helper | ops | S | **Future** — wraps **P3-54** bundle (not internet `curl \| bash`). | `scripts/install-maxio-offline.sh`; verifies `SHA256SUMS`; installs binary + systemd stub from local path only. |
| P3-42 | Optional native TLS termination | ops | M | **Future** — edge/single-node convenience. | `--tls-cert` / `--tls-key`; proxy path remains default. |
| P3-43 | RustFS / enterprise parity epic | storage | XL | **P1-ENT** — see Enterprise GA+ tiers above. | Tier 2–3 complete after GA. |
| P3-44 | Production GA milestone | ops | M | **Milestone** — closes when **P3-52** + **P3-53** done. | P1-14 + airgap + GA rows; README warning removed; Helm not required. |
| P3-45 | Cilium eBPF deployment guide | ops | M | **Future** — plain K8s YAML; after P1-24. See `docs/plans/2026-06-29-cilium-ebpf-deployment.md`. | Doc + `deploy/k8s/` examples; server-tier LB notes. |
| P3-46 | Multi-node Service topology (K8s) | ops | M | **Future** — safe Service patterns for P1-14 tiers. | Plain YAML per tier; NetworkPolicy examples. |
| P3-47 | Object lock / WORM / legal hold | storage | L | **P1-ENT** — immutability for regulated industries. | Governance retention; legal hold API subset; tests; docs vs versioning. |
| P3-48 | Backup automation & verified restore | ops | M | **P1-GA** — offline/removable media; no cloud backup. | Backup script or `maxio-admin backup`; checksum verify; offline restore drill. |
| P3-49 | Disaster recovery runbook (RPO/RTO) | docs | M | **P1-GA** — formal DR for single-node and cluster. | DR section in `docs/operations.md`; failover drills; tied to P1-24. |
| P3-50 | Security audit & hardening checklist | security | M | **P1-GA** — pen-test prep incl. airgap supply chain. | `docs/security-audit.md`; threat model; P3-59 egress; SBOM review from P3-54. |
| P3-51 | Production SLA & incident response | ops | S | **P1-GA** — support and on-call playbook. | SLA section in `docs/operations.md`; severity levels; key metrics. |
| P3-52 | Enterprise GA epic | ops | L | **P1-GA epic** — requires **P3-53** airgap + Enterprise GA rows + P1-14. | P3-44 milestone closes. |
| P3-53 | Airgap deployment epic | ops | L | **P1-GA epic** — primary enterprise install path. See Airgap section. | P3-54–P3-60 complete; airgap checklist signed off. |
| P3-54 | Offline release bundle | ci | M | **P1-GA / airgap** — transferable BM artifact. | See Airgap section. |
| P3-55 | Offline container image pack | ci | M | **P1-GA / airgap** — K8s private registry ingest. | See Airgap section. |
| P3-56 | Airgap deployment runbook | docs | M | **P1-GA / airgap** — no-internet install guide. | See Airgap section. |
| P3-57 | Internal CA & TLS | ops | S | **P1-GA / airgap** — org PKI, no ACME. | See Airgap section. |
| P3-58 | Offline upgrade & rollback bundles | ops | M | **P1-GA / airgap** — patch without upstream access. | See Airgap section. |
| P3-59 | Runtime egress & dependency matrix | docs | S | **P1-GA / airgap** — document optional outbound only. | See Airgap section. |
| P3-60 | Private-registry K8s manifests | ops | M | **P1-GA / airgap** — no public registry defaults. | See Airgap section. |

### Capability map (reference)

| Capability | Backlog |
|------------|---------|
| Multi-node cluster | **P1-14** → P1-17, P1-20, P1-21, P1-24 |
| Distributed EC | P1-18, P1-19, P1-25 |
| **Airgap deploy** | **P3-53** → P3-54–P3-60 (required for GA) |
| Enterprise GA | **P3-52** → P3-53 + P3-18, P3-26, P3-36, P3-37, P3-08, P3-06, P3-48–51, P3-24, P3-05 |
| Regulated / multi-tenant | **P3-43** → P3-28, P3-35, P3-29, P3-38, P3-39, P3-27, P3-47, P3-33 |
| Geo-DR / CRR | Deferred — P3-34, P3-09 |
| Edge LB (Pingora) | [knx-edge](https://github.com/smartedge-in/knx-edge) (P3-25 moved) |
| Helm | P3-19 (future) |
| Cilium eBPF | P3-45, P3-46 (future) |

---

## Completed / already in good shape

Reference only — no backlog action unless regressions appear.

| Area | Status |
|------|--------|
| AWS SigV4 + presigned URLs | Solid; constant-time compares, skew checks |
| Path traversal / key validation | `validate_key()`, reserved segments |
| SSE-S3 / SSE-C / keyring rotation CLI | Implemented with AAD-bound frames |
| Default credential production guard | Refuses `maxioadmin` without `--allow-insecure-dev` |
| Docker non-root runtime | `USER maxio` in Dockerfile |
| Airgap-friendly core | Single static binary; UI embedded; no runtime DB or external service required |
| Integration test suite | ~160 tests in `tests/integration.rs`; `cargo test` in CI |
| Erasure coding (single-node) | Chunked PUT/GET, range reads, parity RS recovery, encrypt-then-EC, ~20 EC/parity integration tests |
| Public bucket anonymous access | Query sub-resource blocklist on bypass |
| Housekeeping | Stale multipart + temp file sweep |

---

## Suggested sprint order

**Phase 0 — Done (foundation):** P0, P1 security, P1 S3 compat, single-node EC, P2 ops tooling ✓

**Phase 1 — Multi-replica & distributed EC (product #1):**
P1-15 → P1-16 → P1-22 → P1-17 → P1-18 → P1-19 → P1-20 → P1-21 → P1-24 → P1-25 → **P1-14 close**
(parallel: P3-24 license gate for Raft dep)

**Phase 2 — Enterprise GA (airgap-first, single-region HA):**
**P3-54 → P3-55 → P3-56 → P3-57 → P3-58 → P3-59 → P3-60 → P3-53** → P3-18 → P3-26 → P3-36 → P3-37 → P3-08 → P3-06 → P3-48 → P3-49 → P3-50 → P3-51 → P3-24 → P3-05 → **P3-52 / P3-44 close**

**Phase 3 — Enterprise GA+ (regulated / multi-tenant):**
P3-28 → P3-35 → P3-29 → P3-38 → P3-39 → P3-27 → P3-47 → P3-33 → **P3-43 close**

**Phase 4 — Future / optional:**
P3-34 CRR · P3-09 operator sync · P3-19 Helm · P3-45/46 Cilium · P3-40 fuzz · P3-41 install · P3-42 native TLS · P3-30/31 OpenStack
(parallel anytime: P3-17 admin CLI boundary, P3-23 crate boundary CI)

---

## How to use this file

1. Move items to `docs/plans/` when design work starts.
2. Link PRs in commit messages: `fix(storage): P0-03 enforce max object size`.
3. Re-prioritize after production pilot feedback.