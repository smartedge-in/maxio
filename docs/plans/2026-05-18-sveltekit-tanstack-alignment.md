# SvelteKit + TanStack Query architecture alignment

## Goal

Align MaxIO with `/Users/heyandras/devel/architecture/RUST_WEB_APP_SERVICES.md` for single-binary Rust web apps while preserving MaxIO-specific S3/object-storage requirements.

## Decisions

- Use SvelteKit static SPA in the existing `ui/` directory.
- Use `@sveltejs/adapter-static` with `fallback: '200.html'`, `ssr = false`, and `prerender = false`.
- Use TanStack Query for shared browser API/server state: auth, buckets, objects, versions, bucket settings.
- Keep direct one-off browser actions for file downloads, presigned URL copy/open, and other no-cache actions.
- Keep filesystem object metadata; do not introduce SQLite until user/account metadata needs it.
- Keep the single crate for now; a workspace split can happen later when module boundaries justify it.
- Keep `ui/` instead of renaming to `frontend/` to avoid churn.
- Add hardening and build/deploy hygiene from the guide: frontend build orchestration, static fallback to `200.html`, security headers, `/readyz`, explicit `serve`, version/tag checks, and Docker lockfile builds.
- Do not add a blanket small global request limit that breaks S3 uploads. Console JSON routes get a body limit; S3 upload limits remain streaming/product-configurable.

## Verification

- `cd ui && bun install --frozen-lockfile`
- `cd ui && bun run check`
- `cd ui && bun run build`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all --all-features`
- `cargo build --release --locked`
