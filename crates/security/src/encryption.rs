//! Encryption of environment-variable secrets at rest.
//!
//! XChaCha20-Poly1305: authenticated, and its 192-bit nonce is large enough
//! that random generation per value has no realistic collision risk — which
//! removes the need for a nonce counter that would have to survive restarts.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroize;

use crate::secret::Secret;

pub const KEY_BYTES: usize = 32;
pub const NONCE_BYTES: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum EncryptionError {
    #[error("encryption key must be exactly {KEY_BYTES} bytes")]
    InvalidKeyLength,
    #[error("nonce must be exactly {NONCE_BYTES} bytes")]
    InvalidNonceLength,
    #[error("encryption failed")]
    Encrypt,
    /// Also returned when the ciphertext or associated data has been tampered
    /// with — the two are indistinguishable, and deliberately so.
    #[error("decryption failed: wrong key or tampered ciphertext")]
    Decrypt,
}

/// One encrypted value, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ciphertext {
    pub bytes: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// The master key. Held in the OS keychain, never on disk in the clear.
#[derive(Clone)]
pub struct EncryptionKey(Secret<Vec<u8>>);

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncryptionKey([redacted])")
    }
}

impl EncryptionKey {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, EncryptionError> {
        if bytes.len() != KEY_BYTES {
            let mut bytes = bytes;
            bytes.zeroize();
            return Err(EncryptionError::InvalidKeyLength);
        }
        Ok(Self(Secret::new(bytes)))
    }

    pub fn generate() -> Self {
        let mut bytes = vec![0u8; KEY_BYTES];
        rand::rng().fill_bytes(&mut bytes);
        Self(Secret::new(bytes))
    }

    fn cipher(&self) -> Result<XChaCha20Poly1305, EncryptionError> {
        XChaCha20Poly1305::new_from_slice(self.0.expose())
            .map_err(|_| EncryptionError::InvalidKeyLength)
    }

    pub fn expose_bytes(&self) -> &[u8] {
        self.0.expose()
    }
}

/// Encrypt a value.
///
/// `associated_data` is authenticated but not encrypted. Passing the project id
/// and variable key binds the ciphertext to its row: a value moved to a
/// different project or renamed to a different key will not decrypt, so a
/// database edit cannot silently repoint a secret.
pub fn encrypt(
    key: &EncryptionKey,
    plaintext: &Secret<String>,
    associated_data: &[u8],
) -> Result<Ciphertext, EncryptionError> {
    let mut nonce_bytes = [0u8; NONCE_BYTES];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);

    let bytes = key
        .cipher()?
        .encrypt(
            nonce,
            Payload {
                msg: plaintext.expose().as_bytes(),
                aad: associated_data,
            },
        )
        .map_err(|_| EncryptionError::Encrypt)?;

    Ok(Ciphertext {
        bytes,
        nonce: nonce_bytes.to_vec(),
    })
}

/// Decrypt a value. The result is wrapped so it cannot be logged on the way out.
pub fn decrypt(
    key: &EncryptionKey,
    ciphertext: &Ciphertext,
    associated_data: &[u8],
) -> Result<Secret<String>, EncryptionError> {
    if ciphertext.nonce.len() != NONCE_BYTES {
        return Err(EncryptionError::InvalidNonceLength);
    }
    let nonce = XNonce::from_slice(&ciphertext.nonce);

    let plaintext = key
        .cipher()?
        .decrypt(
            nonce,
            Payload {
                msg: &ciphertext.bytes,
                aad: associated_data,
            },
        )
        .map_err(|_| EncryptionError::Decrypt)?;

    String::from_utf8(plaintext)
        .map(Secret::new)
        .map_err(|_| EncryptionError::Decrypt)
}

/// Bind a ciphertext to the row that holds it.
pub fn associated_data(project_id: &str, key: &str) -> Vec<u8> {
    format!("{project_id}\u{1f}{key}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plaintext(value: &str) -> Secret<String> {
        Secret::new(value.to_string())
    }

    #[test]
    fn a_value_round_trips() {
        let key = EncryptionKey::generate();
        let aad = associated_data("prj_1", "DISCORD_TOKEN");
        let encrypted = encrypt(&key, &plaintext("a-real-token"), &aad).expect("encrypt");
        let decrypted = decrypt(&key, &encrypted, &aad).expect("decrypt");
        assert_eq!(decrypted.expose(), "a-real-token");
    }

    #[test]
    fn the_ciphertext_does_not_contain_the_plaintext() {
        let key = EncryptionKey::generate();
        let aad = associated_data("prj_1", "TOKEN");
        let encrypted = encrypt(&key, &plaintext("a-real-token"), &aad).expect("encrypt");
        let window = b"a-real-token";
        assert!(
            !encrypted
                .bytes
                .windows(window.len())
                .any(|slice| slice == window),
            "plaintext appears in the ciphertext"
        );
    }

    #[test]
    fn each_encryption_uses_a_fresh_nonce() {
        let key = EncryptionKey::generate();
        let aad = associated_data("prj_1", "TOKEN");
        let first = encrypt(&key, &plaintext("same"), &aad).expect("encrypt");
        let second = encrypt(&key, &plaintext("same"), &aad).expect("encrypt");
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(
            first.bytes, second.bytes,
            "identical plaintexts must not match"
        );
    }

    #[test]
    fn the_wrong_key_cannot_decrypt() {
        let aad = associated_data("prj_1", "TOKEN");
        let encrypted =
            encrypt(&EncryptionKey::generate(), &plaintext("secret"), &aad).expect("encrypt");
        let result = decrypt(&EncryptionKey::generate(), &encrypted, &aad);
        assert!(matches!(result, Err(EncryptionError::Decrypt)));
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let key = EncryptionKey::generate();
        let aad = associated_data("prj_1", "TOKEN");
        let mut encrypted = encrypt(&key, &plaintext("secret"), &aad).expect("encrypt");
        encrypted.bytes[0] ^= 0x01;
        assert!(matches!(
            decrypt(&key, &encrypted, &aad),
            Err(EncryptionError::Decrypt)
        ));
    }

    #[test]
    fn a_value_moved_to_another_project_will_not_decrypt() {
        // The associated data binds the ciphertext to its row. Editing the
        // database to point a secret at a different project breaks it rather
        // than silently succeeding.
        let key = EncryptionKey::generate();
        let encrypted = encrypt(
            &key,
            &plaintext("secret"),
            &associated_data("prj_1", "TOKEN"),
        )
        .expect("encrypt");

        let moved = decrypt(&key, &encrypted, &associated_data("prj_2", "TOKEN"));
        assert!(matches!(moved, Err(EncryptionError::Decrypt)));

        let renamed = decrypt(&key, &encrypted, &associated_data("prj_1", "OTHER"));
        assert!(matches!(renamed, Err(EncryptionError::Decrypt)));
    }

    #[test]
    fn keys_must_be_the_right_length() {
        assert!(matches!(
            EncryptionKey::from_bytes(vec![0u8; 16]),
            Err(EncryptionError::InvalidKeyLength)
        ));
        assert!(EncryptionKey::from_bytes(vec![0u8; KEY_BYTES]).is_ok());
    }

    #[test]
    fn a_malformed_nonce_is_rejected() {
        let key = EncryptionKey::generate();
        let bad = Ciphertext {
            bytes: vec![0u8; 32],
            nonce: vec![0u8; 8],
        };
        assert!(matches!(
            decrypt(&key, &bad, b""),
            Err(EncryptionError::InvalidNonceLength)
        ));
    }

    #[test]
    fn the_key_is_never_printed() {
        let key = EncryptionKey::generate();
        let printed = format!("{key:?}");
        assert!(printed.contains("[redacted]"), "{printed}");
        assert!(!printed.contains("00"), "key bytes leaked: {printed}");
    }

    #[test]
    fn empty_values_round_trip() {
        let key = EncryptionKey::generate();
        let aad = associated_data("prj_1", "EMPTY");
        let encrypted = encrypt(&key, &plaintext(""), &aad).expect("encrypt");
        assert_eq!(
            decrypt(&key, &encrypted, &aad).expect("decrypt").expose(),
            ""
        );
    }

    #[test]
    fn unicode_values_round_trip() {
        let key = EncryptionKey::generate();
        let aad = associated_data("prj_1", "UNICODE");
        let value = "トークン🔐";
        let encrypted = encrypt(&key, &plaintext(value), &aad).expect("encrypt");
        assert_eq!(
            decrypt(&key, &encrypted, &aad).expect("decrypt").expose(),
            value
        );
    }
}
