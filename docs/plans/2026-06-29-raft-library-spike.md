# Raft library spike (P1-16)

## Status

Complete (2026-06). Choice: **[OpenRaft](https://github.com/databendlabs/openraft)** `0.9.x`.

## Candidates

| Crate | License | Verdict |
|-------|---------|---------|
| **openraft** | Apache-2.0 / MIT | **Selected** — active, async, documented, P3-24 clean |
| raft-rs (tikv/raft) | Apache-2.0 / MIT | Viable; lower-level; more boilerplate for networking |
| tikv/raft-rs fork ecosystem | varies | Defer — stick to one dep |

## Decision

Use **OpenRaft 0.9** as the storage-tier consensus library for **P1-17**.

Rationale:

- Permissive license (passes `cargo deny check licenses` with `raft-spike` feature)
- Async-first (fits Tokio stack)
- Used in production-adjacent Rust projects (Databend lineage)
- Clear separation: Raft log + state machine vs MaxIO `StorageBackend` apply path (P1-15)

## Integration plan (P1-17)

1. `RaftStorageBackend` wraps `FilesystemStorage` local I/O; metadata mutations go through Raft apply.
2. Object bytes stay on local disk per node (not in Raft log).
3. `maxio-common` (P1-22) carries RPC types and routing snapshots.

## Repo wiring

| Item | Location |
|------|----------|
| Optional dep | `maxio-storage/Cargo.toml` — `openraft` under `raft-spike` feature |
| License gate | `make deny` / CI `licenses` job (include `--all-features` in P1-17 CI) |
| Smoke test | `crates/maxio-storage/src/raft_spike.rs` — `cargo test -p maxio-storage --features raft-spike` |

## Non-goals (spike)

- Full 3-node cluster
- Server-tier Raft (P1-20) — separate quorum, same library family
- Networking / TLS between nodes (P1-17 deliverable)

## References

- [OpenRaft docs](https://docs.rs/openraft/latest/openraft/)
- MaxIO `StorageBackend` — `crates/maxio-storage/src/backend.rs` (P1-15)