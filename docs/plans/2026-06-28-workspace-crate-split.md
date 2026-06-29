# Workspace crate split (P3-04)

## Goal

Split the monolithic `maxio` library into `maxio-storage` and `maxio-server` crates with a stable facade at the workspace root.

## Layout

```
maxio/                  # facade + binary
crates/
  maxio-storage/        # filesystem, crypto, keys, policy, quota
  maxio-server/         # api, auth, server, embedded UI, metrics, audit
  maxio-admin/          # unchanged; depends on facade
```

## Boundaries

- **maxio-storage** has no `axum`, `http`, or S3 XML types. `StorageError` stays in storage.
- **maxio-server** depends on `maxio-storage` and maps storage errors to `S3Error` via `map_storage_upload_error()`.
- **maxio** (root) re-exports both crates so `maxio::storage::*` and `maxio::server::*` remain the public API.

## Build

- UI embed path: `crates/maxio-server` uses `#[folder = "../../ui/build"]`.
- Frontend `build.rs` logic lives in `maxio-server/build.rs`; root `build.rs` emits `MAXIO_VERSION` only.

## Tests

- Crate-boundary unit tests in each library (`maxio-storage`, `maxio-server`, root facade).
- Integration tests remain at `tests/integration.rs` on the root package.

## Follow-on: asymmetric scale-out (P3-13)

The split enables **independent scaling** once a runtime boundary exists:

```
Clients ──► maxio-server × N   (HTTP/S3, auth, console — stateless)
                 │
                 ▼  remote StorageBackend (from P3-10)
            maxio-storage × M  (filesystem, keys, quota — owns data_dir)
```

Today both crates still ship in one process with a local `data_dir`. P3-13 tracks deploying and scaling each tier with different replica counts; blocked on `StorageBackend` trait work in P3-10.