//! Optional SQLite index for fast `ListObjects` on large buckets (P3-03).
//!
//! The filesystem remains the source of truth; the index is a cache that can be
//! rebuilt from a full bucket walk via [`MetadataIndex::rebuild_bucket`].

use crate::{ObjectMeta, StorageError};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SCHEMA_VERSION: i32 = 1;

pub struct MetadataIndex {
    conn: Mutex<Connection>,
}

impl MetadataIndex {
    pub fn open(data_root: &Path) -> Result<Self, StorageError> {
        let db_path = data_root.join(".maxio-metadata.db");
        let conn = Connection::open(&db_path).map_err(sqlite_err)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS object_index (
                 bucket TEXT NOT NULL,
                 object_key TEXT NOT NULL,
                 meta_json TEXT NOT NULL,
                 PRIMARY KEY (bucket, object_key)
             );
             CREATE INDEX IF NOT EXISTS idx_object_bucket_key
                 ON object_index(bucket, object_key);",
        )
        .map_err(sqlite_err)?;
        let index = Self {
            conn: Mutex::new(conn),
        };
        index.ensure_schema_version()?;
        Ok(index)
    }

    fn ensure_schema_version(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_poison)?;
        let version: i32 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if version == 0 {
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )
            .map_err(sqlite_err)?;
        } else if version != SCHEMA_VERSION {
            return Err(StorageError::InvalidKey(format!(
                "metadata index schema version mismatch: db={version}, expected={SCHEMA_VERSION}"
            )));
        }
        Ok(())
    }

    pub fn upsert(&self, bucket: &str, meta: &ObjectMeta) -> Result<(), StorageError> {
        if meta.is_delete_marker {
            return self.remove(bucket, &meta.key);
        }
        let json = serde_json::to_string(meta)?;
        let conn = self.conn.lock().map_err(lock_poison)?;
        conn.execute(
            "INSERT INTO object_index (bucket, object_key, meta_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(bucket, object_key) DO UPDATE SET meta_json = excluded.meta_json",
            params![bucket, meta.key, json],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub fn remove(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_poison)?;
        conn.execute(
            "DELETE FROM object_index WHERE bucket = ?1 AND object_key = ?2",
            params![bucket, key],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub fn remove_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_poison)?;
        conn.execute(
            "DELETE FROM object_index WHERE bucket = ?1",
            params![bucket],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub fn list(&self, bucket: &str, prefix: &str) -> Result<Vec<ObjectMeta>, StorageError> {
        let conn = self.conn.lock().map_err(lock_poison)?;
        let mut out = Vec::new();
        if prefix.is_empty() {
            let mut stmt = conn
                .prepare("SELECT meta_json FROM object_index WHERE bucket = ?1 ORDER BY object_key")
                .map_err(sqlite_err)?;
            let rows = stmt
                .query_map(params![bucket], |row| row.get::<_, String>(0))
                .map_err(sqlite_err)?;
            for row in rows {
                let json = row.map_err(sqlite_err)?;
                out.push(serde_json::from_str(&json)?);
            }
        } else {
            let like = escape_like_prefix(prefix);
            let mut stmt = conn
                .prepare(
                    "SELECT meta_json FROM object_index
                     WHERE bucket = ?1 AND object_key LIKE ?2 ESCAPE '\\'
                     ORDER BY object_key",
                )
                .map_err(sqlite_err)?;
            let rows = stmt
                .query_map(params![bucket, like], |row| row.get::<_, String>(0))
                .map_err(sqlite_err)?;
            for row in rows {
                let json = row.map_err(sqlite_err)?;
                out.push(serde_json::from_str(&json)?);
            }
        }
        Ok(out)
    }

    pub fn count_bucket(&self, bucket: &str) -> Result<u64, StorageError> {
        let conn = self.conn.lock().map_err(lock_poison)?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM object_index WHERE bucket = ?1",
                params![bucket],
                |row| row.get(0),
            )
            .map_err(sqlite_err)?;
        Ok(count as u64)
    }

    pub fn rebuild_bucket(&self, bucket: &str, objects: &[ObjectMeta]) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_poison)?;
        let tx = conn.unchecked_transaction().map_err(sqlite_err)?;
        tx.execute(
            "DELETE FROM object_index WHERE bucket = ?1",
            params![bucket],
        )
        .map_err(sqlite_err)?;
        for meta in objects {
            if meta.is_delete_marker {
                continue;
            }
            let json = serde_json::to_string(meta)?;
            tx.execute(
                "INSERT INTO object_index (bucket, object_key, meta_json) VALUES (?1, ?2, ?3)",
                params![bucket, meta.key, json],
            )
            .map_err(sqlite_err)?;
        }
        tx.commit().map_err(sqlite_err)?;
        Ok(())
    }

    pub fn db_path(data_root: &Path) -> PathBuf {
        data_root.join(".maxio-metadata.db")
    }
}

fn sqlite_err(e: rusqlite::Error) -> StorageError {
    StorageError::InvalidKey(format!("metadata index: {e}"))
}

fn escape_like_prefix(prefix: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + 8);
    for ch in prefix.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            other => out.push(other),
        }
    }
    out.push('%');
    out
}

fn lock_poison<T>(_err: std::sync::PoisonError<T>) -> StorageError {
    StorageError::InvalidKey("metadata index lock poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_meta(key: &str) -> ObjectMeta {
        ObjectMeta {
            key: key.to_string(),
            size: 42,
            etag: "\"abc\"".into(),
            content_type: "text/plain".into(),
            last_modified: "2026-01-01T00:00:00.000Z".into(),
            version_id: None,
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
            encryption: None,
        }
    }

    #[test]
    fn upsert_list_remove_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let index = MetadataIndex::open(tmp.path()).unwrap();
        let meta = sample_meta("photos/a.jpg");
        index.upsert("b1", &meta).unwrap();
        let listed = index.list("b1", "photos/").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, "photos/a.jpg");
        index.remove("b1", "photos/a.jpg").unwrap();
        assert!(index.list("b1", "").unwrap().is_empty());
    }

    #[test]
    fn rebuild_bucket_replaces_rows() {
        let tmp = TempDir::new().unwrap();
        let index = MetadataIndex::open(tmp.path()).unwrap();
        index.upsert("b1", &sample_meta("old")).unwrap();
        index
            .rebuild_bucket("b1", &[sample_meta("new1"), sample_meta("new2")])
            .unwrap();
        let keys: Vec<_> = index
            .list("b1", "")
            .unwrap()
            .into_iter()
            .map(|m| m.key)
            .collect();
        assert_eq!(keys, vec!["new1".to_string(), "new2".to_string()]);
    }
}
