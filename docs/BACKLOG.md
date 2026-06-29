# MaxIO Backlog

Actionable backlog derived from codebase review (2026-06-28). Items are ordered by priority within each tier.

**Legend**

| Field | Meaning |
|-------|---------|
| **Priority** | P0 = production blocker · P1 = security/reliability · P2 = maintainability · P3 = nice-to-have |
| **Effort** | S (< 1 day) · M (1–3 days) · L (3–7 days) · XL (> 1 week) |
| **Area** | ci · ops · security · storage · api · auth · ui · docs |

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
| P3-09 | Replication Phase 1 — operator sync runbook | ops | M | Phase 0 RFC done (P3-02). Phase 1 adds no replication daemon: operators use `rclone`/`rsync` for active-passive DR. See `docs/plans/2026-06-29-replication-federation.md` § Phase 1. | `docs/operations.md` runbook (sync, keyring/credential coordination, multipart quiesce, failover checklist); paginated bucket inventory export via admin API or `maxio-admin`; integration test for inventory endpoint. |
| P3-10 | Replication Phase 2 — mutation event log | storage | XL | Introduce `StorageBackend` trait and append-only replication log on every mutating storage op; builds on metadata index patterns (P3-03). See RFC § Phase 2. | `MAXIO_REPLICATION_LOG` enables durable log; event schema covers ObjectPut/Delete and bucket meta changes; idempotent replay helper + unit tests; Prometheus lag/sequence metrics. |
| P3-11 | Replication Phase 3 — replication agent | storage | XL | Sidecar `maxio-replicate` tails Phase 2 log and applies changes to standby `data_dir` (active-passive). See RFC § Phase 3. | Agent with checkpoint, bucket include/exclude, lag metrics/alerts; primary PUT → agent → standby GET integration test; failover/failback steps added to operations guide. |
| P3-12 | Multiple replicas support | storage | XL | **Epic** — MaxIO is single-node today; scaling Kubernetes `replicas>1` without coordinated storage is not supported. This item tracks active-passive multi-node deployments (primary + standby/read replica), not erasure-coding parity shards or active/active multi-master. Implementation path: P3-09 → P3-10 → P3-11 (`docs/plans/2026-06-29-replication-federation.md`). | ≥1 standby replica stays within configurable lag of primary; failover runbook validated in docs; `maxio-replicate` agent shipped; integration test: write primary → read standby; explicit non-goals documented (no S3 CRR XML, no multi-master writes). Closes when P3-09, P3-10, and P3-11 are done. |
| P3-13 | Asymmetric scale-out with dual Raft (epic) | storage | XL | **Epic** — P3-04 split is compile-time only. Scale architecture **must** use **two independent Raft consensus groups** (storage + server) plus a **stateless UI tier** (P3-16). See `docs/plans/2026-06-29-distributed-scale-raft.md` and `docs/plans/2026-06-29-ui-scale-out.md`. Depends on P3-10 (`StorageBackend`); distinct from interim DR track P3-09–P3-11. | Storage Raft (P3-14), Server Raft (P3-15), and stateless UI crate (P3-16) shipped; independent replica counts per tier (e.g. 3 UI, 3 server, 5 storage); colocated single-node mode remains default. Closes when P3-14, P3-15, and P3-16 are done. |
| P3-14 | Storage tier Raft consensus | storage | XL | Raft cluster inside `maxio-storage`: replicates bucket/object metadata, multipart state, bucket settings, keyring epoch. Object bytes stay on local FS per node; metadata mutations go through Raft leader. Phase A in distributed-scale-raft plan. | 3-node storage quorum bootstrap/join; metadata write via leader; follower failover < configured SLA in integration test; `StorageBackend` routes mutations through Raft; metrics: `raft_storage_leader`, `raft_storage_commit_lag`. |
| P3-15 | Server tier Raft consensus | api | XL | **Independent** Raft cluster inside `maxio-server`: replicates server membership, storage endpoint map, credential fingerprint epoch, admin routing generation. Any server member serves S3 data plane against storage tier; control writes go through server Raft leader. Phase B in distributed-scale-raft plan. | 2+ server quorum; routing snapshot replicated; storage leader change propagated without manual config; `/readyz` reflects storage quorum reachability; integration test with P3-14 cluster; metrics: `raft_server_leader`, `raft_server_commit_lag`. |
| P3-16 | UI crate — stateless scale-out | ui | L | Console SPA (`ui/`) is embedded in `maxio-server` via `rust-embed`; cannot scale UI independently. Extract `crates/maxio-ui` static asset server; UI pods hold no session state (auth cookies from API tier only). No Raft on UI tier. See `docs/plans/2026-06-29-ui-scale-out.md`. | `maxio-ui` workspace crate serves `ui/build`; distributed deploy removes embed from server; configurable API base URL for split ingress; K8s manifest with UI Deployment ≥2 replicas; Playwright smoke test via UI load balancer. |
| P3-17 | Admin CLI crate boundary | ops | M | `maxio-admin` is a workspace member (P2-12) but depends on root `maxio` facade, coupling the CLI to server+storage re-exports. Must be a standalone crate: `maxio-storage` for local `--data-dir` commands only; `reqwest` for remote API — never `maxio-server`. Stateless operator client, not a cluster tier. See `docs/plans/2026-06-29-admin-cli-crate.md`. | No `maxio` or `maxio-server` path dep in `maxio-admin/Cargo.toml`; local doctor/keyring use `maxio-storage` directly; separate release binary/artifact; crate-boundary CI check; docs updated in `docs/operations.md`. |
| P3-18 | Bare metal deployment pack | ops | M | MaxIO must support native Linux bare-metal/VM installs, not only containers. TLS/LB via **permissive** edge only (Caddy/Traefik — P3-26); **no keepalived/nginx** in official runbooks. See `docs/plans/2026-06-29-deployment-targets.md`. | `deploy/systemd/maxio.service`; bare-metal section with Caddy example; multi-host LB without GPL VIP; P3-26 aligned; smoke test via `maxio healthcheck`. |
| P3-19 | Kubernetes Helm chart | ops | L | MaxIO must support first-class Kubernetes deployment. Today only a minimal Deployment YAML snippet exists; no Helm chart, values, or CI validation. See `docs/plans/2026-06-29-deployment-targets.md`. | Official `deploy/helm/maxio` chart; single-node profile (`replicas: 1`, PVC, probes, Ingress, Secret); `helm lint` + `helm template` in CI; README deployment section; `values-distributed.yaml` stub for P3-13 tiers; document `replicas: 1` constraint until Raft tiers ship. |
| P3-20 | Deployment targets epic (bare metal + K8s) | ops | L | **Epic** — MaxIO supports **bare metal** and **Kubernetes** as equal production targets (P3-18 + P3-19). Docker image remains packaging; operators choose BM or K8s without forked docs. | P3-18 and P3-19 complete; README links both paths; distributed scale-out docs (P3-13) reference BM multi-host and Helm values overlay. |
| P3-21 | Shared library strategy (epic) | storage | M | **Epic** — thin shared types without a monolithic “god crate”. `maxio-storage` remains storage SSOT; new `maxio-common` for cross-component contracts; root facade not a sibling dependency (P3-17). UI stays npm-only. See `docs/plans/2026-06-29-shared-libraries.md`. | P3-22 + P3-23 + P3-17 complete; dependency graph documented; no `axum`/`reqwest` in `maxio-common`. |
| P3-22 | `maxio-common` crate | storage | M | Thin workspace crate: `VERSION`, admin API JSON types (server handlers + `maxio-admin` client), shared `MAXIO_*` constant names. No HTTP, storage I/O, or framework deps. | `crates/maxio-common` in workspace; server + admin use shared admin DTOs; version from single module; `deny.toml` or README lists forbidden deps for common. |
| P3-23 | Crate boundary CI enforcement | ci | S | Automate dependency rules from P3-04 / shared-library plan: e.g. `maxio-admin` must not depend on `maxio` or `maxio-server`; `maxio-common` must not depend on `axum` or `maxio-storage`. | `cargo deny` bans or CI script fails on forbidden edges; documented in `docs/plans/2026-06-29-shared-libraries.md`; passes on current graph after P3-17/P3-22. |
| P3-24 | Permissive-only license policy (mandatory) | ci | S | **Requirement** — no copyleft, weak copyleft, or non-standard licenses in production artifacts; prefer Apache-2.0/MIT for new deps. Partially enforced via `deny.toml` + CI `licenses` job; npm/UI path needs explicit check. See `docs/licensing.md`. | `docs/licensing.md` mandatory section + PR checklist; `deny.toml` documents forbidden categories; `make deny` in CI (existing); add `ui/` npm license audit step (allow-list Apache/MIT/BSD/ISC/0BSD/CC0); contributors reference policy in README or CONTRIBUTING. |
| ~~P3-25~~ | ~~Optional edge LB (`maxio-edge` / Pingora)~~ | ops | L | **Dropped** — use **Caddy** (Apache-2.0), **Traefik** (MIT), or K8s **MetalLB** / Ingress (P3-26) instead; avoids GPL **keepalived** / HAProxy. Embedded Pingora in `maxio-server` also rejected. See `docs/plans/2026-06-30-pingora-edge-lb.md`. | — |
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
| P3-32 | Bitrot scanner & healing | storage | L | RustFS advertises bitrot protection with background scanning. MaxIO verifies chunks on read (EC) and uses sidecar HMACs but has no proactive scanner. | Background scanner job (housekeeping extension); configurable cadence and cycle budget; detect corrupt EC chunks / sidecar mismatch; auto-heal from parity when possible; Prometheus counters (`scanner_objects_checked`, `scanner_heal_total`); ops tuning doc. |
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
| P3-43 | RustFS parity epic | storage | XL | **Epic** — track closing competitive gaps vs RustFS without abandoning MaxIO principles (filesystem simplicity, permissive-only deps). Distinct from but overlaps P3-12 (replication), P3-13 (distributed), P3-19 (Helm). | Tier 1 (ops/docs): P3-36, P3-37, P3-41, P3-05. Tier 2 (S3 product): P3-27, P3-28, P3-33, P3-39. Tier 3 (enterprise): P3-29, P3-35, P3-38. Tier 4 (scale/protocol): P3-30, P3-31, P3-34 + P3-12/13. Tier 5 (data integrity): P3-32. Epic closes when Tier 1–3 complete and Tier 4 has RFC or shipped path. |
| P3-44 | Production GA milestone | ops | M | README warns against production use. RustFS ships beta/production releases with install/Helm paths. | Remove dev-only warning when criteria met: P3-18 + P3-19 + P3-26 done; P3-06 smoke tests green; security audit checklist; CHANGELOG GA entry; `docs/operations.md` production SLA section. |

**RustFS parity — already covered elsewhere (no new ID)**

| RustFS capability | MaxIO backlog |
|-------------------|---------------|
| Distributed / multi-node mode | P3-13 → P3-14, P3-15, P3-16 |
| Operator + agent replication | P3-09 → P3-10 → P3-11 → P3-12 |
| Kubernetes Helm chart | P3-19 (epic P3-20) |
| ARM64 images | P3-05 |
| OIDC console login | P3-08 (+ P3-38 for IAM) |
| Lifecycle (basic expiration) | ~~P3-01~~ (extend via P3-33) |
| Erasure coding | Shipped; scanner/healing via P3-32 |
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

**Sprint 1 (stabilize):** ~~P0-01~~, ~~P0-02~~, ~~P2-03~~, ~~P1-07~~ ✓
**Sprint 2 (harden):** ~~P0-03~~, ~~P1-01~~, ~~P1-02~~, ~~P0-04~~ ✓
**Sprint 3 (scale maintainability):** ~~P2-01~~, ~~P2-04~~, ~~P0-05~~, ~~P2-06~~ ✓
**Sprint 4 (erasure coding hardening):** ~~P1-13~~, ~~P2-11~~, ~~P2-09~~, ~~P2-10~~, ~~P1-12~~ ✓
**Sprint 5 (ops tooling):** ~~P2-13~~ (admin API), ~~P2-12~~ (CLI: profiles → remote status/info/doctor → housekeeping; local keyring rotate) ✓

---

## How to use this file

1. Move items to `docs/plans/` when design work starts.
2. Link PRs in commit messages: `fix(storage): P0-03 enforce max object size`.
3. Re-prioritize after production pilot feedback.