# Changelog

All notable changes to MaxIO are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **P3-17:** `maxio-admin` no longer depends on the root `maxio` facade — local doctor/keyring use `maxio-storage` directly, shrinking the link graph and fixing `rust-lld` bus errors during workspace tests on constrained hosts.
- **P1-MR completion:** server `maxio serve --cluster-mode` routes bucket metadata mutations (`CreateBucket`/`DeleteBucket`) to the storage Raft leader via HTTP propose; object I/O remains on the server’s local filesystem (documented phase-1 limitation). K8s distributed storage StatefulSet enables EC + bitrot scan flags.
- **Enterprise GA completion:** Keycloak console SSO in the UI (config fetch, login, silent refresh); Playwright smoke uses browser login form; release CI ships `maxio` + `maxio-admin` + `maxio-ui` binaries and offline bundle/image jobs; `Dockerfile.ui` + `maxio-ui` in offline image pack.

### Added

- **Enterprise GA (P3-44 / P3-52 / P3-53):** airgap install scripts (`scripts/build-offline-bundle.sh`, `build-offline-images.sh`, `load-images.sh`), bare-metal systemd unit, observability compose stack, backup script, security audit checklist, DR/SLA/airgap runbook sections in `docs/operations.md`, private-registry K8s `imagePullSecrets`, Playwright console E2E (`e2e/`, CI `e2e` job).
- **P1-25 cluster EC bitrot scanner:** proactive shard checksum scan + heal from parity/peers on storage nodes (`MAXIO_BITROT_SCAN_*`); HTTP `GET /internal/shard`; Prometheus `maxio_ec_bitrot_*` counters; acceptance test in `cluster_p14`.

- Production cluster wiring: `maxio storage-raft` subcommand for multi-process storage peers over HTTP Raft (`/internal/raft/*`); server `MAXIO_STORAGE_ENDPOINTS` background routing sync; updated `deploy/k8s/distributed/` StatefulSet + server Deployment; `scripts/kind-cluster-smoke.sh`.

- `StorageBackend` trait (P1-15): all S3 metadata and object mutations go through `DynStorage` (`Arc<dyn StorageBackend>`) in `maxio-server`; `FilesystemStorage` is the default implementation; prerequisite for Raft apply path (`crates/maxio-storage/src/backend.rs`).
- Raft library spike (P1-16): OpenRaft `0.9` selected; optional `raft-spike` feature and CI smoke test; documented in `docs/plans/2026-06-29-raft-library-spike.md`.
- `maxio-common` crate (P1-22): shared `VERSION`, admin API JSON types (`StatusResponse`, `InfoResponse`, `DoctorResponse`), and cluster routing DTOs (`Tier`, `StorageEndpoint`, `RoutingSnapshot`); imported by `maxio-server`, `maxio-storage`, and `maxio-admin`.
- Multi-replica cluster epic (P1-14): `crates/maxio-cluster` with storage Raft (P1-17), distributed EC shard placement + peer read (P1-18/P1-19), server routing snapshot (P1-20), and `ClusterHarness` acceptance tests (P1-24).
- Stateless UI tier (P1-21): `crates/maxio-ui` binary serving the embedded SPA; `MAXIO_SERVE_UI` on `maxio-server` (default `true`; set `false` when UI runs separately).
- Cluster server mode (P1-20): `MAXIO_CLUSTER_MODE` gates `/readyz` on storage quorum; Prometheus gauges `maxio_cluster_routing_epoch` and `maxio_cluster_storage_quorum_ok`.
- Kubernetes manifests (P1-24): `deploy/k8s/single-node/` and `deploy/k8s/distributed/` (3 storage StatefulSet, 2 server Deployment, 2 UI Deployment, Ingress split `/ui` vs S3).
- Cluster CI harness (P1-24): `scripts/cluster-test.sh` and GitHub Actions `cluster` job.
- Storage Raft types (P1-17): `raft` feature, `crates/maxio-storage/src/raft/` with `RaftNodeConfig`, `StorageMutation` (including `PutShardMap` for distributed EC).
- npm runtime license audit (P3-24): `scripts/check-npm-licenses.sh` in CI; local `make npm-licenses`; documented in `docs/licensing.md`.

- SQLite metadata index (P3-03): `MAXIO_METADATA_INDEX` / `--metadata-index` maintains `{data_dir}/.maxio-metadata.db` for fast `ListObjects`; rebuild on startup; filesystem walk fallback.
- Per-bucket erasure coding (P3-07): `BucketMeta.erasure_coding` override with `PUT/GET ?erasure`; mixed flat/chunked buckets when server EC is enabled.
- Lifecycle expiration (P3-01): prefix-based rules on `BucketMeta`; `PUT/GET/DELETE ?lifecycle`; housekeeping expires objects past `expiration_days` (non-versioned buckets).
- Replication/federation RFC (P3-02): `docs/plans/2026-06-29-replication-federation.md` — design-only; implementation deferred.
- Unit tests for metadata index parity, per-bucket EC, lifecycle sweep; integration tests for lifecycle API and mixed EC layouts.

- Workspace crate split (P3-04): `crates/maxio-storage` (filesystem-backed object storage) and `crates/maxio-server` (Axum S3/console/admin API, embedded UI); root `maxio` package is a thin facade re-exporting both crates for a stable public API.
- Crate-boundary unit tests in `maxio-storage`, `maxio-server`, and root `maxio` facade; coverage floor paths updated for the new layout.
- Prometheus metrics (`MAXIO_METRICS_ENABLED`, optional `MAXIO_METRICS_PORT`): `GET /metrics` with request counters, latency sum/count, SlowDown total, upload bytes, uptime, disk free/total, active multipart uploads.
- Structured audit log (`MAXIO_AUDIT_LOG`): JSON lines on `target=maxio_audit` for mutating S3/console/admin actions (principal, bucket, key, status, outcome).
- Integration tests for metrics upload-byte counter, dedicated metrics port, and audit log principal/object capture.
- `auth/hmac` helper and `AuthPrincipal` request extension for SigV4-authenticated access keys.
- Root **`VERSION`** file as the single source of truth for [Semantic Versioning](https://semver.org/); `make sync-version` propagates it to `Cargo.toml` (workspace) and `ui/package.json`; `maxio::version::VERSION` is exposed at runtime.
- Production GNU **Makefile** with a full local validation pipeline (`make ci` / `make all`): fmt, check, clippy, test, coverage, `cargo audit`, `cargo deny`, Trivy filesystem/secret/config/license scans, CycloneDX SBOM, doc, release build, Docker image, and Trivy image scan.
- `make install-tools` — installs Rust toolchain components, `cargo-audit` / `cargo-deny` / `cargo-llvm-cov`, bun (when `unzip` is available), and Trivy to `~/.local/bin` (run as a normal user, not `sudo`).
- `make deny-all` — full `cargo deny check` (licenses, advisories, bans, sources); `make deny` defaults to licenses only (matches GitHub Actions).
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

- Workspace crate split (P3-04): `map_storage_upload_error()` moved to `maxio-server::error`; UI embed/build pipeline moved to `maxio-server/build.rs`.
- Removed crate-level `#![allow(clippy::…)]` from `lib.rs` / `main.rs`; fixed mechanical clippy lints workspace-wide.
- Replaced `unwrap`/`expect` in auth HMAC and storage hot paths with `StorageError` / `IntegrityError` returns.
- Audit middleware runs after per-route auth so `AuthPrincipal` (SigV4 access key) is recorded; admin API sets principal for Bearer/Basic auth.
- Bumped direct dependency `rand` from `0.10.0` to `0.10.1` (fixes `RUSTSEC-2026-0097` / GHSA-cq8v-f236-94qc reported by Trivy and `cargo audit`).
- Licensing: replaced MPL-2.0 `dirs` in `maxio-admin` with XDG/HOME config-dir resolution; switched `reqwest` from `rustls-tls` to `native-tls-vendored`; embedded UI fonts changed from OFL Geist to MIT Inter + JetBrains Mono; CI enforces permissive Rust licenses via `cargo-deny` (`deny.toml`, `docs/licensing.md`).
- Three review/refactor cycles on P1 S3 code: shared virtual-host helpers, auth public-bypass constants, integration test helpers, expanded unit/integration tests, and ≥80% CI coverage floors on `virtual_host`, `credentials`, and `policy`.
- P1 security & reliability: README and `docs/operations.md` document bind-address exposure risks and recommend `127.0.0.1` for dev (P1-04).
- CORS middleware reordered — `OPTIONS` preflight runs before SigV4 auth so browser clients receive CORS headers on unauthenticated preflight requests.
- SSE read path: `FrameDecryptor::preflight()` validates the first encrypted frame before streaming the response body; sidecar integrity errors map to HTTP 400, EC chunk corruption to HTTP 500.
- `make ci` runs `cargo clean` after `doc` and before `release` to drop debug artifacts and avoid disk exhaustion on small root volumes.
- Trivy vulnerability DB cache defaults to `/tmp/maxio-trivy-cache` instead of the repository tree.
- `maxio-admin` crate version tracks the workspace semver (was independent `0.1.0`).
- Docker `image` tags default to `maxio:v$(VERSION)` from the `VERSION` file.
- `build.rs` falls back gracefully when bun is missing (`SKIP_FRONTEND=1`); `make ci` auto-enables `SKIP_FRONTEND` when bun is not on `PATH`.
- `/readyz` no longer returns `200` unconditionally; it reflects actual storage readiness.
- S3 and console upload paths pass `Content-Length` (when present) into storage for early quota validation.
- Upload error mapping consolidated through `storage::map_upload_error()` for consistent S3 XML responses.
- Integration test server helpers refactored (`spawn_test_server`, `new_test_storage`, `default_test_config`) to support quota configuration.

### CI

- Re-enabled `cargo test --all --all-features` in the pull-request checks workflow (removed in `1aa9fa5`).
- GitHub Actions **License audit** job runs `cargo deny check licenses`; local `make deny` matches this default.
- `deny.toml` ignores transitive `RUSTSEC-2024-0384` (`instant` via `reed-solomon-erasure`) and `RUSTSEC-2026-0097` (`rand` unsound with custom logger — not used in MaxIO); `cargo audit` reports the same advisories as allowed warnings.
- `make ci` extends GitHub Actions with coverage, Trivy, SBOM, release build, and container image validation (requires Docker for `image` / `trivy-image`).

### Docs

- README configuration table: `MAXIO_MAX_OBJECT_BYTES`, `MAXIO_MIN_FREE_DISK_BYTES`, and health endpoint behavior (`/healthz` vs `/readyz`).
- README link to `docs/operations.md`.
- `docs/operations.md` — local CI workflow (`make ci`), tool installation, and disk-space guidance for full pipeline runs.
- `docs/licensing.md` — `make deny` / `make deny-all`, advisory policy, and trimmed SPDX allow-list.
- `CLAUDE.md` — Makefile targets and extended validation pipeline.