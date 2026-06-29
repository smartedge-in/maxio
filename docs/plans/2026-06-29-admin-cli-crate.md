# Admin CLI crate boundary (P3-17)

## Status

`crates/maxio-admin` exists (P2-12) but still depends on the root `maxio` facade (`path = "../.."`), pulling the full server + storage re-export graph into the CLI binary.

## Requirement

`maxio-admin` **must** remain a **standalone workspace crate** with a hard dependency boundary:

| May depend on | Must not depend on |
|---------------|-------------------|
| `maxio-storage` (local `doctor --data-dir`, `keyring rotate`) | Root `maxio` facade |
| `reqwest` (remote `/api/admin/v1/*`) | `maxio-server`, `axum`, embedded UI |
| Shared version/constants crate or `maxio-server::version` | In-process server startup |

The CLI is an **operator client** — stateless, not a cluster tier. It does not scale out like `maxio-ui`; it runs on laptops, CI, or jump hosts and talks to any server API endpoint.

## Current coupling

```toml
# crates/maxio-admin/Cargo.toml (today)
maxio = { path = "../.." }   # ← pulls facade → maxio-server + maxio-storage
```

Used for:

- `maxio::storage::keys` — local keyring rotate/list
- `maxio::storage::filesystem` — offline doctor
- `maxio::version::VERSION` — clap `--version`

## Target layout

```
crates/
  maxio-admin/     # binary: maxio-admin — remote-first ops CLI
  maxio-storage/   # direct dep for local-only commands only
  maxio-server/    # not a dependency of maxio-admin
```

```toml
# target
maxio-storage = { path = "../maxio-storage" }
# version: workspace.package.version or tiny maxio-version crate
```

## Command classes

| Class | Transport | Crate deps |
|-------|-----------|------------|
| **Remote** | HTTP → server Raft leader / any API pod | `reqwest` only |
| **Local** | `--data-dir` filesystem | `maxio-storage` |

Remote commands must never require linking `maxio-server`; they use the admin API contract only.

## Scale / deploy model

- **Not** a replicated service — no Raft, no K8s Deployment for `maxio-admin`
- **Separate release artifact** — own binary in GitHub releases and Docker image (`ghcr.io/.../maxio-admin`)
- **Profiles** — `~/.config/maxio/config.toml` points at server tier load balancer or any API member
- **Distributed ops** — `maxio-admin status` works against server LB; server Raft handles routing to storage

## Deliverables (P3-17)

1. Replace `maxio` facade dep with `maxio-storage` + shared version source.
2. Crate-boundary test: `maxio-admin` `Cargo.toml` has no path dep on workspace root or `maxio-server`.
3. Release workflow publishes `maxio-admin` binary independently of `maxio` server binary.
4. Docs: `docs/operations.md` — CLI is client-only; never colocated requirement on server pods.

## Relationship to P3-13

P3-13 covers **in-cluster tiers** (UI, server, storage). P3-17 is a **build/release boundary** item — completes the workspace split started in P3-04 for all operator-facing binaries.