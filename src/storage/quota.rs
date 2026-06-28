use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

use super::StorageError;

/// Upload and disk reservation limits.
#[derive(Debug, Clone, Copy)]
pub struct QuotaLimits {
    /// Maximum bytes per object (0 = unlimited).
    pub max_object_bytes: u64,
    /// Minimum free bytes to keep on the data volume (0 = disabled).
    pub min_free_disk_bytes: u64,
}

impl QuotaLimits {
    pub fn from_config(max_object_bytes: u64, min_free_disk_bytes: u64) -> Self {
        Self {
            max_object_bytes,
            min_free_disk_bytes,
        }
    }

    pub fn check_declared_size(&self, declared: Option<u64>) -> Result<(), StorageError> {
        if let Some(n) = declared {
            self.check_object_size(n)?;
        }
        Ok(())
    }

    pub fn check_object_size(&self, object_bytes: u64) -> Result<(), StorageError> {
        if self.max_object_bytes > 0 && object_bytes > self.max_object_bytes {
            return Err(StorageError::ObjectTooLarge {
                max: self.max_object_bytes,
            });
        }
        Ok(())
    }

    pub fn check_disk_reserve(&self, data_root: &Path) -> Result<(), StorageError> {
        if self.min_free_disk_bytes == 0 {
            return Ok(());
        }
        let available = available_disk_bytes(data_root).ok_or_else(|| {
            StorageError::InsufficientStorage("unable to determine free disk space".into())
        })?;
        if available < self.min_free_disk_bytes {
            return Err(StorageError::InsufficientStorage(format!(
                "free disk space {} bytes is below reserve {} bytes",
                available, self.min_free_disk_bytes
            )));
        }
        Ok(())
    }

    pub fn check_write_progress(&self, data_root: &Path, written: u64) -> Result<(), StorageError> {
        self.check_object_size(written)?;
        if self.min_free_disk_bytes > 0 {
            let available = available_disk_bytes(data_root).ok_or_else(|| {
                StorageError::InsufficientStorage("unable to determine free disk space".into())
            })?;
            if available < self.min_free_disk_bytes {
                return Err(StorageError::InsufficientStorage(format!(
                    "free disk space {} bytes is below reserve {} bytes",
                    available, self.min_free_disk_bytes
                )));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn statvfs_bytes(path: &Path) -> Option<libc::statvfs> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    Some(unsafe { stat.assume_init() })
}

/// Best-effort free bytes on the filesystem containing `path`.
#[cfg(unix)]
pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    let stat = statvfs_bytes(path)?;
    Some(stat.f_bavail * stat.f_frsize)
}

#[cfg(not(unix))]
pub fn available_disk_bytes(_path: &Path) -> Option<u64> {
    None
}

/// Best-effort `(total_bytes, available_bytes)` for the filesystem containing `path`.
#[cfg(unix)]
pub fn disk_space_bytes(path: &Path) -> Option<(u64, u64)> {
    let stat = statvfs_bytes(path)?;
    let frsize = stat.f_frsize;
    let total = stat.f_blocks * frsize;
    let available = stat.f_bavail * frsize;
    Some((total, available))
}

#[cfg(not(unix))]
pub fn disk_space_bytes(_path: &Path) -> Option<(u64, u64)> {
    None
}

pub struct QuotaReader<R> {
    inner: R,
    written: u64,
    limits: QuotaLimits,
    data_root: std::path::PathBuf,
}

impl<R> QuotaReader<R> {
    pub fn new(inner: R, limits: QuotaLimits, data_root: std::path::PathBuf) -> Self {
        Self {
            inner,
            written: 0,
            limits,
            data_root,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for QuotaReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            let read = (buf.filled().len() - before) as u64;
            if read > 0 {
                self.written += read;
                if let Err(e) = self
                    .limits
                    .check_write_progress(&self.data_root, self.written)
                {
                    return Poll::Ready(Err(quota_io_error(e)));
                }
            }
        }
        poll
    }
}

pub fn quota_io_error(err: StorageError) -> std::io::Error {
    match err {
        StorageError::ObjectTooLarge { max } => {
            std::io::Error::other(format!("maxio:quota:object-too-large:{max}"))
        }
        StorageError::InsufficientStorage(msg) => {
            std::io::Error::other(format!("maxio:quota:insufficient-storage:{msg}"))
        }
        other => std::io::Error::other(other.to_string()),
    }
}

pub fn map_read_quota_error(err: std::io::Error) -> StorageError {
    let msg = err.to_string();
    if let Some(rest) = msg.strip_prefix("maxio:quota:object-too-large:") {
        if let Ok(max) = rest.parse::<u64>() {
            return StorageError::ObjectTooLarge { max };
        }
    }
    if let Some(rest) = msg.strip_prefix("maxio:quota:insufficient-storage:") {
        return StorageError::InsufficientStorage(rest.to_string());
    }
    StorageError::Io(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_declared_object() {
        let limits = QuotaLimits {
            max_object_bytes: 100,
            min_free_disk_bytes: 0,
        };
        assert!(limits.check_declared_size(Some(101)).is_err());
        assert!(limits.check_declared_size(Some(100)).is_ok());
        assert!(limits.check_declared_size(None).is_ok());
    }
}
