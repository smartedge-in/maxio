# MaxIO Backlog

Actionable backlog derived from codebase review (2026-06-28). Items are ordered by priority within each tier.

**Legend**

| Field | Meaning |
|-------|---------|
| **Priority** | P0 = production blocker · P1 = security/reliability · **P1-MR = multi-replica & EC (product #1)** · P2 = maintainability · P3 = nice-to-have / deferred |
| **Effort** | S (< 1 day) · M (1–3 days) · L (3–7 days) · XL (> 1 week) |
| **Area** | ci · ops · security · storage · api · auth · ui · docs |

---

## Priority 1 — Multi-replica architecture & erasure coding (Raft-first)

**Product direction:** Live multi-node cluster via **dual independent Raft** (storage + server) and **distributed erasure coding** across storage nodes. No operator-sync detour on the critical path. Plan: `docs/plans/2026-06-29-multi-replica-raft-priority.md`.

**Do not:** scale `Deployment.replicas` on one RWO PVC; round-robin LB across uncordinated MaxIO instances.

| Order | ID | Title | Area | Effort | Description | Acceptance criteria |
|-------|-----|-------|------|--------|-------------|-------------------|
| — | **P1-14** | **Multi-replica epic (Raft-first)** | storage | XL | **Epic** — closes when P1-15–P1-21 and P1-24 done. Supersedes P3-13 as primary scale-out track. Asymmetric tiers: UI (none), server (Raft), storage (Raft + EC). Helm deferred (P3-19). | 3-node storage quorum + failover test; distributed EC rebuild test; 2+ server + 2+ UI replicas; single-node colocated mode preserved; epic doc acceptance checklist complete. |
| 1 | P1-15 | `StorageBackend` trait | storage | L | Extract `FilesystemStorage` behind a trait; **all** metadata and object mutations go through it. Prerequisite for Raft apply path. *Replaces P3-10 on critical path.* | Trait in `maxio-storage`; server uses `Arc<dyn StorageBackend>`; existing integration tests pass unchanged; no direct `FilesystemStorage` in handlers. |
| 2 | P1-16 | Raft library spike & license gate | storage | S | Evaluate `openraft` / `raft-rs` (or alternatives); **must** pass P3-24 permissive-only `cargo deny`. Document choice in plan. | Spike doc in `docs/plans/`; chosen crate in workspace with deny.toml entry; minimal echo cluster test in CI (optional feature flag). |
| 3 | P1-22 | `maxio-common` crate | storage | M | Cluster RPC types, `VERSION`, routing snapshot DTOs, shared constants. No `axum`/`reqwest`/storage I/O. *Moved ahead of P3-21 epic.* | `crates/maxio-common`; server + storage + admin import shared types; forbidden-deps documented. |
| 4 | P1-17 | Storage tier Raft | storage | XL | Metadata consensus: buckets, object index, multipart, bucket settings, keyring epoch. Object bytes local per node. *Was P3-14.* | 3-node bootstrap/join; writes via leader; follower failover integration test; metrics `raft_storage_leader`, `raft_storage_commit_lag`. |
| 5 | P1-18 | Distributed erasure coding | storage | XL | Spread EC data/parity **shards across storage nodes** (not single-host `.ec` only). Shard map is Raft metadata. Builds on single-node EC (shipped) + per-bucket toggle (P3-07). | Placement policy (e.g. K data + M parity across distinct nodes); PUT stripes shards; shard map in Raft; integration test 3 storage nodes + EC object. |
| 6 | P1-19 | Multi-node EC read & rebuild | storage | L | On shard loss, fetch parity from peer storage nodes and reconstruct. Extends `VerifiedChunkReader` / RS path for remote shard RPC. | Read path pulls missing shard from peer; rebuild after one node down (with sufficient parity); tests with induced shard loss. |
| 7 | P1-20 | Server tier Raft | api | XL | Independent server quorum: membership, storage endpoint map, credential epoch. Stateless S3 workers use routing snapshot. *Was P3-15.* | 2+ server quorum; storage leader change reflected without manual config; `/readyz` reflects storage quorum; integration test with P1-17. |
| 8 | P1-21 | Stateless UI tier (`maxio-ui`) | ui | L | Extract embedded SPA; scale UI independently. *Was P3-16.* | `crates/maxio-ui`; distributed deploy; Ingress split `/ui` vs S3; ≥2 UI replicas in test manifest. |
| 9 | P1-24 | Multi-node CI / dev harness | ci | M | Automated 3-node (storage) cluster test in CI or documented `kind` recipe. Plain K8s YAML or bare-metal scripts — **not** Helm. | Script or CI job: bootstrap Raft, PUT/GET, kill leader, EC shard placement smoke; runs on main or nightly. |
| 10 | P1-25 | EC bitrot scanner (cluster-aware) | storage | L | Proactive shard checksum scan; heal from parity/peers. *Elevated from P3-32 for EC priority.* | Background scanner; cross-node heal when local shard corrupt; Prometheus counters; ops tuning doc. |

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

## P3 — Nice-to-have / future

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| ~~P3-01~~ | ~~Lifecycle rules (expiration)~~ | storage | L | Done — prefix-based `LifecycleRule` on `BucketMeta`; `PUT/GET/DELETE ?lifecycle`; hourly housekeeping sweep expires non-versioned objects. | — |
| ~~P3-02~~ | ~~Replication / federation~~ | storage | XL | Done (RFC) — `docs/plans/2026-06-29-replication-federation.md`; implementation deferred to Phase 1+ runbook. | — |
| ~~P3-03~~ | ~~SQLite metadata index~~ | storage | L | Done — `MAXIO_METADATA_INDEX` enables `{data_dir}/.maxio-metadata.db`; upsert on write/delete; rebuild on startup; walk fallback. | — |
| ~~P3-04~~ | ~~Workspace crate split~~ | storage | L | Done — `crates/maxio-storage` (filesystem, crypto, keys, policy, quota) and `crates/maxio-server` (HTTP/S3 API, auth, embedded UI); root `maxio` is a facade binary + re-exports; `map_storage_upload_error` lives in server `error` module. | — |
| P3-05 | ARM64 release binaries | ci | S | Release workflow may be x86-only today. | Multi-arch Docker image and GitHub release assets. |
| P3-06 | UI E2E tests (Playwright) | ui | M | No browser-level tests for upload/download flows. | Smoke test: login → create bucket → upload → download → delete. |
| ~~P3-07~~ | ~~Per-bucket erasure coding toggle~~ | storage | L | Done — `BucketMeta.erasure_coding` override; `PUT/GET ?erasure`; writes use `effective_erasure_coding()`; reads layout-based; existing flat objects unchanged. | — |
| P3-08 | Keycloak console UI login | ui | M | Server exposes `/api/auth/keycloak-config`, `keycloak-login`, and `keycloak-refresh`; Svelte console still uses access/secret login only. | `Login.svelte` reads `keycloak-config` on load; when enabled, username/password form calls `keycloak-login` and reuses cookie session; silent refresh before expiry; access/secret form hidden when Keycloak-only; Playwright smoke test for Keycloak path (extends P3-06). |
| ~~P3-09~~ | ~~Replication Phase 1 — operator sync runbook~~ | ops | M | **Deferred** — not on Raft-first critical path (see Priority 1). May revisit for geo-DR tooling after P1-14. | — |
| ~~P3-10~~ | ~~Replication Phase 2 — mutation event log~~ | storage | XL | **Superseded by P1-15** — `StorageBackend` trait is the Raft-first entry; optional append-only log deferred unless needed for audit. | — |
| ~~P3-11~~ | ~~Replication Phase 3 — replication agent~~ | storage | XL | **Deferred** — `maxio-replicate` sidecar not on critical path; Raft + distributed EC is Priority 1. | — |
| ~~P3-12~~ | ~~Multiple replicas support (operator epic)~~ | storage | XL | **Superseded by P1-14** — multi-replica epic is Raft-first, not rsync/agent. | — |
| ~~P3-13~~ | ~~Asymmetric scale-out with dual Raft (epic)~~ | storage | XL | **Superseded by P1-14** — same architecture, now Priority 1. | — |
| ~~P3-14~~ | ~~Storage tier Raft consensus~~ | storage | XL | **Superseded by P1-17**. | — |
| ~~P3-15~~ | ~~Server tier Raft consensus~~ | api | XL | **Superseded by P1-20**. | — |
| ~~P3-16~~ | ~~UI crate — stateless scale-out~~ | ui | L | **Superseded by P1-21**. | — |
| P3-17 | Admin CLI crate boundary | ops | M | `maxio-admin` is a workspace member (P2-12) but depends on root `maxio` facade, coupling the CLI to server+storage re-exports. Must be a standalone crate: `maxio-storage` for local `--data-dir` commands only; `reqwest` for remote API — never `maxio-server`. Stateless operator client, not a cluster tier. See `docs/plans/2026-06-29-admin-cli-crate.md`. | No `maxio` or `maxio-server` path dep in `maxio-admin/Cargo.toml`; local doctor/keyring use `maxio-storage` directly; separate release binary/artifact; crate-boundary CI check; docs updated in `docs/operations.md`. |
| P3-18 | Bare metal deployment pack | ops | M | MaxIO must support native Linux bare-metal/VM installs, not only containers. TLS/LB via **permissive** edge only (Caddy/Traefik — P3-26); **no keepalived/nginx** in official runbooks. See `docs/plans/2026-06-29-deployment-targets.md`. | `deploy/systemd/maxio.service`; bare-metal section with Caddy example; multi-host LB without GPL VIP; P3-26 aligned; smoke test via `maxio healthcheck`. |
| P3-19 | Kubernetes Helm chart | ops | L | **Future improvement** — not required for P1-14 or GA. Today only a minimal Deployment YAML snippet exists in `docs/operations.md`. See `docs/plans/2026-06-29-deployment-targets.md`. | Official `deploy/helm/maxio` chart; single-node + `values-distributed.yaml` for P1-14 tiers; `helm lint` + `helm template` in CI; README section. |
| P3-20 | Deployment targets epic (bare metal + K8s) | ops | L | **Epic** — bare metal first (P3-18); K8s via plain YAML/manifests for P1-14; **Helm optional** (P3-19 future). | P3-18 complete; distributed scale-out documented with BM + plain K8s examples; P3-19 optional follow-on. |
| P3-21 | Shared library strategy (epic) | storage | M | **Epic** — thin shared types without a monolithic “god crate”. `maxio-storage` remains storage SSOT; new `maxio-common` for cross-component contracts; root facade not a sibling dependency (P3-17). UI stays npm-only. See `docs/plans/2026-06-29-shared-libraries.md`. | P3-22 + P3-23 + P3-17 complete; dependency graph documented; no `axum`/`reqwest` in `maxio-common`. |
| ~~P3-22~~ | ~~`maxio-common` crate~~ | storage | M | **Promoted to P1-22** on Priority 1 critical path. | — |
| P3-23 | Crate boundary CI enforcement | ci | S | Automate dependency rules from P3-04 / shared-library plan: e.g. `maxio-admin` must not depend on `maxio` or `maxio-server`; `maxio-common` must not depend on `axum` or `maxio-storage`. | `cargo deny` bans or CI script fails on forbidden edges; documented in `docs/plans/2026-06-29-shared-libraries.md`; passes on current graph after P3-17/P3-22. |
| P3-24 | Permissive-only license policy (mandatory) | ci | S | **Requirement** — no copyleft, weak copyleft, or non-standard licenses in production artifacts; prefer Apache-2.0/MIT for new deps. Partially enforced via `deny.toml` + CI `licenses` job; npm/UI path needs explicit check. See `docs/licensing.md`. | `docs/licensing.md` mandatory section + PR checklist; `deny.toml` documents forbidden categories; `make deny` in CI (existing); add `ui/` npm license audit step (allow-list Apache/MIT/BSD/ISC/0BSD/CC0); contributors reference policy in README or CONTRIBUTING. |
| ~~P3-25~~ | ~~Optional edge LB (`maxio-edge` / Pingora)~~ | ops | L | **Moved to [knx-edge](https://github.com/smartedge-in/knx-edge)** — out of tree. MaxIO uses **Caddy** (Apache-2.0), **Traefik** (MIT), or K8s **MetalLB** / Ingress (P3-26); embedded Pingora in `maxio-server` rejected. See `docs/out-of-tree/knx-edge.md`. | — |
| P3-26 | Permissive ingress & HA runbook | ops | S | **Requirement** — official MaxIO deployment docs must not prescribe GPL edge/HA (keepalived, HAProxy CE). Prefer Caddy, Traefik, Envoy, MetalLB, kube-vip. See `docs/plans/2026-06-29-permissive-ingress-ha.md`. | Operations + P3-18/19 examples use Caddy; GPL tools marked not recommended; HA patterns without keepalived documented. |

### RustFS parity gaps

Items below close feature and ops gaps identified vs [RustFS](https://github.com/rustfs/rustfs) (2026-06). Overlaps with existing epics (P3-09–16, P3-19) are called out; new work is additive. Tracked under epic **P3-43**.

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P3-27 | S3 event notifications | api | L | RustFS ships webhook/SQS-style event delivery on object/bucket mutations. MaxIO has audit log only — no subscriber integrations. | `PUT/GET/DELETE ?notification` (or equivalent config API); at least **webhook** target; durable queue/spool on disk; `ObjectCreated` / `ObjectRemoved` / `BucketCreated` event types; integration test with test HTTP receiver; env-config quickstart in `docs/operations.md`. |
| P3-28 | IAM bucket policies v2 | api | L | MaxIO policy v1 is Allow-only, `Principal:*`, two actions. RustFS supports IAM-style `2012-10-17` policies with Deny, conditions, and richer actions. | Parser/evaluator for Deny precedence, `Condition` operators (`StringEquals`, `IpAddress`, etc.), expanded action set (`s3:*` subset documented); integration tests for deny-wins and condition match; `docs/s3-compatibility.md` updated. |
| P3-29 | Multi-tenancy | auth | L | RustFS has first-class multi-tenant isolation. MaxIO has multiple static credentials in `.maxio-credentials.json` but no tenant boundary on buckets or admin scope. | Tenant ID on buckets and credentials; requests scoped to tenant; admin API lists only tenant buckets; cross-tenant access denied; migration path for single-tenant deployments (default tenant). |
| P3-30 | OpenStack Swift API | api | XL | RustFS exposes Swift object API alongside S3. MaxIO is S3-only. | Swift `PUT/GET/HEAD/DELETE` object paths; container listing; `X-Auth-Token` auth path or documented bridge to Keystone (P3-31); compatibility matrix row per endpoint; integration smoke test. |
| P3-31 | OpenStack Keystone authentication | auth | L | RustFS integrates Keystone for Swift and console. MaxIO has Keycloak for console (P3-08) but not Keystone/`X-Auth-Token`. | Keystone token validation middleware; configurable Keystone URL; Swift and/or S3 requests accept valid service tokens; docs for OpenStack deployment; optional dependency feature flag. |
| ~~P3-32~~ | ~~Bitrot scanner & healing~~ | storage | L | **Promoted to P1-25** (cluster-aware EC scanner on Priority 1). | — |
| P3-33 | Lifecycle transitions & non-current expiry | storage | M | P3-01 covers prefix **expiration** only. RustFS lifecycle includes transitions and non-current version rules (under broader lifecycle). | `LifecycleRule` supports `transition_days` / storage class stub and `noncurrent_expiration_days` for versioned buckets; housekeeping applies rules; `PUT/GET ?lifecycle` schema versioned; tests for versioned non-current purge. |
| P3-34 | S3 bucket replication API (CRR) | storage | XL | RustFS has **bucket replication** available. MaxIO defers to operator sync (P3-09) and internal log/agent (P3-10/11) without S3 CRR XML. | `PutBucketReplication` / `GetBucketReplication` / `DeleteBucketReplication`; replicate to standby MaxIO endpoint; replication status headers on objects; lag metrics; builds on P3-10 event log; integration test primary→standby; extends P3-12 epic. |
| P3-35 | External KMS (SSE-KMS compatible) | security | L | RustFS is adding KMS. MaxIO supports SSE-S3/SSE-C only and rejects AWS SSE-KMS requests today. | Pluggable KMS backend trait; at least one backend (e.g. HashiCorp Vault transit or static key service); `aws:kms` SSE header path documented; keys never logged; deny.toml-clean deps; integration test encrypt/decrypt roundtrip. |
| P3-36 | Published S3 compatibility matrix | docs | S | RustFS maintains `docs/architecture/s3-compatibility-matrix.md`. MaxIO has narrative docs only. | Checked-in matrix (operation × status: supported / partial / N/A); CI or PR checklist keeps matrix in sync with integration tests; linked from README and `docs/s3-compatibility.md`. |
| P3-37 | Observability reference stack | ops | M | RustFS documents Compose profiles with Grafana, Prometheus, Jaeger. MaxIO exposes `/metrics` but no reference dashboards or tracing. | `deploy/compose/observability.yml` (or Helm subchart) scraping MaxIO metrics; sample Grafana dashboard JSON; optional OpenTelemetry trace export doc; README quickstart. |
| P3-38 | OIDC claims in bucket policies | auth | L | RustFS maps OIDC `groups`/`roles` (e.g. Entra `roles_claim`) into IAM evaluation. MaxIO Keycloak (P3-08) is console-only — policies do not see JWT claims. | After P3-08 + P3-28: policy conditions on `jwt:groups`, `jwt:roles` (configurable claims); console admin role via policy; Entra/Keycloak example in docs; integration test with mock OIDC claims. |
| P3-39 | S3 server access logging | api | M | RustFS supports access logging. MaxIO has structured audit log to stderr but not S3-style per-bucket delivery to another bucket. | `PUT/GET/DELETE ?logging` per bucket; log lines (combined log format or S3 canonical); deliver to target bucket prefix; configurable enable; integration test validates log object creation. |
| P3-40 | Storage API fuzz testing in CI | ci | M | RustFS includes `fuzz/` targets. MaxIO relies on integration tests only. | `cargo-fuzz` or libFuzzer harness for SigV4 parser, path/key validation, policy parser; CI job on main (nightly acceptable); seed corpus committed; documented in `CLAUDE.md`. |
| P3-41 | One-click bare-metal install script | ops | S | RustFS provides `curl \| bash` installer. MaxIO documents manual build/Docker only. | `scripts/install-maxio.sh` (or documented curl pipe) for Linux amd64/arm64; installs binary + systemd unit stub; checksum verification; linked from README. |
| P3-42 | Optional native TLS termination | ops | M | RustFS supports `RUSTFS_TLS_PATH`. MaxIO requires external Caddy/Traefik (P3-26). Optional native TLS reduces moving parts for edge/single-node. | `--tls-cert` / `--tls-key` or env equivalents; TLS on S3 and console listeners; documented as optional (proxy path remains default); integration test with self-signed cert. |
| P3-43 | RustFS parity epic | storage | XL | **Epic** — competitive gaps vs RustFS; **subordinate to P1-14** (multi-replica ships first). | Tier 1 (ops/docs): P3-36, P3-37, P3-41, P3-05. Tier 2 (S3 product): P3-27, P3-28, P3-33, P3-39. Tier 3 (enterprise): P3-29, P3-35, P3-38. Tier 4 (protocol): P3-30, P3-31, P3-34. Epic closes when Tier 1–3 complete after P1-14. |
| P3-44 | Production GA milestone | ops | M | README warns against production use. | Remove dev-only warning when criteria met: **P1-14** + P3-18 + P3-26 done; P3-06 smoke tests green; security audit checklist; CHANGELOG GA entry; `docs/operations.md` production SLA section. Helm (P3-19) not required. |

### Kubernetes / Cilium (eBPF)

MaxIO on Kubernetes with [Cilium](https://cilium.io/) as CNI: use eBPF for datapath throughput and observability. **eBPF does not replace Raft** — pair with **P1-14** distributed tiers. See `docs/plans/2026-06-29-cilium-ebpf-deployment.md`.

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P3-45 | Cilium eBPF deployment guide | ops | M | Document MaxIO on Cilium: kube-proxy replacement, eBPF Service LB, Gateway/Ingress TLS, optional WireGuard, Hubble. Plain K8s YAML examples — no Helm required. | Doc in `docs/operations.md`; example manifests under `deploy/k8s/`; server-tier LB for P1-20; streaming upload notes. |
| P3-46 | Multi-node Service topology (K8s) | ops | M | Safe Service patterns for **P1-14** tiers — not single-PVC multi-replica. | Plain YAML per tier in `deploy/k8s/`; NetworkPolicy examples; documents unsafe combined-stack multi-replica. |

**Suggested Cilium order:** P1-24 harness → P3-45 → P3-46. Helm (P3-19) optional later.

**RustFS parity — already covered elsewhere (no new ID)**

| RustFS capability | MaxIO backlog |
|-------------------|---------------|
| Distributed / multi-node mode | **P1-14** → P1-17, P1-20, P1-21 |
| Operator + agent replication | Deferred (P3-09–P3-11) |
| Kubernetes Helm chart | P3-19 (future; plain K8s YAML for P1-14) |
| ARM64 images | P3-05 |
| OIDC console login | P3-08 (+ P3-38 for IAM) |
| Lifecycle (basic expiration) | ~~P3-01~~ (extend via P3-33) |
| Erasure coding | Single-node shipped; **distributed EC via P1-18, P1-19, P1-25** |
| Prometheus metrics | ~~P2-07~~ (dashboards via P3-37) |
| Versioning, multipart, encryption | Shipped |

**Suggested RustFS parity order:** P3-36 → P3-27 → P3-28 → P3-33 → P3-32 → P3-37 → P3-34 (with P3-10) → P3-29 → P3-30/31 (if OpenStack required).

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
| Integration test suite | ~160 tests in `tests/integration.rs`; `cargo test` in CI |
| Erasure coding (single-node) | Chunked PUT/GET, range reads, parity RS recovery, encrypt-then-EC, ~20 EC/parity integration tests |
| Public bucket anonymous access | Query sub-resource blocklist on bypass |
| Housekeeping | Stale multipart + temp file sweep |

---

## Suggested sprint order

**Completed (foundation):** P0, P1 security, P1 S3 compat, single-node EC (P1-12/13, P2-09/10/11), P2 ops tooling ✓

**Active — Priority 1 (multi-replica & distributed EC):** P1-15 → P1-16 → P1-22 → P1-17 → P1-18 → P1-19 → P1-20 → P1-21 → P1-24 → P1-25 → **P1-14 epic close**

**After P1-14:** P3-18 (bare metal), P3-24, P3-08, P3-43 RustFS parity (subordinate), P3-44 GA. **Future:** P3-19 Helm, P3-45/46 Cilium polish.

---

## How to use this file

1. Move items to `docs/plans/` when design work starts.
2. Link PRs in commit messages: `fix(storage): P0-03 enforce max object size`.
3. Re-prioritize after production pilot feedback.