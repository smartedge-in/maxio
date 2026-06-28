# Licensing

MaxIO is released under [Apache-2.0](../LICENSE). This document describes third-party
licensing policy for Rust dependencies and embedded UI assets.

## Policy

Production artifacts (`maxio` server binary, embedded web console) should depend only on
**permissive** licenses:

- Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, 0BSD

We avoid copyleft (GPL/AGPL/LGPL), weak copyleft (MPL-2.0), and non-standard licenses
(CDLA, OFL) in paths that ship to users.

## Rust dependencies

CI and local `make deny` run [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny)
against [`deny.toml`](../deny.toml) with **licenses only** (`cargo deny check licenses`).
A full graph check — advisories, duplicate crates, sources — is available via `make deny-all`.

### SPDX allow-list

`deny.toml` allows only licenses present in the production dependency graph:

- Apache-2.0, MIT, BSD-3-Clause, Unicode-3.0

Other common permissive identifiers (ISC, Zlib, BSD-2-Clause, CC0-1.0, 0BSD) are acceptable
in principle but are not listed until a dependency actually requires them.

### Advisory policy

`make deny-all` and `cargo audit` may report transitive advisories that do not affect MaxIO
runtime behavior. These are documented and ignored in `deny.toml`:

| Advisory | Crate | Rationale |
|----------|-------|-----------|
| `RUSTSEC-2024-0384` | `instant` | Unmaintained; pulled in by `reed-solomon-erasure` → `parking_lot` 0.11 (Redox-only path) |
| `RUSTSEC-2026-0097` | `rand` 0.10 | Unsound only when a custom `log` logger is installed; MaxIO does not register one |

`cargo audit` prints these as **allowed warnings** (exit 0). Upgrade `rand` to ≥ 0.10.1 when
the workspace dependency graph permits.

### Recent dependency changes

| Change | Rationale |
|--------|-----------|
| Replaced `dirs` in `maxio-admin` with `XDG_CONFIG_HOME` / `HOME` resolution | Removes MPL-2.0 `option-ext` from the workspace |
| Switched `reqwest` from `rustls-tls` to `native-tls-vendored` | Avoids `ring` (dual Apache/ISC) and `webpki-roots` (CDLA-Permissive-2.0); bundles OpenSSL (Apache-2.0) for portable builds |
| Embedded UI fonts: Inter + JetBrains Mono | Replaces OFL-1.1 Geist fonts with MIT-licensed `@fontsource` packages |

`maxio-admin` and integration tests use OpenSSL via `reqwest`'s `native-tls-vendored` feature
(bundled at build time). Runtime images ship `ca-certificates` for TLS trust anchors.

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