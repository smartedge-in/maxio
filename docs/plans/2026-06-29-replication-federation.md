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

1. **Phase 0 (this RFC):** Document constraints; no code. ✓
2. **Phase 1 (P3-09):** Operator runbook + inventory export — no replication daemon.
3. **Phase 2 (P3-10):** Storage trait + durable mutation event log.
4. **Phase 3 (P3-11):** Replication agent applying the log on a standby node.

**Scale-out track (separate):** Live multi-node replicas use **dual independent Raft** clusters (P3-13 → P3-14 storage, P3-15 server). See `docs/plans/2026-06-29-distributed-scale-raft.md`. P3-09–P3-11 remain the interim operator DR path without Raft.

---

## Phase 1 — Operator sync runbook (P3-09)

**Goal:** Active-passive disaster recovery without new replication code. Operators copy `data_dir` to a standby MaxIO using existing tools.

### Operator workflow

```
┌─────────────┐     rsync / rclone      ┌─────────────┐
│  Primary    │ ──────────────────────► │  Standby    │
│  MaxIO      │   (scheduled or manual) │  MaxIO      │
│  (read/write)│                        │  (stopped or │
└─────────────┘                        │   read-only)│
                                       └─────────────┘
```

1. **Inventory** — Primary exports bucket/object manifest via admin API or CLI (`maxio-admin buckets list --json` + object listing per bucket).
2. **Sync** — Operator runs `rclone sync` or `rsync -a` on `{data_dir}/buckets/` (and coordinated copies of `.maxio-keys.json`, `.maxio-credentials.json` if used).
3. **Cutover** — On primary failure: stop standby writes (if any), verify manifest checksum sample, start standby MaxIO with same credentials/keyring, repoint DNS/ingress.
4. **Catch-up** — After outage, re-sync from surviving node before returning to dual-site layout.

### Deliverables

| Item | Detail |
|------|--------|
| `docs/operations.md` section | Step-by-step runbook: prerequisites, sync commands, keyring coordination, multipart/versioning caveats, failover checklist |
| Admin inventory export | `GET /api/admin/v1/buckets/{bucket}/inventory` or CLI equivalent returning object keys, sizes, etags (paginated JSON) |
| Pre-sync checklist | Document: quiesce multipart uploads, align lifecycle clocks, EC layout preserved byte-for-byte |

### Limitations (explicit)

- Not live failover; lag equals sync interval.
- In-flight multipart and `.uploads/` state may diverge — runbook says complete or abort uploads before sync.
- No S3 `?replication` API.

---

## Phase 2 — Mutation event log (P3-10)

**Goal:** Near-real-time, selective replication foundation. Every mutating operation appends an idempotent event; replicas can apply events instead of full directory scans.

### Architecture

```
┌──────────────┐   append    ┌──────────────────┐
│ MaxIO primary│ ──────────► │ .maxio-repl-log  │
│ (Filesystem  │   on PUT/   │ (SQLite WAL or   │
│  Storage)    │   DELETE/   │  append-only JSONL)│
└──────────────┘   bucket    └────────┬─────────┘
                                      │ tail
                              ┌───────▼─────────┐
                              │ Phase 3 agent   │
                              │ (standby apply) │
                              └─────────────────┘
```

### Code changes

1. **`StorageBackend` trait** — Extract read/write/list/delete from `FilesystemStorage` so a replayer can target local FS or a test double.
2. **Event schema** — Typed records, e.g. `ObjectPut`, `ObjectDelete`, `BucketCreate`, `BucketDelete`, `BucketMetaUpdate` (versioning, policy, lifecycle, EC toggle), `MultipartComplete` (optional v2).
3. **Log writer** — Hook existing mutation paths in `maxio-storage`; atomic append with monotonic sequence id + wall timestamp.
4. **Lag metrics** — Expose `maxio_replication_log_sequence` and `maxio_replication_log_bytes` on `/metrics` when log enabled (`MAXIO_REPLICATION_LOG=true`).
5. **Replay API (internal)** — Idempotent apply function used by Phase 3 agent and integration tests.

### Event record (sketch)

```json
{
  "seq": 1042,
  "ts": "2026-06-29T12:00:00Z",
  "op": "ObjectPut",
  "bucket": "photos",
  "key": "2026/img.jpg",
  "size": 8192,
  "etag": "abc…",
  "layout": "flat"
}
```

### Non-goals in Phase 2

- No background sync process shipped.
- No multi-master conflict resolution.
- Multipart in-progress events deferred unless pilot demands them.

---

## Phase 3 — Replication agent (P3-11)

**Goal:** Automated active-passive sync from primary event log to standby `data_dir`, with operator-tunable bucket filters and lag alerts.

### Architecture

```
┌─────────────┐  tail log   ┌─────────────────┐  S3 PUT/FS   ┌─────────────┐
│   Primary   │ ──────────► │ maxio-replicate │ ───────────► │   Standby   │
│   MaxIO     │  (HTTPS or  │ (sidecar/agent) │   apply      │   MaxIO     │
│             │   shared FS)│                 │              │  data_dir   │
└─────────────┘             └─────────────────┘              └─────────────┘
```

### Agent behaviour

1. **Tail** — Follow replication log from last acknowledged `seq` (checkpoint file on standby).
2. **Fetch** — For `ObjectPut`, stream object from primary S3 API (SigV4) or shared mount; for deletes, remove local object + metadata.
3. **Apply** — Call Phase 2 replay helpers; skip if etag+size already match (idempotent).
4. **Filter** — Optional `--include-bucket` / `--exclude-bucket` flags; per-bucket pause.
5. **Alert** — Log and metric when lag (`primary_seq - applied_seq`) exceeds threshold.

### Deliverables

| Item | Detail |
|------|--------|
| `maxio-replicate` subcommand or crate | Config: primary URL, credentials, standby `data_dir`, log source, checkpoint path |
| Failover doc update | Promote standby: stop agent, start MaxIO, optional reverse log for failback |
| Integration test | Primary PUT → agent apply → standby GET roundtrip |

### Still out of scope

- Active/active writes to two primaries.
- S3 CRR XML (`PutBucketReplication`).
- Metadata-only federation (option C) — separate backlog item if needed later.

---

## Open questions

- How to replicate in-progress multipart uploads without leaking `.uploads/` garbage?
- Per-bucket EC toggles: must replication preserve on-disk layout byte-for-byte?
- Lifecycle expiration (P3-01): replicas must run housekeeping with aligned clocks/rules.
- Credential and keyring files: replicate `.maxio-keys.json` or use external KMS?

## Acceptance (P3-02)

- [x] RFC published under `docs/plans/`
- [ ] Reviewed by operators / maintainers
- [ ] Phase 1 runbook added when replication pilot starts