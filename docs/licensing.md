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

CI runs [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) against
[`deny.toml`](../deny.toml) on every push and pull request.

### Recent changes

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
# Install cargo-deny (once)
cargo install cargo-deny

# License audit (workspace, all features)
cargo deny check

# Inspect a crate's license
cargo metadata --format-version 1 | jq '.packages[] | select(.name=="<crate>") | .license'
```

## Reporting issues

If you find a dependency with a non-permissive license in the production dependency tree,
please open an issue with the crate name, license SPDX identifier, and dependency path.