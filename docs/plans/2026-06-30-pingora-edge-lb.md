# Pingora load balancing (P3-25) — feasibility

## Questions

1. Can Pingora provide **native LB** across MaxIO storage and server components?
2. Should **`maxio-server` be built on Pingora natively** (not a separate front proxy) to support a **VIP with LB**?

## Short answers

| Model | Verdict |
|-------|---------|
| Separate `maxio-edge` proxy | **Optional** — reasonable for HTTP ingress to server/UI pools |
| **Rebuild `maxio-server` on Pingora** for VIP + LB | **Not recommended** — wrong layer, overlaps Raft, huge Axum rewrite |

Pingora is **not** a substitute for **storage Raft routing**, **Server Raft** (P3-15), or **VIP failover** at the network layer.

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

## Embedded Pingora inside `maxio-server` (VIP + native LB)

Some designs propose **building the server tier on Pingora** so each node participates in LB and exposes a **virtual IP (VIP)** without an external proxy.

### What Pingora actually provides

- **Upstream load balancing** — pick a backend from a pool for **forwarded** HTTP requests
- **Health checks, failover, graceful reload** on proxy paths
- It does **not** implement **VIP/VRRP** (floating IP), **BGP**, or **keepalived** — those stay in the OS, network, or K8s Service layer

### What MaxIO server is today

- **Origin application** on **Axum** — S3 handlers, SigV4, streaming I/O, console API, middleware, tests
- Pingora is a **proxy framework** — optimized for “terminate client → forward to upstream,” not for hosting a full S3 implementation as the origin

Rebuilding `maxio-server` on Pingora would mean reimplementing the entire HTTP surface in `ProxyHttp` callbacks instead of Axum — a **multi-month rewrite** with little gain over Axum + hyper for an origin server.

### VIP + HA — where it belongs

| Mechanism | Layer | Fits MaxIO plan |
|-----------|--------|-----------------|
| **VIP / VRRP** (keepalived) | Host / network | Bare metal (P3-18); often **one active** holder, not LB across all nodes |
| **K8s Service / Ingress** | Cluster | P3-19 Helm |
| **Server Raft** (P3-15) | Control plane | Membership, routing epoch, storage leader map |
| **Pingora inside server** | Application | **Overlaps** Raft + external VIP; adds complexity |

Typical HA pattern for server tier:

```
VIP or LB ──► any maxio-server member (all run same Axum S3 app)
                    │
                    └── StorageBackend ──► Storage Raft leader (explicit, not round-robin)
```

VIP floats to **one** node (active/passive) **or** an **external** LB distributes to **all** server replicas. Neither pattern requires Pingora **inside** the server binary.

### Why embedded Pingora is a weak fit

1. **Wrong abstraction** — S3 server is origin, not a proxy to itself.
2. **VIP ≠ Pingora LB** — VIP is network/cluster; Pingora selects HTTP upstreams.
3. **Overlaps P3-15** — Server Raft already owns peer list and routing generation.
4. **Storage path** — Needs **leader-aware** `StorageBackend`, not Pingora round-robin to storage nodes.
5. **Cost** — Axum → Pingora migration touches every route, test, and middleware.
6. **Ops** — Pingora is Linux-first with OpenSSL/BoringSSL build deps; couples release to Pingora MSRV.

### Limited exception (outbound only)

The only plausible **in-process** use: Pingora’s LB **client** pool for **server → storage HTTP** transport **if** storage exposes HTTP and leader address comes from **Storage Raft**, not blind LB. Even then, a thin `StorageBackend` client (hyper/reqwest + leader watch) is simpler and easier to test.

**Recommendation:** keep **Axum** for `maxio-server`; use **Server/Storage Raft** for cluster routing; use **keepalived / K8s Service / optional `maxio-edge`** for VIP and client-facing LB.

## Non-goals

- Pingora as dependency of `maxio-storage`
- Rebuilding `maxio-server` on Pingora as the HTTP framework
- Pingora replacing Server Raft or VIP failover
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