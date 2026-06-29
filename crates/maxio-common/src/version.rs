//! Release version — sourced from the repository root `VERSION` file at build time.

/// MaxIO semantic version string (e.g. `0.4.2`).
pub const VERSION: &str = env!("MAXIO_VERSION");