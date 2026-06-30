#[cfg(test)]
mod tests {
    use crate::keys::Keyring;
    use crate::quota::QuotaLimits;
    use crate::{BucketMeta, ByteStream};
    use std::io::Cursor;
    use std::sync::Arc;
    use tempfile::TempDir;

    use crate::filesystem::FilesystemStorage;

    #[tokio::test]
    async fn metadata_index_matches_filesystem_listing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        let keyring = Arc::new(Keyring::load(dir, None).await.unwrap());
        let storage = FilesystemStorage::new(
            dir,
            false,
            1024,
            0,
            keyring,
            None,
            QuotaLimits::from_config(0, 0),
            true,
        )
        .await
        .unwrap();

        storage
            .create_bucket(&BucketMeta {
                name: "idx".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
                region: "us-east-1".into(),
                versioning: false,
                cors_rules: None,
                encryption_config: None,
                public_read: false,
                public_list: false,
                bucket_policy: None,
                erasure_coding: None,
                lifecycle_rules: None,
                tenant_id: None,
                logging_target_bucket: None,
                logging_target_prefix: None,
                notification_config: None,
                object_lock_enabled: false,
                object_lock_config: None,
            })
            .await
            .unwrap();

        for key in ["a.txt", "b.txt", "prefix/c.txt"] {
            let body: ByteStream = Box::pin(Cursor::new(b"x".to_vec()));
            storage
                .put_object("idx", key, "text/plain", body, None, None, None)
                .await
                .unwrap();
        }

        let indexed = storage.list_objects("idx", "prefix/").await.unwrap();
        let walked = storage.list_objects_walk("idx", "prefix/").await.unwrap();
        assert_eq!(indexed.len(), walked.len());
        assert_eq!(
            indexed.iter().map(|m| &m.key).collect::<Vec<_>>(),
            walked.iter().map(|m| &m.key).collect::<Vec<_>>()
        );

        storage.delete_object("idx", "a.txt").await.unwrap();
        let after_delete = storage.list_objects("idx", "").await.unwrap();
        assert_eq!(after_delete.len(), 2);
    }
}
