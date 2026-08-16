//! The master encryption key, and where it lives between runs.
//!
//! Until now nothing in this application held an [`EncryptionKey`] at runtime,
//! which is why `provisioning` stores source tokens only when handed a key and
//! reports `credential_stored: false` otherwise. A Discord bot token cannot be
//! handled that way: the schema has nowhere to put a plaintext token, and a
//! connection that forgot its token on every restart would be a connection the
//! user re-enters daily.
//!
//! So the key is generated once and kept by
//! [`project_host_platform::secure_storage`], which prefers the OS keychain and
//! falls back to an owner-only file. Which of the two is in use is reported
//! rather than assumed — a fallback that pretended to be a keychain would be a
//! lie about the security posture.
//!
//! # Losing the key
//!
//! Every ciphertext in the database is bound to this key. If it is deleted, the
//! rows that depend on it become undecryptable — the bot tokens stop working
//! and must be re-entered. That is the intended failure: a key that could be
//! reconstructed from the database would not be protecting anything.

use std::path::Path;

use project_host_platform::secure_storage::{
    open_secure_storage, SecureStorageProvider, StorageBackend, StorageError,
};
use project_host_security::encryption::EncryptionError;
use project_host_security::EncryptionKey;

/// The name the master key is filed under.
///
/// Stable: changing it strands every existing ciphertext behind a key nothing
/// looks for any more.
const MASTER_KEY_NAME: &str = "master-key";

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("secure storage could not be used: {0}")]
    Storage(#[from] StorageError),
    #[error("the stored master key is not a usable key: {0}")]
    Unusable(#[from] EncryptionError),
}

/// The key, and how it is being kept.
#[derive(Debug, Clone)]
pub struct MasterKey {
    pub key: EncryptionKey,
    pub backend: StorageBackend,
    /// True when this call created the key rather than finding one.
    pub created: bool,
}

/// Load the master key, creating one on first run.
///
/// Creating on first run rather than at install time keeps the key out of the
/// installer and off any machine image cloned from this one: two installations
/// that started from the same disk still end up with different keys the first
/// time each one runs.
pub fn load_or_create_master_key(config_dir: &Path) -> Result<MasterKey, KeyError> {
    let storage = open_secure_storage(config_dir);
    load_or_create_in(storage.as_ref())
}

/// The same, against a storage provider a test can supply.
pub fn load_or_create_in(storage: &dyn SecureStorageProvider) -> Result<MasterKey, KeyError> {
    let backend = storage.backend();

    match storage.retrieve(MASTER_KEY_NAME) {
        Ok(bytes) => Ok(MasterKey {
            key: EncryptionKey::from_bytes(bytes)?,
            backend,
            created: false,
        }),

        // The only error that means "generate one". Anything else — a
        // permission problem, a malformed value — is reported, because
        // generating a fresh key over an existing-but-unreadable one would
        // silently destroy access to every token already stored.
        Err(StorageError::NotFound { .. }) => {
            let key = EncryptionKey::generate();
            storage.store(MASTER_KEY_NAME, key.expose_bytes())?;
            Ok(MasterKey {
                key,
                backend,
                created: true,
            })
        }

        Err(other) => Err(KeyError::Storage(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Storage that can be told to fail, which a real keychain cannot.
    #[derive(Debug)]
    struct FakeStorage {
        held: Mutex<Option<Vec<u8>>>,
        fail_retrieve_with: Mutex<Option<String>>,
    }

    impl FakeStorage {
        fn empty() -> Self {
            Self {
                held: Mutex::new(None),
                fail_retrieve_with: Mutex::new(None),
            }
        }

        fn unreadable() -> Self {
            Self {
                held: Mutex::new(Some(vec![1u8; 32])),
                fail_retrieve_with: Mutex::new(Some("permission denied".to_string())),
            }
        }
    }

    impl SecureStorageProvider for FakeStorage {
        fn store(&self, _name: &str, value: &[u8]) -> Result<(), StorageError> {
            *self.held.lock().unwrap() = Some(value.to_vec());
            Ok(())
        }

        fn retrieve(&self, name: &str) -> Result<Vec<u8>, StorageError> {
            if let Some(message) = self.fail_retrieve_with.lock().unwrap().clone() {
                return Err(StorageError::Unavailable(message));
            }
            self.held
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| StorageError::NotFound {
                    name: name.to_string(),
                })
        }

        fn delete(&self, _name: &str) -> Result<(), StorageError> {
            *self.held.lock().unwrap() = None;
            Ok(())
        }

        fn backend(&self) -> StorageBackend {
            StorageBackend::RestrictedFile
        }
    }

    #[test]
    fn the_first_run_creates_a_key_and_keeps_it() {
        let storage = FakeStorage::empty();

        let first = load_or_create_in(&storage).expect("create");
        assert!(first.created);

        let second = load_or_create_in(&storage).expect("load");
        assert!(!second.created, "the second run should find the first key");
        assert_eq!(
            first.key.expose_bytes(),
            second.key.expose_bytes(),
            "a different key on restart would strand every stored token"
        );
    }

    #[test]
    fn storage_that_cannot_be_read_is_reported_not_overwritten() {
        // The dangerous case: generating a fresh key over an existing but
        // temporarily unreadable one destroys access to every stored token.
        let storage = FakeStorage::unreadable();

        let error = load_or_create_in(&storage).expect_err("should refuse");
        assert!(matches!(
            error,
            KeyError::Storage(StorageError::Unavailable(_))
        ));
        assert!(
            storage.held.lock().unwrap().is_some(),
            "the existing key must still be there"
        );
    }

    #[test]
    fn the_backend_in_use_is_reported_truthfully() {
        let storage = FakeStorage::empty();
        let loaded = load_or_create_in(&storage).expect("create");
        assert_eq!(loaded.backend, StorageBackend::RestrictedFile);
    }
}
