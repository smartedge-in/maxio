# knx-edge (out of tree)

Pingora L7 edge load balancing lives in the separate **knx-edge** project — not in MaxIO.

| Topic | Location |
|-------|----------|
| Repository | [github.com/smartedge-in/knx-edge](https://github.com/smartedge-in/knx-edge) |
| Architecture | [knx-edge/docs/ARCHITECTURE.md](https://github.com/smartedge-in/knx-edge/blob/main/docs/ARCHITECTURE.md) |
| Pingora LB design (was MaxIO P3-25) | [knx-edge/docs/pingora-edge-lb.md](https://github.com/smartedge-in/knx-edge/blob/main/docs/pingora-edge-lb.md) |
| Permissive HA RFC | [knx-edge/docs/permissive-ha-gateway-rfc.md](https://github.com/smartedge-in/knx-edge/blob/main/docs/permissive-ha-gateway-rfc.md) |
| MaxIO example config | [knx-edge/docs/maxio-example.md](https://github.com/smartedge-in/knx-edge/blob/main/docs/maxio-example.md) |

MaxIO backlog **P3-25** (`maxio-edge`) is dropped. Official permissive ingress remains
**Caddy**, **Traefik**, and **MetalLB** (P3-26). knx-edge is an optional Pingora alternative.