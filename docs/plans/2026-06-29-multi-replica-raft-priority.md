# Priority 1 — Multi-replica architecture & erasure coding (Raft-first)

## Status

Active product direction (2026-06). Supersedes the interim operator-sync replication track (P3-09–P3-11) as the **primary** path to multi-node.

## Goals

1. **Live multi-replica cluster** — asymmetric tiers (UI, server, storage) with **dual independent Raft** (P1-14 epic).
2. **Erasure coding at cluster scope** — extend single-node EC (shipped) to **shard placement across storage nodes** (P1-18, P1-19).
3. **Licensing control** — all new deps (Raft, RPC) pass P3-24 permissive-only `cargo deny`.

## Non-goals (this track)

- Operator `rsync`/`rclone` as the main product path (deferred: P3-09, P3-11).
- `maxio-replicate` sidecar before Storage Raft ships (P3-11 deferred).
- RustFS or other third-party store for multi-node.

## Architecture (target)

See `docs/plans/2026-06-29-distributed-scale-raft.md` and `docs/plans/2026-06-29-ui-scale-out.md`.

```
Clients → Ingress → maxio-ui × K (stateless)
                 → maxio-server × N (Server Raft, P1-20)
                 → maxio-storage × M (Storage Raft, P1-17)
                      ├─ metadata via Raft
                      └─ object bytes + EC shards on local disk per node
```

## Implementation order

| Order | ID | Deliverable |
|-------|-----|-------------|
| 1 | P1-15 | `StorageBackend` trait — all mutations through trait |
| 2 | P1-16 | Raft library spike + `deny.toml` gate |
| 3 | P1-22 | `maxio-common` — cluster RPC types, versions |
| 4 | P1-17 | Storage Raft — metadata quorum |
| 5 | P1-18 | Distributed EC — shard map in Raft; stripes across nodes |
| 6 | P1-19 | Multi-node EC read/rebuild — fetch parity from peers |
| 7 | P1-20 | Server Raft — routing snapshot, scale API pods |
| 8 | P1-21 | Stateless `maxio-ui` |
| 9 | P1-24 | 3-node test harness (`kind` / bare metal / plain K8s YAML — not Helm) |

## Erasure coding: single-node vs cluster

| | Today | P1-18+ |
|--|-------|--------|
| Layout | All chunks on one host | Data + parity shards on **distinct** storage nodes |
| Recovery | Local RS | RS + **peer shard fetch** |
| Metadata | `.meta.json` / sidecars | Raft-replicated shard map |
| Scope | `docs/operations.md` § EC | New `docs/plans/` EC-distributed section |

Single-node EC remains default when `M=1` (Raft disabled or one-member cluster).

## Deferred items

P3-09, P3-11, P3-12 (operator/agent replication) may be revisited **after** P1-14 closes for geo-DR tooling, not as a substitute for Raft.

## Acceptance (epic P1-14)

- [ ] 3-node storage Raft quorum; leader failover; PUT/GET integration test
- [ ] EC object with shards on ≥2 nodes; rebuild after single node loss (with parity)
- [ ] 2+ server pods behind Service LB; routing survives storage leader change
- [ ] 2+ UI replicas; colocated single-node mode still works (Raft off, embed UI)
- [ ] All new deps pass `make deny`

## After P1-14

Enterprise production path is **Phase 2 (GA)** then **Phase 3 (GA+)** in `docs/BACKLOG.md` — **P3-53 airgap** + epic **P3-52** → milestone **P3-44**. Raft-first; airgap-first install; Helm deferred; plain K8s + bare metal.