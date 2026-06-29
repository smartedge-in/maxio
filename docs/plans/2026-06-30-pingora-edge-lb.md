# Pingora edge load balancing (P3-25) — feasibility

## Question

Can [Cloudflare Pingora](https://github.com/cloudflare/pingora) provide **native LB** across MaxIO storage and server components?

## Short answer

**Partially.** Pingora is a strong fit for the **HTTP edge** (UI + S3/API server tier). It is **not** a substitute for **storage Raft routing**, internal `StorageBackend` RPC, or object placement logic.

## What Pingora is

- Rust **L7 reverse proxy / load balancer framework** (Apache-2.0 — compatible with MaxIO policy, P3-24)
- HTTP/1, HTTP/2, TLS, customizable LB/failover, graceful reload, observability
- Linux-first; used at Cloudflare scale for proxying **HTTP** traffic

## Where it fits MaxIO

```
Clients
   │
   ▼
┌─────────────────────┐
│  maxio-edge (Pingora) │  ◄── P3-25: optional native LB
│  TLS, LB, health      │
└──────────┬──────────┘
           │ HTTP
     ┌─────┴─────┐
     ▼           ▼
 maxio-ui × K   maxio-server × N
                     │
                     ▼  StorageBackend (NOT Pingora)
                maxio-storage × M (Storage Raft)
```

| Tier | Pingora LB? | Why |
|------|-------------|-----|
| **maxio-ui** | Yes (optional) | Static HTTP; round-robin / least-conn across UI pods |
| **maxio-server** | Yes (primary use) | S3 + console API are HTTP; health via `/healthz` / `/readyz` |
| **maxio-storage** | No (default) | Not a public HTTP S3 endpoint; needs **Raft leader-aware** routing and streaming object I/O via `StorageBackend`, not generic HTTP proxy |

Server → storage traffic should use explicit backend client logic (P3-10), with optional **leader discovery** from Storage Raft — blind LB to storage nodes would break consistency.

## S3-specific concerns (server tier)

If Pingora fronts `maxio-server`:

| Concern | Mitigation |
|---------|------------|
| Large streaming PUT/GET | Disable buffering; long timeouts; `client_max_body_size` equivalent |
| SigV4 / presigned URLs | Preserve `Host`, `X-Forwarded-*`; consistent upstream selection per connection or sticky optional |
| Virtual-hosted-style `bucket.host` | Pingora must forward original `Host` |
| WebSocket (if added) | Pingora supports WS |
| TLS termination | Pingora or external Ingress — align with `MAXIO_TRUSTED_PROXIES` |

These are solvable but require **S3-aware proxy config**, not stock round-robin.

## Alternatives already in scope

| Approach | Backlog |
|----------|---------|
| nginx / Caddy / cloud LB | Documented in `docs/operations.md` |
| Kubernetes Ingress + Service | P3-19 Helm chart |
| Pingora as first-party `maxio-edge` | P3-25 |

External LBs remain valid; Pingora is optional **native Rust** edge for operators who want one toolchain.

## Recommended scope (P3-25)

**Do**

- Optional `crates/maxio-edge` binary built on `pingora-proxy` + `pingora-load-balancing`
- Upstream pools: `maxio-server` members, optional `maxio-ui` members
- Active health checks: `GET /readyz`
- Document TLS termination + `MAXIO_TRUSTED_PROXIES` on backends

**Do not (v1)**

- Pingora in front of storage Raft peers for metadata writes
- Replace Server Raft or Storage Raft with Pingora
- Embed Pingora inside `maxio-server` process (separate deployable)

## Non-goals

- Pingora as dependency of `maxio-storage`
- Mandatory Pingora for single-node installs
- Replacing K8s Ingress entirely (Helm can still use Ingress; Pingora is for bare metal or dedicated edge tier)

## Open questions

- Separate `maxio-edge` crate vs thin wrapper binary only?
- Sticky sessions for console cookies vs stateless UI (P3-16)?
- MSRV impact: Pingora rolling MSRV (currently 1.84) vs MaxIO edition 2024

## Acceptance (P3-25)

- [ ] `maxio-edge` proxies to ≥2 `maxio-server` backends with failover
- [ ] Integration test: edge → server pool → PUT/GET object
- [ ] Documented alongside nginx/Ingress in operations guide
- [ ] `make deny` passes (Pingora is Apache-2.0)