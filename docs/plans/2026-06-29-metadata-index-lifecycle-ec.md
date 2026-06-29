# P3-03 / P3-07 / P3-01 implementation notes

## P3-03 — SQLite metadata index

- Flag: `MAXIO_METADATA_INDEX` / `--metadata-index`
- Database: `{data_dir}/.maxio-metadata.db` (WAL mode)
- Hooks: upsert on put/multipart/folder writes; remove on delete
- Startup: full rebuild per bucket when enabled
- Fallback: filesystem `walk_dir` when index disabled

## P3-07 — Per-bucket erasure coding

- Field: `BucketMeta.erasure_coding: Option<bool>`
- API: `PUT/GET ?erasure` with `ErasureConfiguration` XML
- Write gate: `effective_erasure_coding(bucket)` — server flag must be on
- Read path: unchanged (layout-based via `.ec/`)

## P3-01 — Lifecycle expiration

- Field: `BucketMeta.lifecycle_rules: Vec<LifecycleRule>`
- API: `PUT/GET/DELETE ?lifecycle`
- Enforcement: hourly `housekeeping_sweep` phase 3
- v1: non-versioned buckets only; longest prefix match wins