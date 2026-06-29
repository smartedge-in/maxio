# Replication and federation RFC (P3-02)

## Status

Draft — design only. No replication code ships in this milestone.

## Problem

MaxIO is a single-node filesystem-backed object store. Operators who need geographic redundancy or multi-site read replicas cannot sync or federate buckets today.

## Current architecture constraints

- `FilesystemStorage` is the only backend; `AppState.storage` is `Arc<FilesystemStorage>` with no trait boundary.
- Object identity is `(bucket, key)` on local disk; metadata lives in `.meta.json` sidecars and `.bucket.json`.
- Erasure coding is single-node Reed-Solomon parity (data recovery), not geo-replication.
- Multipart uploads, versioning directories, and SSE keyring state are local and not replicated.
- Optional SQLite metadata index (P3-03) is a local cache, not a replication log.

## Non-goals (v1)

- Multi-master active/active writes across sites.
- Strong cross-region consistency or linearizable reads.
- S3 Cross-Region Replication (CRR) XML compatibility in the first iteration.
- Automatic background replication without an explicit operator workflow.

## Options considered

### A. Active-passive `data_dir` sync (rsync/rclone)

Periodic or continuous copy of `{data_dir}/buckets/` to a standby node.

| Pros | Cons |
|------|------|
| Simple; matches backup model | No live failover; multipart in-flight state diverges |
| Works with mixed flat/EC layouts (P3-07) | Keyring rotation must be coordinated |

### B. Event log + async replication worker

Append mutation events (PUT/DELETE bucket settings) to a durable log; replayer applies on replica.

| Pros | Cons |
|------|------|
| Near-real-time; selective bucket sync | Requires new log subsystem and idempotent apply |
| Composes with metadata index for lag metrics | Versioned keys and EC manifests need careful ordering |

### C. Metadata-only federation (read-through proxy)

Federate bucket listings across independent MaxIO nodes; objects fetched on demand from peer.

| Pros | Cons |
|------|------|
| No full data copy | High latency; not true disaster recovery |

## Recommended path

1. **Phase 0 (this RFC):** Document constraints; no code.
2. **Phase 1:** Admin export of bucket inventory + `rclone`/`rsync` runbook in `docs/operations.md`.
3. **Phase 2:** Introduce `maxio-storage` trait + mutation event log (builds on P3-03 index).
4. **Phase 3:** Optional replication agent consuming the log (active-passive).

## Open questions

- How to replicate in-progress multipart uploads without leaking `.uploads/` garbage?
- Per-bucket EC toggles: must replication preserve on-disk layout byte-for-byte?
- Lifecycle expiration (P3-01): replicas must run housekeeping with aligned clocks/rules.
- Credential and keyring files: replicate `.maxio-keys.json` or use external KMS?

## Acceptance (P3-02)

- [x] RFC published under `docs/plans/`
- [ ] Reviewed by operators / maintainers
- [ ] Phase 1 runbook added when replication pilot starts