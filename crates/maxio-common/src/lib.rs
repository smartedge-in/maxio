//! Thin shared contracts for MaxIO tiers (P1-22).
//!
//! No HTTP framework, storage I/O, or network clients — cluster RPC DTOs and constants only.

pub mod admin;
pub mod cluster;
pub mod version;

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_is_non_empty_semver() {
        assert!(!version::VERSION.is_empty());
        assert_eq!(version::VERSION.split('.').count(), 3);
    }
}