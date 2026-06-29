# Storage tier Raft implementation (P1-17) — **Done**

## Status

**In progress** (2026-06-29). Prerequisites P1-15, P1-16, and P1-22 are complete.

## Goal

Replicate **metadata mutations** across a storage Raft quorum while object **bytes** stay on each node's local filesystem. Server tier workers continue to call `StorageBackend`; the Raft-backed implementation forwards writes to the leader and applies committed entries locally.

## Library

[OpenRaft](https://github.com/databendlabs/openraft) `0.9.x` — see `docs/plans/2026-06-29-raft-library-spike.md`.

| Feature | Crate | CI |
|---------|-------|-----|
| `raft-spike` | `maxio-storage` | `cargo test -p maxio-storage --features raft-spike` |
| `raft` | `maxio-storage` | unit tests under `src/raft/` (expand as implementation lands) |

## Code layout (v1)

```
crates/maxio-storage/src/
  backend.rs          # StorageBackend trait (done, P1-15)
  raft/
    mod.rs            # RaftNodeConfig, StorageMutation enum (started)
    state_machine.rs  # apply StorageMutation → FilesystemStorage (TODO)
    network.rs        # peer RPC transport (TODO)
    node.rs           # OpenRaft wiring (TODO)

crates/maxio-common/src/
  cluster.rs          # StorageEndpoint, RoutingSnapshot (done, P1-22)
```

## Replicated state (v1)

| Domain | Notes |
|--------|-------|
| Bucket registry | create/delete, region, versioning, public ACL |
| Object metadata index | names, etags, sizes, version pointers |
| Multipart uploads | create, complete, abort transitions |
| Bucket settings | lifecycle, policy, per-bucket EC toggle |
| Keyring epoch | active key id rotation — not raw key bytes on every follower |

## Non-goals (v1)

- Object byte replication (local disk only)
- Server tier Raft (P1-20)
- Distributed EC shard placement (P1-18)

## Milestones

1. **Alpha** — single-process smoke: leader election + one `CreateBucket` mutation applied on all peers (dev feature flag).
2. **Beta** — 3-node bootstrap/join CLI; integration test kills leader, new leader serves writes.
3. **Done** — Prometheus metrics `raft_storage_leader`, `raft_storage_commit_lag`; documented in `docs/operations.md`.

## Acceptance (P1-17)

- [ ] 3-node bootstrap/join
- [ ] Metadata writes via leader; followers consistent after commit
- [ ] Failover integration test
- [ ] Metrics exported when `MAXIO_METRICS_ENABLED=1`
- [ ] `StorageBackend` remains the server-facing API

## References

- `docs/plans/2026-06-29-distributed-scale-raft.md`
- `crates/maxio-storage/src/backend.rs`
- `crates/maxio-common/src/cluster.rs`