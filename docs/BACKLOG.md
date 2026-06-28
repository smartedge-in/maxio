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
| P1-01 | S3 API rate limiting | security | M | Only console login is rate-limited; S3 routes have no throttle. | Configurable limits on auth failures and/or PUT rate per IP; returns 429 with `Retry-After`. |
| P1-02 | Tighten console CSP | ui | M | CSP allows `'unsafe-inline'` for scripts and styles. | Hash- or nonce-based script policy where feasible; document any remaining inline exceptions. |
| P1-03 | Trusted proxy configuration | security | S | `X-Forwarded-For` is intentionally ignored for console login IP. | `MAXIO_TRUSTED_PROXIES` (CIDR list) enables correct client IP behind known load balancers only. |
| P1-04 | Secure bind defaults documentation | ops | S | Server binds `0.0.0.0` by default. | README warns about exposure; recommend `127.0.0.1` for dev and ingress-only for prod. |
| P1-05 | Session invalidation on credential rotate | auth | M | Console HMAC tokens remain valid for 7 days after access/secret change. | Document limitation or add token version keyed to credential hash; force re-login on rotate. |
| P1-06 | Distributed login rate limit | security | M | In-memory `LoginRateLimiter` does not work across replicas. | Optional Redis/shared store backend or document single-replica console requirement. |
| ~~P1-07~~ | ~~Presigned URL detection hardening~~ | auth | S | Done — `query_has_presigned_signature()` and case-insensitive `parse_presigned_query`; unit + integration tests. | — |
| P1-08 | Deep health metrics | ops | M | `/healthz` is liveness-only with no subsystem signal. | Optional `/healthz?verbose=1` or metrics export for housekeeping lag, disk free %, active uploads. |

---

## P1 — S3 compatibility & product

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P1-09 | Virtual-hosted-style requests | api | XL | Only path-style `/{bucket}/{key}` routing is supported. | `Host: bucket.endpoint` requests resolve correctly; integration tests for AWS SDK virtual-host mode. |
| P1-10 | Multi-user / IAM-style credentials | auth | XL | Single access/secret pair for entire server. | Design doc + phased implementation: multiple keys, or integration with external IdP. |
| P1-11 | Bucket policy engine | api | XL | Public read/list flags only; no JSON bucket policies. | Evaluate MinIO policy subset; document explicit non-goals for v1. |

---

## P1 — Erasure coding (reliability & coverage)

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P1-12 | Clean S3 errors on EC read failure | storage | M | When Reed-Solomon recovery fails or a chunk checksum mismatches without recoverable parity, reads often fail mid-stream (connection reset) instead of returning a structured S3/XML error. | `GetObject` / range reads return a deterministic HTTP error (e.g. 500 + `InternalError` or dedicated code) when chunk verification or RS reconstruction fails; integration test for `test_parity_too_many_failures`-style scenario asserts status/body, not only connection drop. |
| P1-13 | EC corruption tests in CI | ci | S | `aws-cli` job starts MaxIO without `--erasure-coding`, so `tests/aws_cli_test.sh` EC corruption checks are skipped (`INFO: erasure coding corruption tests skipped`). | Main-branch CI runs `aws_cli_test.sh` against a server with `--erasure-coding` (and optionally `--parity-shards`); corruption and recovery assertions execute, not skip. |

---

## P2 — Maintainability & code health

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P2-01 | Split `filesystem.rs` | storage | L | ~3,800-line monolith complicates review and change safety. | Extract modules: `object_io`, `multipart`, `encryption_io`, `listing`, `housekeeping`; no behavior change; tests pass. |
| P2-02 | Reduce crate-level clippy allows | storage | M | Broad `#![allow(clippy::...)]` in `main.rs` / `lib.rs` hides issues. | Remove allows file-by-file; fix or locally allow with justification. |
| ~~P2-03~~ | ~~Add `bun run check` to CI~~ | ci | S | Done — `bun run check` step in `.github/workflows/ci.yml` before frontend build. | — |
| P2-04 | Unit test coverage report in CI | ci | S | Coverage is unknown; integration tests dominate. | `cargo llvm-cov` or `cargo tarpaulin` job publishes summary; set minimum threshold for `storage/crypto`, `auth`. |
| P2-05 | Replace `unwrap()` in hot paths | storage | M | `unwrap`/`expect` in auth and storage error paths. | Audit and convert to `?` + proper `S3Error`/`StorageError` where user-visible. |
| P2-06 | Console API integration tests | api | M | Console routes lack dedicated integration coverage vs S3. | Tests for login, rate limit, presign, bucket settings JSON API. |
| P2-09 | Multipart + EC integration tests | storage | M | `complete_multipart_chunked` / `complete_multipart_chunked_encrypted` exist but multipart integration tests use the default non-EC server. | Tests under `start_server_ec()` / `start_server_ec_parity()`: create upload → upload parts → complete → GET roundtrip; SSE-S3 multipart variant covered. |
| P2-10 | CopyObject + EC integration tests | storage | S | Copy rewrites via `put_object` (chunks on EC-enabled servers) but no dedicated EC copy coverage. | With EC enabled: copy same-bucket and cross-bucket; destination is chunked on disk; GET roundtrip passes; SSE-S3 copy paths included. |
| P2-11 | Document EC operational limits | docs | S | Server-wide `--erasure-coding` toggle, parity required for recovery, GF(2⁸) 255-shard cap, and single-node scope are implicit in code but not summarized for operators. | `docs/operations.md` (or README) section: when to enable parity, max shards formula, no per-bucket EC, no recovery without parity, link to config flags. |

---

## P2 — Operations tooling

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P2-13 | Authenticated admin API (remote ops) | api | L | **Scaffolding:** `/api/admin/v1/*` routes exist in `src/api/admin.rs` but return `501` without auth. Remote administration still needs real handlers beyond S3, console UI, and public `/healthz`/`/readyz`. | Versioned admin HTTP API (e.g. `/api/admin/v1/…`) protected by admin credentials (dedicated token or scoped use of access/secret with `MAXIO_ADMIN` role — design doc required). Endpoints at minimum: **GET /status** (healthz + readyz + version + uptime), **GET /info** (data-dir path, disk free/used, bucket/object counts, active config: EC, quotas, region), **GET /doctor** (readiness + disk reserve + keyring usable), **POST /housekeeping/run** (on-demand stale-multipart/temp sweep), **GET /keyring** (ids + metadata, never raw keys). TLS documented as required for production; rate-limited; mutating calls audit-logged when P2-08 lands. Integration tests for auth failure, success paths, and JSON schema stability. |
| P2-12 | MaxIO admin / ops CLI (remote-first) | ops | XL | **Scaffolding:** `crates/maxio-admin` binary with command stubs + profile config; server exposes `/api/admin/v1/*` 501 stubs. The `maxio` binary still only has `healthcheck` and local `keyring`. | **`maxio-admin` CLI is remote-first** (finish P2-12) — every inspect/manage command targets a running instance via **P2-13 admin API** using named **profiles** (`endpoint`, TLS, admin credentials, timeout). Commands: `admin status`, `admin info`, `admin doctor`, `admin buckets list|head`, `admin housekeeping run`, `admin keyring list` (remote metadata). **Local-only** commands (explicit `--data-dir`, no network): `admin keyring rotate`, offline `doctor` for air-gapped recovery. Supports multiple profiles (`prod`, `staging`), `--json` + human tables, config file (`~/.config/maxio/config.toml`), env overrides. Does **not** replace `mc`/AWS CLI for object put/get. Documented in `docs/operations.md` with remote examples (including behind reverse proxy). CLI integration tests against live test server + admin API; contract tests for JSON output. **Blocked on P2-13** for remote paths. |

---

## P2 — Observability

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P2-07 | Prometheus `/metrics` endpoint | ops | M | No RED metrics for request rate, latency, errors. | Optional `--metrics-port` or `/metrics` with request counters, upload bytes, disk usage gauge. |
| P2-08 | Structured audit log | security | M | No audit trail for bucket/object mutations. | Opt-in JSON log line per mutating S3/console action: principal, bucket, key, outcome. |

---

## P3 — Nice-to-have / future

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P3-01 | Lifecycle rules (expiration) | storage | L | No object expiration or transition policies. | S3 lifecycle XML subset or console UI for prefix-based expiry. |
| P3-02 | Replication / federation | storage | XL | Single-node filesystem only. | Out of scope until clustering design exists; capture as RFC. |
| P3-03 | SQLite metadata index | storage | L | Listing large buckets scans filesystem. | Optional index for faster `ListObjectsV2` on millions of keys; migration path documented. |
| P3-04 | Workspace crate split | storage | L | Single crate for server + storage + UI embed. | `maxio-server`, `maxio-storage` crates when API boundaries stabilize. |
| P3-05 | ARM64 release binaries | ci | S | Release workflow may be x86-only today. | Multi-arch Docker image and GitHub release assets. |
| P3-06 | UI E2E tests (Playwright) | ui | M | No browser-level tests for upload/download flows. | Smoke test: login → create bucket → upload → download → delete. |
| P3-07 | Per-bucket erasure coding toggle | storage | L | Erasure coding is server-wide (`MAXIO_ERASURE_CODING`); operators cannot mix flat and chunked layouts per bucket on one instance. | Design note + implementation: per-bucket or per-prefix EC policy, migration path for existing flat objects, documented trade-offs. |

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
**Sprint 2 (harden):** P0-03, P1-01, P1-02, P0-04  
**Sprint 3 (scale maintainability):** P2-01, P2-04, P0-05, P2-06  
**Sprint 4 (erasure coding hardening):** P1-13, P2-11, P2-09, P2-10, P1-12  
**Sprint 5 (ops tooling):** P2-13 (admin API), then P2-12 (CLI: profiles → remote status/info/doctor → housekeeping; local keyring rotate last)

---

## How to use this file

1. Move items to `docs/plans/` when design work starts.
2. Link PRs in commit messages: `fix(storage): P0-03 enforce max object size`.
3. Re-prioritize after production pilot feedback.