# MaxIO

S3-compatible object storage server written in Rust. Single-binary replacement for MinIO.

## Naming Convention

Always spell the product name **MaxIO** (capital M, capital I, capital O). Never use "Maxio", "maxio", or "MAXIO" in prose. Lowercase `maxio` is acceptable only for CLI binary names, environment variable prefixes (`MAXIO_`), mc aliases, and code identifiers.

## User Preferences

- Use **bun** (not npm) for the `ui/` frontend

## Versioning

MaxIO uses [Semantic Versioning](https://semver.org/). The release number lives in the repository
root **`VERSION`** file (currently `0.4.2`). Edit `VERSION`, then run `make sync-version` to
update `Cargo.toml` and `ui/package.json`. `make check`, `make release`, and `make ci` run
`sync-version` automatically. Runtime and CLI `--version` read `maxio::version::VERSION`.

```bash
make version       # print current semver
make sync-version  # propagate VERSION → manifests
```

## Build & Run

```bash
make install-tools   # once — rustfmt/clippy, cargo-audit/deny/llvm-cov, bun, Trivy
make release
./target/release/maxio --data-dir ./data --port 9000
```

Environment variables: `MAXIO_PORT`, `MAXIO_ADDRESS`, `MAXIO_DATA_DIR`, `MAXIO_ACCESS_KEY` (aliases: `MINIO_ROOT_USER`, `MINIO_ACCESS_KEY`), `MAXIO_SECRET_KEY` (aliases: `MINIO_ROOT_PASSWORD`, `MINIO_SECRET_KEY`), `MAXIO_REGION` (aliases: `MINIO_REGION_NAME`, `MINIO_REGION`)

## Makefile CI pipeline

The root `Makefile` mirrors and extends GitHub Actions validation. Default goal is `help`.

| Target | Purpose |
|--------|---------|
| `make ci` / `make all` | Full pipeline (fmt → release → Docker image scan) |
| `make test` | `cargo test --workspace --all-features` |
| `make lint` | Clippy with `-D warnings` |
| `make coverage` | HTML report under `coverage/` |
| `make deny` | `cargo deny check licenses` (CI default) |
| `make deny-all` | Full cargo-deny (licenses, advisories, bans, sources) |
| `make audit` | `cargo audit` (ignored transitive advisories = allowed warnings) |
| `make trivy-fs` | Trivy vuln/secret/misconfig filesystem scan |
| `make release` | Optimized binaries in `target/release/` |

Run `make install-tools` as a **normal user** (not `sudo`). Without bun, `SKIP_FRONTEND=1` is
auto-set. Without Docker, skip `image` / `trivy-image`. On disks under ~20 GiB, ensure several
GB free before `make ci` — the pipeline runs `cargo clean` before `release`. Details:
`docs/operations.md`.

## Production Build

The release binary is fully self-contained — the frontend UI is embedded at compile time via `rust-embed`. No external files needed.

```bash
# 1. Install frontend dependencies
cd ui && bun install

# 2. Build frontend (outputs to ui/build/; cargo build also does this automatically)
bun run build && cd ..

# 3. Build optimized binary
cargo build --release

# Result: single binary at ./target/release/maxio
# Copy it anywhere — no ui/build/ or other files needed at runtime
```

The binary serves the web console at `/ui/` with proper MIME types, ETags, and cache headers (immutable for hashed assets, no-store for `200.html` / HTML shell).

Defaults: port 9000, access/secret `maxioadmin`/`maxioadmin`, region `us-east-1`

## Development Workflow

**Test-Driven Development (TDD)**: Before implementing any new function or feature, write a failing test first. Then implement until the test passes.

**After every code change**, re-run the full test suite to catch regressions:

```bash
# 1. Unit + integration tests (always run first, no server needed)
cargo test

# 2. AWS CLI integration tests (start server, run tests, stop server)
cargo build && RUST_LOG=info ./target/debug/maxio --data-dir /tmp/maxio-test --port 9876 &
./tests/aws_cli_test.sh 9876 /tmp/maxio-test
kill %1 && rm -rf /tmp/maxio-test
```

**Hot-reload dev server** (for manual testing):

```bash
bun run dev
```

This runs both processes concurrently (Ctrl+C kills both):
- `cargo watch` — rebuilds and restarts the Rust server on backend changes
- Vite dev server — serves the UI with HMR at `http://127.0.0.1:5173/ui/` and proxies `/api` to the Rust server

## Architecture

### Module Layout

- `src/main.rs` — entry point, config, server start, graceful shutdown
- `src/config.rs` — CLI args + env vars via clap derive
- `src/server.rs` — Axum router construction, AppState, middleware wiring
- `src/error.rs` — S3Error with XML error response rendering
- `src/auth/` — AWS Signature V4 verification + Axum middleware
- `src/api/` — S3 API handlers (bucket.rs, object.rs, multipart.rs, list.rs, router.rs, console.rs)
- `src/storage/` — Filesystem storage (buckets as dirs, objects as files, JSON sidecar metadata)
- `src/xml/` — S3 XML response types (serde + quick-xml)

### Key Design Decisions

- **Pure filesystem storage**: No database. Buckets are directories, objects are files at their key path, metadata in `.meta.json` sidecars. Backup-friendly — just copy the data dir
- **Storage layout**: `{data_dir}/buckets/{bucket-name}/{key-path}` for data, `{key-path}.meta.json` for metadata, `.bucket.json` for bucket metadata
- **Path-style and virtual-hosted-style**: `/{bucket}/{key}` and `Host: bucket.endpoint/key` (see `docs/s3-compatibility.md`)
- **UNSIGNED-PAYLOAD accepted**: Skips body hashing for PutObject (AWS CLI default)
- **Embedded UI assets**: Frontend is compiled into the binary via `rust-embed`. In debug builds, assets are read from the SvelteKit static build (`ui/build/`) when embedded; dev uses Vite/SvelteKit HMR. In release builds, assets are baked in — single binary, no external files needed
- **Web console**: SPA at `/ui/`, API at `/api/`. Cookie-based auth (HMAC tokens, not SigV4). Presigned URL generation with configurable expiry (1h/6h/24h/7d picker in UI)

### Data Layout

```
{data_dir}/
└── buckets/
    └── my-bucket/
        ├── .bucket.json                    # bucket metadata
        ├── .uploads/                       # in-progress multipart uploads
        │   └── {uploadId}/
        │       ├── .meta.json              # MultipartUploadMeta (key, content_type, initiated)
        │       ├── 1                       # part 1 bytes
        │       └── 1.meta.json             # PartMeta (part_number, etag, size)
        ├── photos/
        │   ├── vacation.jpg                # object data
        │   └── vacation.jpg.meta.json      # object metadata (etag, size, content_type, last_modified)
        └── readme.txt
            └── readme.txt.meta.json
```

### S3 Operations Implemented

| Operation | Method | Path |
|---|---|---|
| ListBuckets | GET | `/` |
| CreateBucket | PUT | `/{bucket}` |
| HeadBucket | HEAD | `/{bucket}` |
| DeleteBucket | DELETE | `/{bucket}` |
| GetBucketLocation | GET | `/{bucket}?location` |
| ListObjectsV1 | GET | `/{bucket}?prefix=&marker=&max-keys=&delimiter=` |
| ListObjectsV2 | GET | `/{bucket}?list-type=2` |
| ListObjectVersions | GET | `/{bucket}?versions` |
| GetBucketVersioning | GET | `/{bucket}?versioning` |
| PutBucketVersioning | PUT | `/{bucket}?versioning` |
| DeleteObjects | POST | `/{bucket}?delete` |
| PutObject | PUT | `/{bucket}/{key}` |
| GetObject | GET | `/{bucket}/{key}` |
| HeadObject | HEAD | `/{bucket}/{key}` |
| DeleteObject | DELETE | `/{bucket}/{key}` |
| CopyObject | PUT | `/{bucket}/{key}` (with `x-amz-copy-source` header) |
| GetObjectTagging | GET | `/{bucket}/{key}?tagging` |
| PutObjectTagging | PUT | `/{bucket}/{key}?tagging` |
| DeleteObjectTagging | DELETE | `/{bucket}/{key}?tagging` |
| GetBucketCors | GET | `/{bucket}?cors` |
| PutBucketCors | PUT | `/{bucket}?cors` |
| DeleteBucketCors | DELETE | `/{bucket}?cors` |
| GetBucketEncryption | GET | `/{bucket}?encryption` |
| PutBucketEncryption | PUT | `/{bucket}?encryption` |
| DeleteBucketEncryption | DELETE | `/{bucket}?encryption` |
| CreateMultipartUpload | POST | `/{bucket}/{key}?uploads` |
| UploadPart | PUT | `/{bucket}/{key}?partNumber=N&uploadId=X` |
| UploadPartCopy | PUT | `/{bucket}/{key}?partNumber=N&uploadId=X` (with `x-amz-copy-source` header) |
| CompleteMultipartUpload | POST | `/{bucket}/{key}?uploadId=X` |
| AbortMultipartUpload | DELETE | `/{bucket}/{key}?uploadId=X` |
| ListParts | GET | `/{bucket}/{key}?uploadId=X` |
| ListMultipartUploads | GET | `/{bucket}?uploads` |

### Console API (`/api/`)

| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/api/auth/login` | POST | none | Login with accessKey/secretKey, sets session cookie |
| `/api/auth/check` | GET | none | Check if session cookie is valid |
| `/api/auth/logout` | POST | cookie | Clear session cookie |
| `/api/buckets` | GET | cookie | List all buckets |
| `/api/buckets` | POST | cookie | Create bucket (`{ name }`) |
| `/api/buckets/{bucket}` | DELETE | cookie | Delete bucket |
| `/api/buckets/{bucket}/objects` | GET | cookie | List objects (`?prefix=&delimiter=`) |
| `/api/buckets/{bucket}/objects/{key}` | DELETE | cookie | Delete object |
| `/api/buckets/{bucket}/upload/{key}` | PUT | cookie | Upload object |
| `/api/buckets/{bucket}/download/{key}` | GET | cookie | Download object |
| `/api/buckets/{bucket}/presign/{key}` | GET | cookie | Generate presigned URL (`?expires=SECONDS`, default 3600, max 604800) |

### Server-Side Encryption (SSE)

MaxIO supports **SSE-S3** (server-managed keys) and **SSE-C** (customer-supplied keys) using AES-256-GCM with per-frame nonces (65,536-byte chunks). SSE-KMS is intentionally not supported and rejected with `InvalidEncryptionAlgorithm`.

- **Per-object DEK**: Each object gets a fresh 256-bit Data Encryption Key. For SSE-S3, the DEK is wrapped by the active master key (AES-256-GCM) and stored alongside the object metadata. For SSE-C, the DEK is wrapped by the customer-supplied key submitted on every read.
- **Sidecar integrity**: HMAC-SHA256 binds encryption metadata (key id, wrapped DEK, nonce prefix) to the object — tampering with the sidecar causes decryption to fail.
- **Erasure coding composition**: When EC is enabled, plaintext is encrypted first, then the ciphertext is sharded across EC chunks. Range reads work transparently across encrypted EC chunks.
- **Bucket default encryption**: `PutBucketEncryption` / `GetBucketEncryption` / `DeleteBucketEncryption` set a per-bucket default. Explicit `x-amz-server-side-encryption` headers on PUT override the default.
- **Multipart**: One DEK per multipart session. SSE-C parts must submit the same customer key (validated via MD5) on every part.

#### Master Key Management

| Concern | Behavior |
|---|---|
| Bootstrap | First server start auto-generates a 32-byte master key in `<data-dir>/.maxio-keys.json` (file mode 0600 on Unix). Back this file up — losing it makes all SSE-S3 objects unrecoverable |
| Override | Set `MAXIO_MASTER_KEY` (or `--master-key`) to a base64-encoded 32-byte key. Bypasses the on-disk keyring file |
| Rotation | `maxio keyring rotate --data-dir <dir>` generates a new active key and demotes the previous active key (retained so existing objects keep decrypting). Restart the server to begin encrypting new objects with the new key. Existing objects remain readable; they do not get rewritten |
| Inspection | `maxio keyring list --data-dir <dir>` prints key ids, creation times, and active flag (never the raw key material) |
| Windows | `0600` file mode is only enforced on Unix — on Windows, restrict ACLs manually or use full-disk encryption |

#### Backup & Recovery

The `.maxio-keys.json` file is the single source of truth for SSE-S3 decryption. **Back it up offline** at the same time as the data directory. Loss of all keys in the ring = permanent data loss for SSE-S3 objects (this is by design — there is no escrow). For disaster recovery, copy the keyring file to the new host before restoring object data.

### Frontend Error Logging

All `fetch` catch blocks in UI components log errors via `console.error` with context (e.g. `'fetchBuckets failed:'`, `'shareObject failed:'`). Check browser DevTools console for debugging.

### Testing with MinIO Client (mc)

```bash
# Install mc
brew install minio/stable/mc

# Configure alias
mc alias set maxio http://localhost:9000 maxioadmin maxioadmin

# Bucket operations
mc mb maxio/test-bucket
mc ls maxio/

# Upload / download
echo "hello maxio" > /tmp/test.txt
mc cp /tmp/test.txt maxio/test-bucket/test.txt
mc ls maxio/test-bucket/
mc cat maxio/test-bucket/test.txt
mc cp maxio/test-bucket/test.txt /tmp/downloaded.txt

# Nested keys
mc cp /tmp/test.txt maxio/test-bucket/folder/nested/file.txt
mc ls maxio/test-bucket/folder/

# Cleanup
mc rm maxio/test-bucket/test.txt
mc rm maxio/test-bucket/folder/nested/file.txt
mc rb maxio/test-bucket
```

### Testing with AWS CLI

```bash
export AWS_ACCESS_KEY_ID=maxioadmin
export AWS_SECRET_ACCESS_KEY=maxioadmin
aws --endpoint-url http://localhost:9000 s3 mb s3://test-bucket
aws --endpoint-url http://localhost:9000 s3 cp file.txt s3://test-bucket/file.txt
aws --endpoint-url http://localhost:9000 s3 ls s3://test-bucket/
aws --endpoint-url http://localhost:9000 s3 cp s3://test-bucket/file.txt downloaded.txt
aws --endpoint-url http://localhost:9000 s3 rm s3://test-bucket/file.txt
aws --endpoint-url http://localhost:9000 s3 rb s3://test-bucket
```

### Running Tests

```bash
# Unit + integration tests (no server needed)
make test
# or: cargo test --workspace --all-features

# AWS CLI integration tests (requires running server)
./tests/aws_cli_test.sh
```

### License and security checks

```bash
make deny          # licenses (CI)
make deny-all      # full cargo-deny graph
make audit         # RustSec advisories
make trivy-fs      # Trivy filesystem scan
```

### Observability

- **Metrics:** `MAXIO_METRICS_ENABLED=1` → `GET /metrics` (Prometheus text). Optional `MAXIO_METRICS_PORT` for a dedicated listener.
- **Audit log:** `MAXIO_AUDIT_LOG=1` → JSON lines on tracing target `maxio_audit` for mutating API calls.

Crate-level `#![allow(clippy::…)]` has been removed from `lib.rs` / `main.rs`; only function-level allows remain where needed (e.g. `too_many_arguments` on storage entry points).

### Benchmarking (MaxIO vs MinIO)

Uses [WARP](https://github.com/minio/warp) to compare MaxIO against MinIO across 7 scenarios: PUT (4KiB/1MiB/64MiB), GET (4KiB/1MiB), mixed workload, and multipart uploads. Prerequisites: `brew install minio-warp` and `brew install minio/stable/minio`.

```bash
# Full benchmark (starts both servers automatically)
cargo build --release
./tests/bench.sh

# Quick benchmark (small objects + mixed only, 10s each)
./tests/bench.sh --duration=10s --scenarios=put-small,get-small,mixed

# Custom duration
./tests/bench.sh --duration=60s

# Against external servers (skip automatic server management)
./tests/bench.sh --maxio-host=server1:9000 --minio-host=server2:9000

# Via root package scripts
bun run bench        # full (30s per scenario)
bun run bench:quick  # quick smoke test
```

**Remote server benchmark** (single command — cross-compiles, copies binary, auto-downloads warp + minio on the server, runs, streams results):

```bash
./tests/bench-remote.sh user@host
./tests/bench-remote.sh user@host --duration=60s --scenarios=put-small,mixed
```

## UI Design System

The web console (`ui/`) follows the Coolify design system. The full specification is in [`ui/DESIGN_SYSTEM.md`](ui/DESIGN_SYSTEM.md). Key points:

- **Stack**: SvelteKit static SPA, Svelte 5, Vite, Tailwind CSS v4, shadcn-svelte components, TanStack Query
- **Theme**: Class-based dark mode (`.dark` on `<html>`), with light/dark CSS variable swap in `ui/src/app.css`
- **Accent colors**: Coollabs purple `#6b16ed` (light) / warning yellow `#fcd452` (dark). Brand purple (`--color-brand`) is always `#6b16ed` regardless of theme
- **Font**: Inter + JetBrains Mono via `@fontsource/inter` / `@fontsource/jetbrains-mono` (MIT-licensed)
- **Inputs**: Inset box-shadow system (4px colored left bar on focus), no standard borders — see `.input-cool` in `app.css`
- **Buttons**: `border-2`, `h-8`, `rounded-sm`. Variants: `default`, `highlighted`, `destructive`, `outline`, `secondary`, `ghost`, `link`, `brand`
- **Border radius**: `0.125rem` (2px) everywhere — set via `--radius` in `@theme inline`
- **Sidebar**: Collapsible 224px → 56px icon-only, uses `--cool-sidebar-*` CSS variables

## Roadmap

- **Phase 2**: ~~Multipart upload~~, ~~presigned URLs~~, ~~CopyObject~~, ~~DeleteObjects batch~~, ~~CORS~~, ~~Range headers~~
- **Phase 3**: ~~Web console (SPA at `/ui/`)~~, ~~versioning~~, lifecycle rules, multi-user, metrics
- **Phase 4**: Distributed mode, ~~erasure coding~~, replication
