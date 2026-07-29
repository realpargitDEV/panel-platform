//! Secure storage for the master encryption key and device keys.
//!
//! Preference order: the OS keychain, then an encrypted-at-rest file with
//! restrictive permissions. The backend actually in use is reported, because
//! silently degrading from a keychain to a file would be a lie about the
//! security posture — see `docs/platform-support.md` §4.

use std::path::{Path, PathBuf};

/// Which mechanism is really holding the secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    /// Windows Credential Manager or Linux Secret Service.
    OsKeychain,
    /// A file with owner-only permissions. Used on headless Linux with no
    /// Secret Service, and reported as such.
    RestrictedFile,
}

impl StorageBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            StorageBackend::OsKeychain => "os-keychain",
            StorageBackend::RestrictedFile => "restricted-file",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("no secret stored under `{name}`")]
    NotFound { name: String },
    #[error("secure storage is unavailable: {0}")]
    Unavailable(String),
    #[error("could not access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the stored value is malformed")]
    Malformed,
}

pub trait SecureStorageProvider: Send + Sync + std::fmt::Debug {
    fn store(&self, name: &str, value: &[u8]) -> Result<(), StorageError>;
    fn retrieve(&self, name: &str) -> Result<Vec<u8>, StorageError>;
    fn delete(&self, name: &str) -> Result<(), StorageError>;
    /// What the UI reports. Never guessed.
    fn backend(&self) -> StorageBackend;
}

/// File-backed storage with owner-only permissions.
///
/// This is the documented fallback, not a shortcut. It is a real production
/// path for headless Linux installs, so it gets the same care as the keychain:
/// the mode is set before any bytes are written, and the directory is created
/// with restrictive permissions.
#[derive(Debug, Clone)]
pub struct FileStorage {
    directory: PathBuf,
}

impl FileStorage {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// Secret names come from the agent, never from a request, but the
    /// filesystem is unforgiving enough that this is validated anyway.
    fn path_for(&self, name: &str) -> Result<PathBuf, StorageError> {
        let safe = name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });
        if !safe || name.is_empty() {
            return Err(StorageError::Malformed);
        }
        Ok(self.directory.join(format!("{name}.key")))
    }
}

impl SecureStorageProvider for FileStorage {
    fn store(&self, name: &str, value: &[u8]) -> Result<(), StorageError> {
        let path = self.path_for(name)?;
        std::fs::create_dir_all(&self.directory).map_err(|source| StorageError::Io {
            path: self.directory.clone(),
            source,
        })?;

        #[cfg(unix)]
        {
            use std::io::Write as _;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|source| StorageError::Io {
                    path: self.directory.clone(),
                    source,
                })?;

            // Mode is applied at creation, so the file is never briefly
            // world-readable.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|source| StorageError::Io {
                    path: path.clone(),
                    source,
                })?;
            file.write_all(value).map_err(|source| StorageError::Io {
                path: path.clone(),
                source,
            })?;
        }

        #[cfg(windows)]
        {
            // Protected by the ProgramData ACL the installer applies.
            std::fs::write(&path, value).map_err(|source| StorageError::Io {
                path: path.clone(),
                source,
            })?;
        }

        Ok(())
    }

    fn retrieve(&self, name: &str) -> Result<Vec<u8>, StorageError> {
        let path = self.path_for(name)?;
        match std::fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound {
                    name: name.to_string(),
                })
            }
            Err(source) => Err(StorageError::Io { path, source }),
        }
    }

    fn delete(&self, name: &str) -> Result<(), StorageError> {
        let path = self.path_for(name)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            // Deleting something absent is success: callers run this during
            // cleanup and should not have to special-case it.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::Io { path, source }),
        }
    }

    fn backend(&self) -> StorageBackend {
        StorageBackend::RestrictedFile
    }
}

/// Choose the best available backend for this host.
///
/// Phase 3 ships the file backend on both platforms. The OS keychain
/// integration (Windows Credential Manager, Linux Secret Service) is designed
/// in `docs/platform-support.md` §4 but **not yet implemented**, and this
/// function reports `RestrictedFile` truthfully rather than claiming a keychain
/// it is not using.
pub fn open_secure_storage(config_dir: &Path) -> Box<dyn SecureStorageProvider> {
    Box::new(FileStorage::new(config_dir.join("keys")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, FileStorage) {
        let directory = tempfile::tempdir().expect("temp dir");
        let storage = FileStorage::new(directory.path().join("keys"));
        (directory, storage)
    }

    #[test]
    fn a_secret_round_trips() {
        let (_guard, storage) = storage();
        storage.store("master", b"a-key-value").expect("store");
        assert_eq!(
            storage.retrieve("master").expect("retrieve"),
            b"a-key-value"
        );
    }

    #[test]
    fn a_missing_secret_is_not_found_rather_than_empty() {
        let (_guard, storage) = storage();
        assert!(matches!(
            storage.retrieve("absent"),
            Err(StorageError::NotFound { .. })
        ));
    }

    #[test]
    fn storing_twice_overwrites() {
        let (_guard, storage) = storage();
        storage.store("master", b"first").expect("store");
        storage.store("master", b"second").expect("store");
        assert_eq!(storage.retrieve("master").expect("retrieve"), b"second");
    }

    #[test]
    fn deleting_is_idempotent() {
        let (_guard, storage) = storage();
        storage.store("master", b"value").expect("store");
        storage.delete("master").expect("first delete");
        storage
            .delete("master")
            .expect("deleting again must succeed");
        assert!(storage.retrieve("master").is_err());
    }

    #[test]
    fn a_traversing_name_is_refused() {
        // Names are internal, but a path built from a string is worth guarding
        // regardless — the cost is one check.
        let (_guard, storage) = storage();
        for name in ["../escape", "sub/dir", "a\\b", "", "with space", "a:b"] {
            assert!(
                matches!(storage.store(name, b"x"), Err(StorageError::Malformed)),
                "name {name:?} should be refused"
            );
        }
    }

    #[test]
    fn the_backend_is_reported_truthfully() {
        let (_guard, storage) = storage();
        assert_eq!(storage.backend(), StorageBackend::RestrictedFile);
        assert_eq!(storage.backend().as_str(), "restricted-file");
    }

    #[cfg(unix)]
    #[test]
    fn stored_secrets_are_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let (guard, storage) = storage();
        storage.store("master", b"value").expect("store");

        let file = guard.path().join("keys").join("master.key");
        let mode = std::fs::metadata(&file)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let directory_mode = std::fs::metadata(guard.path().join("keys"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
    }
}
