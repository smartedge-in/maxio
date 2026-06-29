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