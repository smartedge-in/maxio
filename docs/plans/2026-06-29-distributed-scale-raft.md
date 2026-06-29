# Distributed scale architecture — dual Raft (P1-14 / was P3-13)

## Status

**Priority 1** — active product direction. See `docs/plans/2026-06-29-multi-replica-raft-priority.md`. No Raft code ships today.

## Requirement

MaxIO scale-out **must** use **two independent Raft consensus groups**:

1. **Storage Raft** — `maxio-storage` tier; owns `data_dir` metadata and storage control plane.
2. **Server Raft** — `maxio-server` tier; owns API/console routing and server control plane.

The groups are **not** a single shared Raft cluster. Each tier elects its own leader, maintains its own log, and tolerates failures within its quorum independently.

## Why independent Raft

| Concern | Storage Raft | Server Raft |
|---------|--------------|-------------|
| Scale asymmetrically | 3–7 storage nodes | 2–N stateless API pods |
| Failure domain | Disk / object metadata | HTTP / auth / console |
| Quorum sizing | Tied to data durability | Tied to request availability |
| Upgrade rollouts | Storage upgrades are cautious | Server upgrades can be faster |

Coupling both tiers into one Raft group would force identical replica counts and synchronized restarts — contradicting P3-13 asymmetric scale-out.

## Target topology

Three tiers scale **asymmetrically**. Only storage and server use Raft; UI is stateless static (P3-16).

```
                    ┌─────────────────────────┐
                    │   maxio-ui × K          │  no Raft — static SPA only
 Browser ──────────►└───────────┬─────────────┘
              /api, S3         │
                         ┌─────▼───────────────────────────────┐
                         │         Server Raft cluster          │
                         │  (maxio-server × N, own quorum)      │
                         │  leader: config epoch, route table   │
                         │  followers: serve reads, forward     │
                         │           control writes to leader   │
                         └──────────────────┬──────────────────┘
                                            │ StorageBackend RPC
                         ┌──────────────────▼──────────────────┐
                         │        Storage Raft cluster          │
                         │  (maxio-storage × M, own quorum)     │
                         │  leader: metadata mutations,         │
                         │          multipart commit, keyring   │
                         │  followers: replicated metadata log  │
                         │             + local object payloads  │
                         └─────────────────────────────────────┘
```

UI separation: `docs/plans/2026-06-29-ui-scale-out.md`.

### Storage Raft replicates (v1 sketch)

- Bucket registry (`.bucket.json` equivalents)
- Object metadata index (names, etags, sizes, versioning pointers)
- Multipart upload state transitions (create → complete → abort)
- Lifecycle / policy / EC toggles on `BucketMeta`
- Keyring **version** and active-key id (not raw key material on every follower unless encrypted)
- Replication / mutation log offsets (composes with P3-10)

Object **bytes** remain on local filesystem per storage node; Raft provides consistent metadata and ordered mutations. EC parity shards stay a single-node layout concern unless a later phase shards objects across storage nodes.

### Server Raft replicates (v1 sketch)

- Cluster membership for server tier (peer list, roles)
- Storage backend endpoint map (which storage Raft leader / endpoints to use)
- Published credential fingerprint epoch (console session invalidation, P1-05)
- Global rate-limit policy generation (optional; may still delegate hot path to Redis)
- Admin API routing epoch

Server nodes remain **horizontally scalable for data plane** (S3 GET/PUT): any server member can proxy object I/O to the storage tier once it holds a current routing snapshot from Server Raft.

## Relationship to replication epic (P3-12)

| Track | Purpose |
|-------|---------|
| **P3-09 → P3-11** | Interim active-passive DR (`rsync`, event log, `maxio-replicate`) — operator-driven, no Raft |
| **P3-13 → P3-15** | Target multi-node product architecture with dual Raft |

P3-09–P3-11 remain valid for geo-DR and migration. P3-13+ is the supported way to run multiple **live** replicas with consistent metadata.

## Implementation phases

### Phase A — Storage Raft (P3-14)

- Embed Raft log (e.g. `openraft` or `raft-rs`) in `maxio-storage`
- `StorageBackend` trait (P3-10) routes metadata writes through Raft leader
- 3-node minimum quorum; bootstrap/join CLI
- Failover: new storage leader elected; server tier picks up new endpoint via watch

### Phase B — Server Raft (P3-15)

- Server control-plane Raft in `maxio-server`
- Stateless S3 workers join server cluster; pull routing snapshot
- Health: `/readyz` fails when server cannot reach storage Raft quorum

### Phase C — Stateless UI tier (P3-16)

- `crates/maxio-ui` serves static assets; remove `rust-embed` from distributed `maxio-server`
- Ingress: `/ui` → UI service, `/api` + S3 → server service
- See `docs/plans/2026-06-29-ui-scale-out.md`

### Phase D — Asymmetric operations (P3-13 closes)

- Helm/K8s charts with independent `replicas` and PDBs per tier (UI, server, storage)
- HPA on UI and server tiers; storage tier scaled manually or via Raft lag metric
- Integration test: 2 UI + 2 server + 3 storage; kill storage leader → elect new leader → PUT/GET succeed

## Non-goals (v1 Raft)

- Single global Raft spanning storage + server members
- Active/active multi-region writes without storage Raft serialization
- Replacing S3 SigV4 with inter-node mTLS (may layer later)
- Automatic cross-tier quorum coupling (storage outage does not block server Raft election and vice versa for control plane — data plane may degrade)

## Open questions

- Raft library choice and WAL on-disk format vs reusing P3-03 SQLite index
- Whether object bytes ever leave the storage node that accepted the PUT (sticky writes vs redirect)
- Keyring material distribution: KMS-only vs encrypted Raft snapshot
- Console session store: Server Raft epoch vs external Redis

## Acceptance (epic P3-13)

- [ ] Storage and server each run a distinct Raft cluster with independent quorums
- [ ] UI runs as stateless `maxio-ui` tier with independent replica count (P3-16)
- [ ] Asymmetric replica counts documented and tested (e.g. 3 UI + 3 server + 5 storage)
- [ ] Leader failover integration tests for storage and server tiers
- [ ] Single-node colocated mode remains default (Raft disabled, optional UI embed)