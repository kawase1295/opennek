//! One host per data dir: a lock on a file in it, held while the host runs, and the URL of the host holding it

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// Locked while a host runs; the OS releases it if the process dies, so a crash leaves no stale lock
const LOCK_FILE: &str = "opennek.lock";
/// The running host's URL; a separate file, since Windows does not let others read a locked one
const URL_FILE: &str = "opennek.url";

#[derive(Debug)]
pub enum LockError {
    /// Another host holds the lock; the URL is None if it has not written it yet
    Held {
        url: Option<String>,
    },
    Io(io::Error),
}

/// The lock on a data dir; dropping it releases the lock
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    url_path: PathBuf,
}

impl InstanceLock {
    /// Locks `dir`, which must exist, without waiting
    pub fn acquire(dir: &Path) -> Result<InstanceLock, LockError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))
            .map_err(LockError::Io)?;
        let url_path = dir.join(URL_FILE);
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let url = std::fs::read_to_string(&url_path).ok();
                return Err(LockError::Held {
                    url: url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()),
                });
            }
            Err(TryLockError::Error(e)) => return Err(LockError::Io(e)),
        }
        // a host that died left its URL behind
        let _ = std::fs::remove_file(&url_path);
        Ok(InstanceLock { _file: file, url_path })
    }

    /// Tells a later start where this host is
    pub fn publish(&self, url: &str) -> io::Result<()> {
        std::fs::write(&self.url_path, url)
    }

    /// Removes the URL and releases the lock
    pub fn release(self) {
        let _ = std::fs::remove_file(&self.url_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_reports_the_published_url_and_is_free_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let first = InstanceLock::acquire(dir.path()).unwrap();
        assert!(matches!(
            InstanceLock::acquire(dir.path()),
            Err(LockError::Held { url: None })
        ));
        first.publish("http://127.0.0.1:8085/").unwrap();
        match InstanceLock::acquire(dir.path()) {
            Err(LockError::Held { url }) => assert_eq!(url.as_deref(), Some("http://127.0.0.1:8085/")),
            other => panic!("{other:?}"),
        }
        first.release();
        assert!(!dir.path().join(URL_FILE).exists());
        InstanceLock::acquire(dir.path()).unwrap();
    }

    #[test]
    fn a_url_left_by_a_dead_host_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(URL_FILE), "http://127.0.0.1:1/").unwrap();
        let _lock = InstanceLock::acquire(dir.path()).unwrap();
        assert!(!dir.path().join(URL_FILE).exists());
    }
}
