//! Release version — sourced from the repository root `VERSION` file at build time.

/// Semantic version string (e.g. `0.4.2`).
pub const VERSION: &str = env!("MAXIO_VERSION");