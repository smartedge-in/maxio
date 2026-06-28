# maxio-admin

Remote-first operations CLI for [MaxIO](https://github.com/coollabsio/maxio) instances.

## Build

From the repository root:

```bash
cargo build -p maxio-admin
./target/debug/maxio-admin --help
```

## Configuration

```bash
maxio-admin config path
cp crates/maxio-admin/config.example.toml ~/.config/maxio/config.toml
```

Set `admin_token` to match the server's `MAXIO_ADMIN_TOKEN`, or provide `access_key` / `secret_key` for Basic auth fallback.

## Commands

| Command | Scope | Description |
|---------|-------|-------------|
| `status` | remote | Health + readiness summary |
| `info` | remote | Disk, counts, server config |
| `doctor` | remote | Preflight checks |
| `doctor --data-dir <path>` | **local** | Offline doctor (no network) |
| `buckets list` | remote | Bucket inventory |
| `buckets head <name>` | remote | Single bucket metadata |
| `housekeeping run` | remote | On-demand maintenance sweep |
| `keyring list` | remote | Keyring metadata (no secrets) |
| `keyring rotate --data-dir <path>` | **local** | Rotate on-disk SSE-S3 keyring |

Global flags: `--profile`, `--endpoint`, `--json`, `--config`.

## Example

```bash
export MAXIO_ADMIN_ENDPOINT=http://127.0.0.1:9000
export MAXIO_ADMIN_TOKEN=your-admin-token
maxio-admin --json status
maxio-admin doctor --data-dir ./data
```

See [docs/operations.md](../../docs/operations.md) for TLS, authentication, and production deployment guidance.