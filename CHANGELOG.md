# Changelog

All notable changes to MaxIO are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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