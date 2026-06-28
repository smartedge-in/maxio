//! MaxIO facade crate — re-exports `maxio-server` and `maxio-storage` for a stable public API.

pub use maxio_server::{
    api, audit, auth, config, embedded, error, metrics, proxy, rate_limit, server, version, xml,
};
pub use maxio_storage as storage;

#[cfg(test)]
mod reexport_tests {
    use crate::{storage, version};

    #[test]
    fn facade_reexports_storage_validation() {
        assert!(storage::validate_bucket_name("valid-bucket").is_ok());
        assert!(storage::validate_bucket_name("../evil").is_err());
    }

    #[test]
    fn facade_reexports_version() {
        assert!(!version::VERSION.is_empty());
    }

    #[test]
    fn facade_reexports_server_modules() {
        // Compile-time boundary: public modules remain reachable through `maxio::`.
        let _ = std::any::type_name::<crate::server::AppState>();
    }
}
