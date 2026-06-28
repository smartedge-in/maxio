# Changelog

All notable changes to MaxIO are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Refactored P1 S3 modules: shared virtual-host helpers (`resolve_bucket`, `signature_path_from_request`), simplified policy action parsing, expanded unit tests; CI coverage floors at 80% for `virtual_host`, `credentials`, and `policy`.

### Added

- P1 S3 compatibility: virtual-hosted-style requests — `Host: bucket.{server_host}` with `MAXIO_SERVER_HOST` / `--server-host`; handler dispatch + SigV4 client-path verification (P1-09).
- P1 S3 compatibility: multi-credential store — bootstrap env keys plus optional `<data-dir>/.maxio-credentials.json` for additional access/secret pairs (P1-10 phase 1).
- P1 S3 compatibility: bucket policy v1 — `PUT/GET/DELETE ?policy` with Allow/`Principal:*` subset for `s3:GetObject` and `s3:ListBucket` (P1-11).
- Design docs: `docs/plans/2026-06-28-multi-user-credentials.md`, `docs/plans/2026-06-28-bucket-policy-evaluation.md`, `docs/s3-compatibility.md`.
- Unit tests for `CredentialStore` and policy parser; integration tests for virtual-host PUT/GET, secondary credential auth, and public-read bucket policy.
- P1 security & reliability: `MAXIO_TRUSTED_PROXIES` for safe `X-Forwarded-For` client IP behind known load balancers (P1-03).
- P1 security & reliability: console session tokens keyed to credential fingerprint — sessions invalidate immediately when access/secret change (P1-05).
- P1 security & reliability: optional `MAXIO_LOGIN_RATE_LIMIT_REDIS_URL` for distributed console login rate limiting across replicas (P1-06).
- P1 security & reliability: `/healthz?verbose=1` returns JSON subsystem metrics — disk free %, active multipart uploads, housekeeping lag, readyz (P1-08).
- Unit and integration tests for trusted proxy, verbose healthz, and session credential invalidation.

### Changed

- P1 security & reliability: README and `docs/operations.md` document bind-address exposure risks and recommend `127.0.0.1` for dev (P1-04).

- Sprint 5 (ops tooling): authenticated admin HTTP API at `/api/admin/v1/*` — Bearer `MAXIO_ADMIN_TOKEN` or Basic access/secret auth, per-IP rate limiting, endpoints for status, info, doctor, buckets, keyring metadata, and on-demand housekeeping (P2-13).
- Sprint 5 (ops tooling): `maxio-admin` CLI — remote-first commands via admin API; local `doctor --data-dir` and `keyring rotate`; profiles, `--json`, human tables; documented in `docs/operations.md` (P2-12).
- Admin API integration tests (auth failure, Bearer/Basic success, JSON schema checks).
- `maxio-admin` unit tests for auth header selection and config parsing.
- `keys::list_metadata()` and storage helpers for admin info/doctor endpoints.

- Sprint 4 (erasure coding): `VerifiedChunkReader::preflight()` validates the first required chunk before streaming EC reads; failures map to HTTP 500 S3 `InternalError` XML (P1-12).
- Sprint 4 (erasure coding): `aws-cli` CI job starts MaxIO with `--erasure-coding` so corruption tests in `aws_cli_test.sh` execute (P1-13).
- Sprint 4 (erasure coding): integration tests for multipart+EC (plain and SSE-S3) and CopyObject+EC (same/cross-bucket, SSE-S3) (P2-09, P2-10).
- Sprint 4 (erasure coding): `docs/operations.md` erasure coding section — server-wide toggle, parity, GF(2⁸) 255-shard cap, single-node scope (P2-11).
- Unit tests for `VerifiedChunkReader::preflight()` in `storage/chunk_reader.rs`.

- Sprint 3 (maintainability): split `src/storage/filesystem.rs` into `filesystem/{mod,common,object_io,multipart,encryption_io,listing,housekeeping}.rs` (P2-01).
- Sprint 3 (maintainability): CI `coverage` job — `cargo llvm-cov` summary plus line-coverage floors for `storage/crypto.rs` (80%) and `auth/signature_v4.rs` (25%) (P2-04).
- Sprint 3 (maintainability): console API integration tests — login failure/rate-limit, auth check/logout, list buckets, versioning/public settings, protected-route gate (P2-06).
- Unit test for `validate_key()` in `storage/filesystem/common.rs`.

- Sprint 2 (harden): S3 API rate limiting — configurable per-IP limits on auth failures (`MAXIO_S3_RATE_AUTH_MAX`, `MAXIO_S3_RATE_AUTH_WINDOW_SECS`, default 60 per 5 min) and PUT requests (`MAXIO_S3_RATE_PUT_MAX`, `MAXIO_S3_RATE_PUT_WINDOW_SECS`, default disabled); returns HTTP 429 with `Retry-After` and S3 `SlowDown` XML (P1-01).
- Sprint 2 (harden): tightened Content-Security-Policy — inline theme script moved to `ui/static/theme-init.js`; `script-src 'self'` without `'unsafe-inline'`; Svelte inline styles documented as remaining `'unsafe-inline'` exception (P1-02).
- Shared `rate_limit` module (`SlidingWindowLimiter`, `S3RateLimiter`, `LoginRateLimiter`) with unit tests.
- Integration tests for S3 auth-failure and PUT rate limits and CSP script policy.

- Sprint 1 (stabilize): case-insensitive presigned URL detection (`query_has_presigned_signature`, `parse_presigned_query`); regression tests in `signature_v4` and integration suite.
- CI: `bun run check` (svelte-check) in pull-request checks before frontend build (P2-03).
- `maxio-admin` workspace crate — remote-first ops CLI scaffolding (`status`, `info`, `doctor`, `buckets`, `housekeeping`, `keyring`) with profile config and stub responses until P2-13 admin API is implemented.
- Server stub routes at `/api/admin/v1/*` returning `501 Not Implemented` (P2-13 placeholder).

- Storage-aware `/readyz` readiness probe: verifies the data directory exists, is writable (write probe), and the SSE-S3 keyring has at least one key. Returns `503 Service Unavailable` when storage is not usable.
- Configurable upload quotas via `MAXIO_MAX_OBJECT_BYTES` / `--max-object-bytes` (default `0` = unlimited). Oversized uploads are rejected with S3 `EntityTooLarge` (HTTP 400).
- Configurable disk reserve via `MAXIO_MIN_FREE_DISK_BYTES` / `--min-free-disk-bytes` (default `0` = disabled). New uploads are rejected with `InsufficientStorage` (HTTP 507) when free space on the data volume falls below the reserve.
- `QuotaReader` stream wrapper that enforces object size and disk reserve limits during streaming uploads (S3 PUT, multipart parts, and console uploads).
- Quota checks on `CompleteMultipartUpload` for total assembled object size and disk reserve.
- S3 error codes `EntityTooLarge` and `InsufficientStorage` with matching HTTP status codes.
- `Keyring::is_usable()` for readiness checks.
- `FilesystemStorage::check_readiness()` for storage subsystem health.
- `docs/operations.md` — production deployment guide covering TLS termination, credentials, keyring backup, health probes, quotas, Docker, and Kubernetes.
- Integration tests for `/readyz` failure on unwritable data directory and quota enforcement on PUT.
- CI job `aws-cli` on push to `main`: builds the release binary, starts a server, and runs `tests/aws_cli_test.sh`.

### Changed

- `/readyz` no longer returns `200` unconditionally; it reflects actual storage readiness.
- S3 and console upload paths pass `Content-Length` (when present) into storage for early quota validation.
- Upload error mapping consolidated through `storage::map_upload_error()` for consistent S3 XML responses.
- Integration test server helpers refactored (`spawn_test_server`, `new_test_storage`, `default_test_config`) to support quota configuration.

### CI

- Re-enabled `cargo test --all --all-features` in the pull-request checks workflow (removed in `1aa9fa5`).

### Docs

- README configuration table: `MAXIO_MAX_OBJECT_BYTES`, `MAXIO_MIN_FREE_DISK_BYTES`, and health endpoint behavior (`/healthz` vs `/readyz`).
- README link to `docs/operations.md`.