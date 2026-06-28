# maxio-admin

Remote-first operations CLI for [MaxIO](https://github.com/coollabsio/maxio) instances.

## Status

**Scaffolding / stubs** — commands are wired to `/api/admin/v1/…` on the server. The server returns `501 Not Implemented` until [P2-13](../../docs/BACKLOG.md) lands; the CLI prints a stub JSON/human response when the API is unavailable.

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

## Commands

| Command | Scope | Description |
|---------|-------|-------------|
| `status` | remote | Health + readiness summary |
| `info` | remote | Disk, counts, server config |
| `doctor` | remote | Preflight checks |
| `buckets list` | remote | Bucket inventory |
| `buckets head <name>` | remote | Single bucket metadata |
| `housekeeping run` | remote | On-demand maintenance sweep |
| `keyring list` | remote | Keyring metadata (no secrets) |
| `keyring rotate --data-dir` | **local** | Stub — use `maxio keyring rotate` for now |

Global flags: `--profile`, `--endpoint`, `--json`, `--config`.

## Example

```bash
export MAXIO_ADMIN_ENDPOINT=http://127.0.0.1:9000
maxio-admin --json status
```