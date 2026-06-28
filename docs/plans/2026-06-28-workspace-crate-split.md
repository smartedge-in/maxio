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