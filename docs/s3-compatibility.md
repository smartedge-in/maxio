# S3 compatibility

MaxIO targets AWS S3 API compatibility for common object-storage workflows. This page summarizes routing, auth, and policy behaviour.

## URL styles

| Style | Example | Status |
|-------|---------|--------|
| Path-style | `http://endpoint/bucket/key` | Supported (default) |
| Virtual-hosted-style | `http://bucket.endpoint/key` | Supported (P1-09) |

### Virtual-hosted-style

- Configure the public endpoint host with `MAXIO_SERVER_HOST` / `--server-host` (e.g. `s3.example.com` or `localhost:9000`).
- When unset, MaxIO derives `{bind-address}:{port}` (loopback substitution when binding `0.0.0.0`).
- Requests with `Host: {bucket}.{server_host}` are dispatched to the correct bucket; object keys come from the URI path (`/key`).
- Signature V4 verification uses the **client path** (`/key`), not an internal rewritten path.
- Path-style requests continue to work on the same listener.

**TLS:** Terminate TLS at your proxy and forward `Host` unchanged.

## Authentication

- AWS Signature V4 for all mutating and private reads.
- Presigned URLs (`X-Amz-Signature` query param).
- Multiple static credential pairs via `.maxio-credentials.json` (see [multi-user credentials plan](plans/2026-06-28-multi-user-credentials.md)).

## Bucket policies (v1)

JSON policies via `?policy` with a restricted Allow/`Principal:*` subset. See [bucket policy evaluation](plans/2026-06-28-bucket-policy-evaluation.md).

## Console vs S3

The web console uses cookie sessions (not SigV4). S3 clients use access/secret keys from the same credential store.