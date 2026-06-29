//! `maxio-admin` — remote-first operations CLI for MaxIO instances.
//!
//! Commands talk to the authenticated admin HTTP API (`/api/admin/v1/…`) on a
//! running server. Local-only maintenance (e.g. offline keyring rotate) uses
//! `--data-dir` explicitly.

pub mod cli;
pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;

pub use cli::Cli;
pub use error::{AdminError, Result};

#[cfg(test)]
mod tests {
    use maxio_common::version;

    #[test]
    fn cli_version_matches_common_crate() {
        assert!(!version::VERSION.is_empty());
    }
}
