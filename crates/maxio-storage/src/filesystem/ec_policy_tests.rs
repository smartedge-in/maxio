#[cfg(test)]
mod tests {
    use crate::keys::Keyring;
    use crate::quota::QuotaLimits;
    use crate::{BucketMeta, ByteStream};
    use std::io::Cursor;
    use std::sync::Arc;
    use tempfile::TempDir;

    use crate::filesystem::FilesystemStorage;

    async fn storage_with_ec(data_dir: &str, server_ec: bool) -> FilesystemStorage {
        let keyring = Arc::new(Keyring::load(data_dir, None).await.unwrap());
        FilesystemStorage::new(
            data_dir,
            server_ec,
            1024,
            0,
            keyring,
            QuotaLimits::from_config(0, 0),
            false,
        )
        .await
        .unwrap()
    }

    async fn create_bucket(storage: &FilesystemStorage, name: &str, ec: Option<bool>) {
        storage
            .create_bucket(&BucketMeta {
                name: name.into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
                region: "us-east-1".into(),
                versioning: false,
                cors_rules: None,
                encryption_config: None,
                public_read: false,
                public_list: false,
                bucket_policy: None,
                erasure_coding: ec,
                lifecycle_rules: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn per_bucket_ec_disabled_writes_flat_while_server_ec_enabled() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        let storage = storage_with_ec(dir, true).await;
        create_bucket(&storage, "flat-only", Some(false)).await;

        let body: ByteStream = Box::pin(Cursor::new(b"plain".to_vec()));
        storage
            .put_object("flat-only", "obj.txt", "text/plain", body, None, None, None)
            .await
            .unwrap();

        let ec_dir = storage.ec_dir("flat-only", "obj.txt");
        assert!(!ec_dir.exists());
        assert!(storage.object_path("flat-only", "obj.txt").exists());
    }

    #[tokio::test]
    async fn per_bucket_ec_enabled_writes_chunked_layout() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        let storage = storage_with_ec(dir, true).await;
        create_bucket(&storage, "chunked", None).await;

        let body: ByteStream = Box::pin(Cursor::new(vec![0u8; 2048]));
        storage
            .put_object(
                "chunked",
                "big.bin",
                "application/octet-stream",
                body,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(
            storage
                .ec_dir("chunked", "big.bin")
                .join("manifest.json")
                .exists()
        );
    }
}
