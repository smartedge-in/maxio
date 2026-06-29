# Shared library strategy (P3-21+)

## Status

Draft — architectural policy. P3-04 split `maxio-storage` / `maxio-server`; root `maxio` facade re-exports both. No `maxio-common` crate yet.

## Principle

**Share types and constants, not runtimes.** Each deployable component keeps its own dependency graph. A thin shared crate prevents drift; a fat shared crate recreates the monolith and blocks asymmetric scale-out (P3-13).

## Target dependency graph

```
                         ui/ (npm — not Rust)
                              │
maxio-common  ◄─────── maxio-admin (remote: common + reqwest)
 (thin)         ◄─────── maxio-server (handlers import common types)
     │                  maxio-replicate (P3-11 — event types from common or storage)
     │
maxio-storage ◄──────── maxio-server
 (storage SSOT) ◄─────── maxio-admin (local: --data-dir only)

maxio (root)  — facade binary only; not a dependency of sibling crates (P3-17)
maxio-ui      — static assets only (P3-16); no Rust shared lib with server
```

## What lives where

| Crate | Owns | Must not depend on |
|-------|------|-------------------|
| **maxio-common** | `VERSION`, admin API JSON types, shared constants, replication event schema (P3-10) | `axum`, `tokio` (full), `reqwest`, `maxio-storage`, `maxio-server` |
| **maxio-storage** | Filesystem, crypto, keys, policy, quota, `StorageError` | `axum`, `http`, S3 XML |
| **maxio-server** | HTTP/S3, auth, metrics, audit | — (depends on `common` + `storage`) |
| **maxio-admin** | CLI, HTTP client | `maxio` facade, `maxio-server` |
| **maxio-ui** | Static SPA | — (separate from Rust graph) |

## maxio-common scope (P3-22)

**Phase 1**

- `version::VERSION` (single source; root/server/admin read from common)
- Admin API types: `StatusResponse`, `InfoResponse`, `DoctorResponse`, bucket/keyring DTOs — today duplicated implicitly as JSON in server handlers and admin client
- Env/config name constants shared across CLI and server (`MAXIO_*` documentation anchors)

**Phase 2** (when P3-10 lands)

- Replication log event enum + serde schema in `common` or `storage` — prefer `storage` for mutation events, `common` only for cross-tier API contracts

**Explicit non-goals**

- No HTTP client in common
- No storage I/O in common
- No UI TypeScript in common (optional OpenAPI → TS codegen is a separate `ui/` concern)

## Crate boundary enforcement (P3-23)

CI policy (cargo-deny `bans` or custom script):

| Rule | Rationale |
|------|-----------|
| `maxio-admin` → deny `maxio`, `maxio-server` | P3-17 |
| `maxio-common` → deny `axum`, `maxio-server`, `maxio-storage` | Keep common thin |
| `maxio-storage` → deny `axum` | P3-04 boundary |
| Root `maxio` lib → allowed to depend on server + storage | Binary/facade only |

## Relationship to existing backlog

| Item | Role |
|------|------|
| P3-04 ✓ | Storage / server split |
| P3-17 | Admin decoupled from facade |
| P3-21 | Epic — shared library strategy |
| P3-22 | Implement `maxio-common` |
| P3-23 | CI dependency boundary checks |

## Acceptance (epic P3-21)

- [ ] `maxio-common` published in workspace with documented allow/deny deps
- [ ] Admin API types shared between server and `maxio-admin` (no JSON shape drift)
- [ ] `maxio-admin` depends on `common` + `storage` only
- [ ] CI fails on forbidden crate edges
- [ ] No requirement for UI, server, and storage to share one Rust library beyond this split