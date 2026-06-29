# UI separation and stateless scale-out (P3-16)

## Status

Draft — architectural requirement. UI is embedded in `maxio-server` today (`rust-embed` → `/ui/*`).

## Requirement

The web console **must** become a **separate deployable component** (`crates/maxio-ui` or equivalent workspace crate) that is:

1. **Decoupled** from `maxio-server` — no `rust-embed` in the API binary for scale-out deployments.
2. **Stateless** — serves static SPA assets only; no server-side sessions, cookies, or `data_dir`.
3. **Horizontally scalable** — any UI replica is interchangeable; scale replica count independently of server and storage tiers.

The UI tier does **not** participate in Raft. Consensus stays on storage (P3-14) and server (P3-15) only.

## Current vs target

| Today | Target |
|-------|--------|
| `ui/build` embedded in `maxio-server` | `maxio-ui` binary or container serves `ui/build` |
| Same pod serves `/ui` and `/api` | Ingress splits `/ui` → UI service, `/api` → server service |
| Dev proxy in Vite (`vite.config.ts`) | Production: UI calls API via configurable base URL |

## Target topology

```
                    ┌─────────────────────────┐
                    │   maxio-ui × K          │  stateless static (no Raft)
 Browser ──────────►│   Svelte SPA assets     │  HPA / CDN-friendly caching
                    │   /ui/* only            │
                    └───────────┬─────────────┘
                                │ fetch /api/*  (cookies on API host)
                    ┌───────────▼─────────────┐
                    │   maxio-server × N      │  Server Raft (P3-15)
                    │   S3 + /api/*           │
                    └───────────┬─────────────┘
                                │
                    ┌───────────▼─────────────┐
                    │   maxio-storage × M     │  Storage Raft (P3-14)
                    └─────────────────────────┘
```

## Stateless rules

- **UI pod** — GET static files only (`200.html`, hashed JS/CSS). No POST handlers except optional health.
- **Auth state** — `maxio_session` / Keycloak cookies set by **`maxio-server`** on `/api/auth/*`; UI uses `credentials: 'include'` against API origin.
- **App state** — TanStack Query in browser; no sticky sessions to UI pods.
- **Config** — Runtime `window.__MAXIO_API_BASE__` or build-time `VITE_API_BASE` so UI replicas need no per-pod configuration.

## Crate / workspace layout (proposed)

```
crates/
  maxio-ui/           # NEW — static file server binary (~embedded.rs moved here)
  maxio-server/       # drops rust-embed; optional --serve-ui=false default in distributed mode
  maxio-storage/
ui/                   # Svelte source (unchanged); build artifact consumed by maxio-ui
```

Single-node dev convenience: root `maxio` binary may still bundle UI embed behind a flag, or `docker compose` runs `maxio-ui` + `maxio-server` side by side.

## Deliverables (P3-16)

| Item | Detail |
|------|--------|
| `crates/maxio-ui` | Workspace member; serves `ui/build` with same cache/ETag behaviour as today |
| Remove embed from server | `maxio-server` API-only in distributed profile; document migration |
| API base URL | SPA resolves API host for split-origin deploys; CORS/cookie `SameSite` documented |
| K8s / ops | Separate Deployment + Service for UI; sample ingress paths |
| Scale test | ≥2 UI replicas behind load balancer; login → upload flow unchanged |

## Relationship to P3-13 epic

P3-13 asymmetric scale-out closes when **storage Raft**, **server Raft**, and **stateless UI tier** are all shippable. UI is the third independently scalable tier (no Raft).

## Non-goals (v1)

- SSR or server-side SvelteKit endpoints in `maxio-ui`
- UI tier Raft or shared session store
- Embedding UI in `maxio-server` for production distributed deploy (dev/single-binary may retain optional embed)