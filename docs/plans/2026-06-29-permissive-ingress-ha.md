# Permissive ingress and HA (P3-26)

## Policy

MaxIO official docs and deployment packs (**P3-18**, plain K8s manifests) recommend only **permissive-licensed** (P3-24) edge and HA components.

**Do not recommend** in MaxIO runbooks:

| Tool | License | Why excluded |
|------|---------|--------------|
| **keepalived** | GPL-2.0 | Copyleft; conflicts with P3-24 |
| **HAProxy** (Community) | GPL-2.0 | Copyleft |
| **Corosync / Pacemaker** | GPL / LGPL mix | Copyleft stack |

**nginx** open source is BSD-2-Clause (permissive), but operators may prefer to avoid it (commercial Plus variants, policy). MaxIO docs **prefer Apache-2.0 / MIT** alternatives below.

## Recommended stack

### TLS + HTTP reverse proxy (bare metal or VM)

| Tool | License | Notes |
|------|---------|-------|
| **[Caddy](https://caddyserver.com/)** | Apache-2.0 | **Default** in MaxIO examples; automatic TLS, streaming proxy |
| **[Traefik](https://traefik.io/)** | MIT | LB + Ingress-style routing |
| **[Envoy](https://www.envoyproxy.io/)** | Apache-2.0 | Heavier; good for large clusters |

### Kubernetes

| Tool | License | Notes |
|------|---------|-------|
| **Ingress** (Traefik / Caddy ingress controller / Gateway API) | MIT / Apache-2.0 | `deploy/k8s/` manifests |
| **[MetalLB](https://metallb.io/)** | Apache-2.0 | Bare-metal or on-prem **LoadBalancer** VIP |
| **[kube-vip](https://kube-vip.io/)** | Apache-2.0 | Control-plane / Service VIP on K8s |

Prefer **Ingress + Service** or **MetalLB** over host **keepalived**.

### HA without GPL floating VIP

You do **not** need keepalived for most MaxIO layouts:

```
Option A — K8s (preferred multi-node)
  MetalLB or cloud LB → Service → maxio-server pods

Option B — Bare metal
  2+ Caddy/Traefik nodes + DNS (round-robin or health-checked)
  OR k3s on bare metal + MetalLB (Apache-2.0)

Option C — DNS only
  Multiple A records → each server behind local Caddy on :443
  Server Raft (P3-15) + LB health checks on /readyz
```

Floating VIP (VRRP) is optional; **permissive VIP** on K8s uses **MetalLB / kube-vip**, not keepalived.

### Pingora edge (`knx-edge`)

**Not in MaxIO.** Backlog P3-25 (`maxio-edge`) is dropped. **Caddy** (Apache-2.0) is the
default permissive path.

**Optional alternative:** [knx-edge](https://github.com/smartedge-in/knx-edge) — separate
Apache-2.0 Pingora L7 gateway (Phase 1 proxy/LB shipped; Phase 2 permissive VIP research).
MaxIO links example configs only. See [`docs/out-of-tree/knx-edge.md`](../out-of-tree/knx-edge.md).

## P3-18 / K8s deliverables (updated)

- Example **Caddyfile** (not nginx) in `docs/operations.md`
- Bare-metal multi-node: Caddy or Traefik LB tier — **no keepalived** in official pack
- Plain K8s: Traefik or Caddy Ingress Controller manifests; optional **MetalLB** manifest stub under `deploy/k8s/`
- Helm chart (P3-19) optional later — not required for P3-26

## Acceptance (P3-26)

- [ ] `docs/operations.md` uses Caddy as primary TLS proxy example
- [ ] GPL edge tools (keepalived, HAProxy CE) listed as **not recommended**
- [ ] P3-18 runbook references this doc
- [ ] K8s manifests reference permissive Ingress / MetalLB options