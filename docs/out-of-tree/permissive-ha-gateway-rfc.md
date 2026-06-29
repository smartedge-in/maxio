# Out-of-tree RFC: permissive HA gateway (Pingora)

> **Not a MaxIO component.** This document proposes a **separate open-source project**
> (its own repo, release cycle, and maintainers). MaxIO may *consume* it as an optional
> edge tier (like Caddy or Traefik) but will not embed or ship it in the `maxio` binary.

## Project

**[knx-edge](https://github.com/smartedge-in/knx-edge)** — separate Apache-2.0 repo (Pingora L7 gateway).

## Problem

Operators who follow a **permissive-only** policy (see MaxIO P3-24 / P3-26) want to avoid:

| Tool | License | Role |
|------|---------|------|
| **keepalived** | GPL-2.0 | Floating VIP (VRRP) |
| **HAProxy CE** | GPL-2.0 | L4/L7 LB |
| **nginx** | BSD (often avoided on policy) | L7 reverse proxy + TLS |

MaxIO documents **Caddy**, **Traefik**, and **MetalLB** instead. Some teams still want a
**single Rust-native, Apache-2.0** edge + HA story comparable to “nginx + keepalived” in one
toolchain — without making that MaxIO’s problem.

## Is this a good idea?

**Partially — if scoped in phases and kept out of MaxIO.**

| Scope | Verdict | Why |
|-------|---------|-----|
| **L7 gateway on Pingora** (nginx-like) | **Good separate project** | Pingora is Apache-2.0, proven at scale; clear product boundary |
| **Full keepalived clone (VRRP)** in v1 | **Too ambitious** | VRRP + netlink + ARP is security-sensitive, years of edge cases |
| **Combined “one binary replaces both”** day one | **Risky** | Two different problems (L3 VIP vs L7 HTTP); scope explosion |

**Recommendation:** green-light a **sibling OSS project** for **Phase 1 (L7 only)**; treat
VIP failover as **Phase 2** with a **permissive design** (leader-elected IP bind or BGP),
not a GPL VRRP reimplementation.

## What Pingora can and cannot do

```
┌─────────────────────────────────────────────────────────┐
│  nginx-like (YES — Pingora core competency)              │
│  • TLS termination                                     │
│  • Reverse proxy, upstream LB, health checks           │
│  • HTTP/1, HTTP/2, graceful reload                     │
│  • Custom routing / observability                      │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  keepalived-like (NO — not in Pingora today)             │
│  • VRRP / floating VIP on L2                           │
│  • Kernel netlink ARP mastership                       │
│  • Needs separate Rust component or host integration   │
└─────────────────────────────────────────────────────────┘
```

## Proposed architecture (separate repo)

### Phase 1 — L7 gateway (`knx-edge` binary)

Apache-2.0 stack: **Pingora** + config file (TOML/YAML) + upstream pools.

```
Clients ──► knx-edge (Pingora) ──► upstream pool
                                      ├─ app-1:9000
                                      ├─ app-2:9000
                                      └─ app-3:9000
```

**Features (MVP)**

- Reverse proxy with streaming bodies (large uploads)
- Active health checks (`GET /healthz` or custom)
- TLS (ACME or file certs)
- `X-Forwarded-*` preservation
- Prometheus metrics
- Example configs for MaxIO, generic HTTP backends

**Not MaxIO-specific** — S3-aware routing is optional plugin/config, not required.

### Phase 2 — Permissive VIP / failover (research)

Alternatives to GPL **keepalived**:

| Approach | License | Notes |
|----------|---------|-------|
| **Leader + netlink IP bind** | Apache-2.0 (this project) | Raft/consul-lite leader holds VIP; followers drop IP |
| **BGP / anycast** | Depends on daemon (FRR is GPL) | Often needs routing team |
| **Delegate to MetalLB / kube-vip** | Apache-2.0 | K8s-only; document integration, don’t reimplement |
| **VRRP in Rust** | Apache-2.0 possible | High effort; security audit burden |

Suggested Phase 2 MVP: **small `knx-vip` sidecar** — watches Raft/etcd/lease API; on
leadership, `ip addr add VIP/32`; on step-down, removes. Not full VRRP; good enough for
many bare-metal pairs.

```
┌──────────────┐     leader lease      ┌──────────────┐
│  knx-vip     │ ◄──────────────────► │  knx-vip     │
│  + knx-edge  │                      │  (standby)   │
│  holds VIP   │                      │  no VIP      │
└──────────────┘                      └──────────────┘
```

## Relationship to MaxIO

| | MaxIO | This project |
|--|-------|--------------|
| Repo | `maxio` | **New repo** |
| License | Apache-2.0 | Apache-2.0 (proposed) |
| Scope | S3 storage, server, UI | Generic L7 edge (+ optional VIP) |
| Backlog | P3-25 dropped | Own roadmap |
| Docs | Points to Caddy **or** optional `knx-edge` | Installation, HA patterns |

MaxIO **does not** need to own or block on this project. P3-26 (Caddy/Traefik/MetalLB)
remains the **official** permissive path.

## Why not inside MaxIO?

1. **Scope** — edge HA is a general infra product, not object storage.
2. **Maintenance** — Pingora MSRV, Linux-only ops, proxy CVEs — separate release train.
3. **Licensing narrative** — keeps MaxIO focused on permissive *application* code.
4. **Adoption** — other services (Polaris, internal APIs) can use the same edge without pulling `maxio-server`.

## Risks

- **Caddy already exists** (Apache-2.0, simpler ops) — differentiate on Pingora performance / programmability
- **Team bandwidth** — two-phase project still needs dedicated maintainers
- **VIP Phase 2** — easy to get wrong (split-brain, ARP storms)
- **Pingora** — Linux-first; Windows unsupported

## Go / no-go

| Decision | Recommendation |
|----------|----------------|
| Separate project at all? | **Go** — if team wants Rust-native permissive edge |
| Phase 1 (Pingora L7 only)? | **Go** — reasonable 1–2 month MVP |
| Phase 2 (VIP)? | **Defer** — spike leader+netlink; avoid VRRP clone |
| Inside MaxIO repo? | **No** |
| Replace P3-26 Caddy docs? | **No** — optional alternative |

## Suggested next steps (if approved)

1. Repo: [`github.com/smartedge-in/knx-edge`](https://github.com/smartedge-in/knx-edge) (Apache-2.0).
2. Spike: Pingora proxy → 2 static upstreams, health check, TLS.
3. Publish MaxIO example config (reference only, in both repos).
4. Phase 2 RFC only after Phase 1 adoption feedback.

## References

- [Cloudflare Pingora](https://github.com/cloudflare/pingora) — Apache-2.0
- MaxIO `docs/plans/2026-06-29-permissive-ingress-ha.md` (P3-26)
- MaxIO `docs/plans/2026-06-30-pingora-edge-lb.md` (why `maxio-edge` was dropped)