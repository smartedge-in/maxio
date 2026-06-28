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
| P1-07 | Presigned URL detection hardening | auth | S | Presigned detection uses case-sensitive `query.contains("X-Amz-Signature=")`. | Parse query keys case-insensitively per AWS conventions; add regression test. |
| P1-08 | Deep health metrics | ops | M | `/healthz` is liveness-only with no subsystem signal. | Optional `/healthz?verbose=1` or metrics export for housekeeping lag, disk free %, active uploads. |

---

## P1 — S3 compatibility & product

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P1-09 | Virtual-hosted-style requests | api | XL | Only path-style `/{bucket}/{key}` routing is supported. | `Host: bucket.endpoint` requests resolve correctly; integration tests for AWS SDK virtual-host mode. |
| P1-10 | Multi-user / IAM-style credentials | auth | XL | Single access/secret pair for entire server. | Design doc + phased implementation: multiple keys, or integration with external IdP. |
| P1-11 | Bucket policy engine | api | XL | Public read/list flags only; no JSON bucket policies. | Evaluate MinIO policy subset; document explicit non-goals for v1. |

---

## P2 — Maintainability & code health

| ID | Title | Area | Effort | Description | Acceptance criteria |
|----|-------|------|--------|-------------|-------------------|
| P2-01 | Split `filesystem.rs` | storage | L | ~3,800-line monolith complicates review and change safety. | Extract modules: `object_io`, `multipart`, `encryption_io`, `listing`, `housekeeping`; no behavior change; tests pass. |
| P2-02 | Reduce crate-level clippy allows | storage | M | Broad `#![allow(clippy::...)]` in `main.rs` / `lib.rs` hides issues. | Remove allows file-by-file; fix or locally allow with justification. |
| P2-03 | Add `bun run check` to CI | ci | S | `svelte-check` exists in `ui/package.json` but CI only builds. | CI runs `bun run check` in `ui/`; TypeScript/Svelte errors block merge. |
| P2-04 | Unit test coverage report in CI | ci | S | Coverage is unknown; integration tests dominate. | `cargo llvm-cov` or `cargo tarpaulin` job publishes summary; set minimum threshold for `storage/crypto`, `auth`. |
| P2-05 | Replace `unwrap()` in hot paths | storage | M | `unwrap`/`expect` in auth and storage error paths. | Audit and convert to `?` + proper `S3Error`/`StorageError` where user-visible. |
| P2-06 | Console API integration tests | api | M | Console routes lack dedicated integration coverage vs S3. | Tests for login, rate limit, presign, bucket settings JSON API. |

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
| Integration test suite | ~160 tests in `tests/integration.rs` (local, not CI) |
| Public bucket anonymous access | Query sub-resource blocklist on bypass |
| Housekeeping | Stale multipart + temp file sweep |

---

## Suggested sprint order

**Sprint 1 (stabilize):** P0-01, P0-02, P2-03, P1-07  
**Sprint 2 (harden):** P0-03, P1-01, P1-02, P0-04  
**Sprint 3 (scale maintainability):** P2-01, P2-04, P0-05, P2-06  

---

## How to use this file

1. Move items to `docs/plans/` when design work starts.
2. Link PRs in commit messages: `fix(storage): P0-03 enforce max object size`.
3. Re-prioritize after production pilot feedback.