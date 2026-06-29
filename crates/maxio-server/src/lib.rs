pub mod api;
pub mod audit;
pub mod auth;
pub mod cluster;
pub mod cluster_sync;
pub mod config;
pub mod embedded;
pub mod error;
pub mod metrics;
pub mod proxy;
pub mod rate_limit;
pub mod server;
pub mod version;
pub mod xml;

/// Re-export the storage crate so server modules keep `crate::storage::…` paths.
pub use maxio_storage as storage;

#[cfg(test)]
mod server_cluster_tests;

#[cfg(test)]
mod crate_boundary_tests {
    use crate::error::{S3ErrorCode, map_storage_upload_error};
    use crate::storage::StorageError;

    #[test]
    fn maps_storage_errors_to_s3_errors() {
        let err = map_storage_upload_error(StorageError::InvalidKey(
            "MalformedPolicy: only Effect=Allow".into(),
        ));
        assert!(matches!(err.code, S3ErrorCode::InvalidArgument));
    }

    #[test]
    fn storage_reexport_is_accessible() {
        assert!(crate::storage::validate_bucket_name("my-bucket").is_ok());
    }
}
