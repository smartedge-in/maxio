# S3 compatibility matrix (P3-36)

Published compatibility status for AWS S3 API workflows. This matrix is the procurement and integration gate for enterprise deployments.

**CI reference:** Compatibility rows marked **CI** are exercised on every push to `main`:

| Job | Workflow | What it runs |
|-----|----------|--------------|
| `aws-cli` | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) | `tests/aws_cli_test.sh` against release `maxio` with `--erasure-coding` |
| `checks` | `.github/workflows/ci.yml` | `cargo test --workspace --all-features` including integration tests |
| `coverage` | `.github/workflows/ci.yml` | Line-coverage floors on virtual-host, credentials, policy modules |

Local reproduction:

```bash
cargo build --release
DATA_DIR=$(mktemp -d)
./target/release/maxio --data-dir "$DATA_DIR" --port 19000 --address 127.0.0.1 \
  --erasure-coding --chunk-size 1048576 --allow-insecure-dev &
./tests/aws_cli_test.sh 19000 "$DATA_DIR"
```

## URL styles

| Feature | Example | Status | CI |
|---------|---------|--------|-----|
| Path-style | `http://endpoint/bucket/key` | Supported | aws-cli |
| Virtual-hosted-style | `http://bucket.endpoint/key` | Supported (P1-09) | integration + coverage |

Configure virtual-hosted requests with `MAXIO_SERVER_HOST` / `--server-host`. Signature V4 uses the client path via [`VirtualHostContext`](../crates/maxio-server/src/api/virtual_host.rs).

## Authentication

| Feature | Status | CI |
|---------|--------|-----|
| AWS Signature V4 | Supported | aws-cli |
| Presigned GET/PUT/HEAD | Supported | aws-cli |
| Multiple static credential pairs (`.maxio-credentials.json`) | Supported | integration |
| Console cookie sessions | Supported (not SigV4) | integration |

## Bucket operations

| API / operation | Status | CI |
|-----------------|--------|-----|
| `CreateBucket` / `DeleteBucket` | Supported | aws-cli |
| `HeadBucket` | Supported | aws-cli |
| `ListBuckets` | Supported | aws-cli |
| `GetBucketLocation` | Supported (fixed `us-east-1`) | aws-cli |
| `PutBucketCors` / `GetBucketCors` / `DeleteBucketCors` | Supported | aws-cli |
| `PutBucketEncryption` / `GetBucketEncryption` / `DeleteBucketEncryption` | Supported (SSE-S3 AES256) | aws-cli |
| `PutBucketVersioning` / `GetBucketVersioning` | Supported (Enabled/Suspended) | aws-cli |
| `PutBucketPolicy` / `GetBucketPolicy` / `DeleteBucketPolicy` | v1 subset only | integration |
| `PutBucketLifecycle` / `GetBucketLifecycle` / `DeleteBucketLifecycle` | Prefix + days expiration | integration |
| `PutBucketTagging` | Not supported | — |

## Object operations

| API / operation | Status | CI |
|-----------------|--------|-----|
| `PutObject` / `GetObject` / `HeadObject` / `DeleteObject` | Supported | aws-cli |
| `CopyObject` (same-bucket, cross-bucket) | Supported | aws-cli |
| `UploadPart` / `CompleteMultipartUpload` / `AbortMultipartUpload` | Supported | aws-cli |
| `UploadPartCopy` | Supported | aws-cli |
| `DeleteObjects` (batch) | Supported | aws-cli |
| `ListObjects` / `ListObjectsV2` | Supported | aws-cli |
| `ListObjectsV1` pagination | Supported | aws-cli |
| Range GET (`bytes=`) | Supported | aws-cli |
| Conditional headers (`If-Match`, `If-None-Match`, etc.) | Supported | aws-cli |
| Checksum headers (CRC32, CRC32C, SHA1, SHA256) | Supported | aws-cli |
| Object tagging (`PutObjectTagging`, etc.) | Supported | aws-cli |
| Object versioning (list versions, delete version) | Supported (non-current delete) | aws-cli |
| Object ACLs / canned ACL grants | Not supported | — |
| Object Lock / legal hold | Not supported | — |

## Encryption

| Feature | Status | CI |
|---------|--------|-----|
| SSE-S3 (AES256) | Supported | aws-cli |
| SSE-C (customer-provided keys) | Supported | aws-cli |
| SSE-C + range reads | Supported | aws-cli |
| SSE-C → SSE-S3 re-encryption on copy | Supported | aws-cli |
| Bucket default encryption | Supported | aws-cli |
| SSE-KMS (`aws:kms`) | Not supported (P3-35 planned) | — |

## Bucket policies (v1)

JSON policies via `?policy` with a restricted subset:

| Capability | Status | CI |
|------------|--------|-----|
| `Effect: Allow` only | Supported | integration |
| `Principal: *` | Supported | integration |
| `s3:GetObject`, `s3:ListBucket` | Supported | integration |
| `Effect: Deny`, conditions, IAM principals | Not supported (P3-28) | — |

See [bucket policy evaluation](plans/2026-06-28-bucket-policy-evaluation.md).

## Erasure coding

| Feature | Status | CI |
|---------|--------|-----|
| Single-node EC layout (`--erasure-coding`) | Supported | aws-cli (when EC enabled) |
| Corruption detection on read | Supported | aws-cli |
| Reed-Solomon recovery (`--parity-shards > 0`) | Supported | integration |
| Distributed EC (multi-node) | Supported via storage Raft + `maxio-cluster` harness (P1-18+) — not yet exercised by `aws-cli` on distributed K8s deploy | `cluster-test` / `cluster_p14` |

## Not implemented / deferred

| Feature | Status | Backlog |
|---------|--------|---------|
| S3 event notifications (webhooks) | Not supported | P3-27 |
| IAM users / roles / STS | Not supported | — |
| Replication / CRR | Not supported | P3-09+ (deferred) |
| S3 Select / Glacier tiers | Not supported | — |

## Console vs S3

The web console uses cookie sessions (not SigV4). S3 clients use access/secret keys from the same credential store. Optional Keycloak SSO (P3-08) applies to the console only.

## Document maintenance

When adding or changing S3 behaviour:

1. Add or extend tests in `tests/aws_cli_test.sh` and/or `tests/integration.rs`
2. Update this matrix and link the CI job that enforces the row
3. Mention compatibility impact in `CHANGELOG.md` for release notes