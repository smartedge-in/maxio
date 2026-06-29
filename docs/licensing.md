# Licensing

MaxIO is released under [Apache-2.0](../LICENSE). This document describes third-party
licensing policy for Rust dependencies and embedded UI assets.

## Mandatory requirement

**All code that ships in MaxIO production artifacts must use permissive licenses only.**

- **Preferred:** [Apache-2.0](../LICENSE) (same as MaxIO itself) for new Rust crates and libraries.
- **Also allowed:** MIT, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, 0BSD — when no Apache-2.0/MIT alternative exists.
- **Forbidden:** copyleft (GPL, AGPL, LGPL), weak copyleft (MPL-2.0), proprietary, and non-standard licenses (CDLA, OFL, custom EULAs) in any dependency path that reaches users.

This applies to **every workspace crate** (`maxio`, `maxio-common`, `maxio-storage`, `maxio-server`, `maxio-admin`, future `maxio-ui`) and to **embedded UI assets** (fonts, bundled static files).

New dependencies require license review before merge. CI **`make deny`** / `cargo deny check licenses` must pass. If a crate is Apache-2.0 **AND** another permissive license (e.g. ISC), both identifiers must be in [`deny.toml`](../deny.toml) `allow`.

### Edge and HA tooling (deployments)

Official MaxIO runbooks (P3-18, plain K8s manifests, P3-26) recommend **permissive** ingress/LB only — e.g. **Caddy** (Apache-2.0), **Traefik** (MIT), **MetalLB** (Apache-2.0). **Do not document keepalived or HAProxy CE** as the default path (GPL-2.0). See [`docs/plans/2026-06-29-permissive-ingress-ha.md`](plans/2026-06-29-permissive-ingress-ha.md).

### Adding a dependency (checklist)

1. Confirm SPDX identifier is on the `deny.toml` allow-list (or add it only if permissive).
2. Prefer Apache-2.0 or MIT crates over alternatives with stricter or compound licenses.
3. Avoid `rustls-tls` + `webpki-roots` (CDLA) — use `native-tls-vendored` for HTTP clients.
4. Run `make deny` locally before opening a PR.

### UI / npm (`ui/`)

Runtime dependencies (`package.json` → `dependencies`, not devDependencies) are audited via
[`scripts/check-npm-licenses.sh`](../scripts/check-npm-licenses.sh) in CI and locally via `make npm-licenses`.

| Allowed | Notes |
|---------|-------|
| Apache-2.0, MIT, BSD-*, ISC, 0BSD, CC0-1.0, Unlicense | Same spirit as Rust allow-list |
| **OFL-1.1** | **Only** `@fontsource/*` embedded fonts (not a general OFL pass) |

Build-time tools (Vite, Tailwind, TypeScript, etc.) live in `devDependencies` and are not
shipped in the embedded UI bundle; they are excluded from this audit.

## Rust dependencies

CI and local `make deny` run [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny)
against [`deny.toml`](../deny.toml) with **licenses only** (`cargo deny check licenses`).
A full graph check — advisories, duplicate crates, sources — is available via `make deny-all`.

### SPDX allow-list

`deny.toml` allows only licenses present in the production dependency graph:

- Apache-2.0, MIT, BSD-3-Clause, ISC, Unicode-3.0

Other common permissive identifiers (Zlib, BSD-2-Clause, CC0-1.0, 0BSD) are acceptable
in principle but are not listed until a dependency actually requires them.

### Advisory policy

`make deny-all` and `cargo audit` may report transitive advisories that do not affect MaxIO
runtime behavior. These are documented and ignored in `deny.toml`:

| Advisory | Crate | Rationale |
|----------|-------|-----------|
| `RUSTSEC-2024-0384` | `instant` | Unmaintained; pulled in by `reed-solomon-erasure` → `parking_lot` 0.11 (Redox-only path) |

`cargo audit` prints ignored advisories as **allowed warnings** (exit 0). Direct dependency
`rand` is pinned to ≥ 0.10.1 (fixes `RUSTSEC-2026-0097` / GHSA-cq8v-f236-94qc).

### Recent dependency changes

| Change | Rationale |
|--------|-----------|
| Replaced `dirs` in `maxio-admin` with `XDG_CONFIG_HOME` / `HOME` resolution | Removes MPL-2.0 `option-ext` from the workspace |
| `reqwest` uses `native-tls-vendored` (maxio-server, maxio-admin, integration tests) | Avoids `webpki-roots` (CDLA-Permissive-2.0) and rustls-only deps; bundles OpenSSL (Apache-2.0) for portable builds |
| `jsonwebtoken` (Keycloak JWT validation) | Adds `ring` (Apache-2.0 AND ISC) and `simple_asn1` (ISC); ISC added to `deny.toml` allow-list |
| Embedded UI fonts: Inter + JetBrains Mono | Replaces OFL-1.1 Geist fonts with MIT-licensed `@fontsource` packages |

`maxio-server` (Keycloak client), `maxio-admin`, and integration tests use OpenSSL via
`reqwest`'s `native-tls-vendored` feature (bundled at build time). Runtime images ship
`ca-certificates` for TLS trust anchors.

## UI (npm) dependencies

The web console (`ui/`) is built with Bun and embedded into the `maxio` binary. Font files
are the primary npm licensing concern; runtime JS libraries (Svelte, Tailwind, etc.) are
MIT or similarly permissive.

Embedded fonts (MIT):

- `@fontsource/inter` — UI sans-serif
- `@fontsource/jetbrains-mono` — monospace (keys, paths, code)

## Verifying locally

```bash
# Install developer tooling (cargo-deny, cargo-audit, Trivy, etc.)
make install-tools

# License audit — same as GitHub Actions (default)
make deny

# Full cargo-deny graph (licenses + advisories + bans + sources)
make deny-all

# RustSec advisory scan (may show allowed warnings for ignored advisories)
make audit

# Inspect a crate's license
cargo metadata --format-version 1 | jq '.packages[] | select(.name=="<crate>") | .license'
```

## Reporting issues

If you find a dependency with a non-permissive license in the production dependency tree,
please open an issue with the crate name, license SPDX identifier, and dependency path.