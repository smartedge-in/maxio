# Pingora load balancing (P3-25) — feasibility

## Decision (dropped)

**P3-25 / `maxio-edge` is not pursued.** Client-facing LB and TLS stay with **standard external components**:

- **Kubernetes:** Ingress + Service (P3-19)
- **Bare metal:** Caddy / Traefik + optional DNS or K8s MetalLB (P3-26) — **not** keepalived (GPL)
- **Single-node:** direct bind or local reverse proxy (`docs/operations.md`)

MaxIO focuses on S3 storage and cluster tiers (P3-13+), not shipping a custom edge proxy. Pingora remains a fine choice for operators who build their **own** edge — it is not a MaxIO product requirement.

---

## Questions (historical)

1. Can Pingora provide **native LB** across MaxIO storage and server components?
2. Should **`maxio-server` be built on Pingora natively** (not a separate front proxy) to support a **VIP with LB**?

## Short answers

| Model | Verdict |
|-------|---------|
| Separate `maxio-edge` proxy | **Dropped** — duplicates Ingress/nginx; not worth maintaining |
| **Rebuild `maxio-server` on Pingora** for VIP + LB | **Rejected** — wrong layer, overlaps Raft, huge Axum rewrite |

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

## Supported LB approach (instead of maxio-edge)

| Approach | Backlog / docs |
|----------|----------------|
| nginx / Caddy / cloud LB | `docs/operations.md` |
| Kubernetes Ingress + Service | P3-19 Helm chart |
| keepalived VIP (bare metal) | P3-18 |
| Server / Storage Raft routing | P3-14, P3-15 |

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

**Recommendation:** keep **Axum** for `maxio-server`; use **Server/Storage Raft** for cluster routing; use **keepalived / K8s Ingress / nginx** for VIP and client-facing LB.

## Non-goals

- Pingora as dependency of `maxio-storage`
- Rebuilding `maxio-server` on Pingora as the HTTP framework
- Pingora replacing Server Raft or VIP failover
- Mandatory Pingora for single-node installs
- Shipping a first-party Pingora-based `maxio-edge` binary