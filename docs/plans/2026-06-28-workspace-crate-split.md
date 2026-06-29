# Workspace crate split (P3-04)

## Goal

Split the monolithic `maxio` library into `maxio-storage` and `maxio-server` crates with a stable facade at the workspace root.

## Layout

```
maxio/                  # facade + binary
crates/
  maxio-storage/        # filesystem, crypto, keys, policy, quota
  maxio-server/         # api, auth, metrics, audit (UI embed → P3-16 extract)
  maxio-ui/             # planned (P3-16) — stateless static console server
  maxio-admin/          # ops CLI — decouple from facade (P3-17)
ui/                     # Svelte source → built assets for maxio-ui
```

## Boundaries

- **maxio-storage** has no `axum`, `http`, or S3 XML types. `StorageError` stays in storage.
- **maxio-server** depends on `maxio-storage` and maps storage errors to `S3Error` via `map_storage_upload_error()`.
- **maxio** (root) re-exports both crates so `maxio::storage::*` and `maxio::server::*` remain the public API.
- **maxio-admin** should depend on `maxio-storage` only (P3-17), not the root facade or `maxio-server`.

## Build

- UI embed path: `crates/maxio-server` uses `#[folder = "../../ui/build"]`.
- Frontend `build.rs` logic lives in `maxio-server/build.rs`; root `build.rs` emits `MAXIO_VERSION` only.

## Tests

- Crate-boundary unit tests in each library (`maxio-storage`, `maxio-server`, root facade).
- Integration tests remain at `tests/integration.rs` on the root package.

## Follow-on: asymmetric scale-out with dual Raft (P3-13+)

The split enables **independent scaling** with **two separate Raft clusters** (not one global quorum):

```
Clients ──► maxio-server × N   Server Raft (own quorum, P3-15)
                 │
                 ▼  StorageBackend RPC (P3-10)
            maxio-storage × M   Storage Raft (own quorum, P3-14)
```

Design: `docs/plans/2026-06-29-distributed-scale-raft.md`, `docs/plans/2026-06-29-ui-scale-out.md`. Today server embeds UI and both ship with a local `data_dir` and no Raft.