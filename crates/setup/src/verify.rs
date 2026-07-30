//! Deciding whether a downloaded file may be run.
//!
//! Two checks, both required, and they are not the same kind of thing:
//!
//! * The **minisign signature** proves the file came from whoever holds the
//!   private key. The public key is compiled in, never taken from the feed.
//! * The **SHA-256** proves the file is not corrupt. `SHA256SUMS.txt` travels
//!   the same channel as the artefact, so anyone able to substitute one can
//!   substitute the other — it is an integrity check, not an authenticity one,
//!   and it is not the security boundary.
//!
//! Everything here is a pure function of bytes, so a tampered artefact, a wrong
//! signature and a missing one are all tested without a network.

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// The minisign public key, put here by `build.rs` from `tauri.conf.json` so
/// this program and the in-app updater trust the same key by construction.
pub const PUBKEY: &str = env!("PANEL_MINISIGN_PUBKEY");

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("the downloaded file is not signed by Panel Platform and will not be run")]
    BadSignature,
    #[error("the signature accompanying the download is not a valid minisign signature")]
    MalformedSignature,
    #[error("this installer was built with an unusable signing key")]
    MalformedKey,
    #[error("{0} is not listed in SHA256SUMS.txt")]
    NotListed(String),
    #[error("the downloaded file is damaged: its checksum does not match the release")]
    ChecksumMismatch,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // `write!` to a String cannot fail, and this avoids a fallible call in
        // a crate where `unwrap` is denied.
        hex.push(nibble(byte >> 4));
        hex.push(nibble(byte & 0x0f));
    }
    hex
}

fn nibble(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'a' + value - 10) as char,
    }
}

/// Finds the checksum for one file in `sha256sum` output.
///
/// Both the text (`hash  name`) and binary (`hash *name`) markers appear in the
/// wild, and a name may carry a directory prefix, so the comparison is on the
/// final path component.
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let line = line.trim();
        let (hash, listed) = line.split_once(char::is_whitespace)?;
        let listed = listed.trim_start().trim_start_matches('*');
        let listed = listed.rsplit(['/', '\\']).next().unwrap_or(listed);

        (listed == name && !hash.is_empty()).then(|| hash.to_ascii_lowercase())
    })
}

/// The whole gate. Signature first: a file that is not authentic is refused
/// before its checksum is even interesting.
pub fn verify(
    bytes: &[u8],
    name: &str,
    signature: &str,
    sums: &str,
    pubkey: &str,
) -> Result<(), VerifyError> {
    verify_signature(bytes, signature, pubkey)?;
    verify_checksum(bytes, name, sums)
}

pub fn verify_signature(bytes: &[u8], signature: &str, pubkey: &str) -> Result<(), VerifyError> {
    let key = decode_key(pubkey)?;
    let signature = decode_signature(signature)?;

    key.verify(bytes, &signature, false)
        .map_err(|_| VerifyError::BadSignature)
}

/// Accepts a minisign signature in either encoding.
///
/// `tauri-plugin-updater` writes its `.sig` files base64'd whole, the way it
/// writes the public key in `tauri.conf.json`; the minisign tool writes plain
/// text. Both are read, because which one is on the release is a detail of
/// whoever signed it and not something a user can be asked to care about.
fn decode_signature(signature: &str) -> Result<minisign_verify::Signature, VerifyError> {
    if let Ok(parsed) = minisign_verify::Signature::decode(signature) {
        return Ok(parsed);
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|_| VerifyError::MalformedSignature)?;
    let text = String::from_utf8(decoded).map_err(|_| VerifyError::MalformedSignature)?;

    minisign_verify::Signature::decode(&text).map_err(|_| VerifyError::MalformedSignature)
}

pub fn verify_checksum(bytes: &[u8], name: &str, sums: &str) -> Result<(), VerifyError> {
    let expected =
        checksum_for(sums, name).ok_or_else(|| VerifyError::NotListed(name.to_owned()))?;

    if sha256_hex(bytes) == expected {
        Ok(())
    } else {
        Err(VerifyError::ChecksumMismatch)
    }
}

/// `tauri.conf.json` stores the minisign public key *file* base64'd whole, so
/// it decodes to a comment line followed by the key line.
fn decode_key(pubkey: &str) -> Result<minisign_verify::PublicKey, VerifyError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(pubkey.trim())
        .map_err(|_| VerifyError::MalformedKey)?;
    let text = String::from_utf8(decoded).map_err(|_| VerifyError::MalformedKey)?;

    let line = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
        .ok_or(VerifyError::MalformedKey)?;

    minisign_verify::PublicKey::from_base64(line).map_err(|_| VerifyError::MalformedKey)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SUMS: &str = "\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  empty.bin
9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08  Panel.deb
0000000000000000000000000000000000000000000000000000000000000000 *Other.AppImage
";

    #[test]
    fn sha256_matches_the_known_digest_of_empty_input() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_matches_the_known_digest_of_test() {
        assert_eq!(
            sha256_hex(b"test"),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }

    #[test]
    fn a_listed_checksum_is_found_in_either_marker_style() {
        assert!(checksum_for(SUMS, "Panel.deb").is_some());
        assert!(checksum_for(SUMS, "Other.AppImage").is_some());
    }

    #[test]
    fn an_unlisted_file_has_no_checksum() {
        assert_eq!(checksum_for(SUMS, "Absent.msi"), None);
    }

    #[test]
    fn a_matching_file_passes_its_checksum() {
        assert_eq!(verify_checksum(b"test", "Panel.deb", SUMS), Ok(()));
    }

    /// One byte different must fail, not round to "close enough".
    #[test]
    fn a_tampered_byte_fails_the_checksum() {
        assert_eq!(
            verify_checksum(b"tesT", "Panel.deb", SUMS),
            Err(VerifyError::ChecksumMismatch)
        );
    }

    /// A file nobody vouched for is refused rather than passed for lack of
    /// evidence against it.
    #[test]
    fn a_file_missing_from_the_sums_is_refused() {
        assert_eq!(
            verify_checksum(b"test", "Absent.msi", SUMS),
            Err(VerifyError::NotListed("Absent.msi".to_owned()))
        );
    }

    #[test]
    fn the_compiled_in_key_is_a_usable_minisign_key() {
        assert!(decode_key(PUBKEY).is_ok(), "PUBKEY did not decode");
    }

    /// The real `.sig` from the 0.1.0 release, exactly as GitHub serves it.
    ///
    /// It is base64 around the minisign text, which is what
    /// `tauri-plugin-updater` writes. The first end-to-end run of this program
    /// failed here — every byte downloaded and verified against nothing —
    /// because only the plain form was read.
    const TAURI_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSREtlVysrVUw1SzBTVTM0NllQWHZVcVM4NGt5bmwyaUtvQWlRN2ZYMW9xeGFTNG9XLzJwUU1Nb0dmNDViOEQ5MXUvRG1tdHdMcmtnajc1L1hIN3U1T3BVU3BpVXNqSmdzPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg1NDUxNDYyCWZpbGU6UGFuZWwgUGxhdGZvcm1fMC4xLjBfeDY0LXNldHVwLmV4ZQpRQUk2ZldGUUwwMFplS295dVRTZkVJcS9hMWQzR2k3dzRYUXlSdk5nYlJrbWYraExFd0N1YVJZd2xtMjhMQWtZWWFrRTZNMXpMQkovZzJFYkljRVBEZz09Cg==";

    #[test]
    fn a_tauri_written_signature_is_understood() {
        assert!(
            decode_signature(TAURI_SIG).is_ok(),
            "the encoding the project actually ships was not read"
        );
    }

    /// Both encodings must be accepted, since which one appears is a detail of
    /// whoever signed the release.
    #[test]
    fn both_signature_encodings_are_accepted() {
        let plain = base64::engine::general_purpose::STANDARD
            .decode(TAURI_SIG)
            .map(String::from_utf8)
            .unwrap()
            .unwrap();

        assert!(decode_signature(&plain).is_ok(), "plain minisign text");
        assert!(decode_signature(TAURI_SIG).is_ok(), "base64-wrapped");
    }

    /// Reading an extra encoding must not weaken the check: this signature is
    /// well formed and genuinely from the project, and still must not verify
    /// bytes it was not made over.
    #[test]
    fn a_real_signature_does_not_verify_the_wrong_bytes() {
        assert_eq!(
            verify_signature(b"not the installer", TAURI_SIG, PUBKEY),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn a_garbage_signature_is_rejected_as_malformed() {
        assert_eq!(
            verify_signature(b"anything", "not a signature", PUBKEY),
            Err(VerifyError::MalformedSignature)
        );
    }

    /// A well-formed signature over different bytes must not verify. This is
    /// the check the whole program rests on.
    #[test]
    fn a_signature_over_other_bytes_does_not_verify() {
        // Structurally valid minisign signature, wrong for this input.
        let signature = "untrusted comment: signature\n\
            RWRDKeW++UL5K7dOZOaXlIYc4nQ8p6ZAqRRMdxfvJ0hQpZ7z9Yd1mHM9m4wOAK0kMk8s3W5MCT0P4vRk5nCkGQBnMFvUZ0wJ8gY=\n\
            trusted comment: t\n\
            RWRDKeW++UL5K7dOZOaXlIYc4nQ8p6ZAqRRMdxfvJ0hQpZ7z9Yd1mHM9m4wOAK0kMk8s3W5MCT0P4vRk5nCkGQBnMFvUZ0wJ8gY=\n";

        assert!(
            verify_signature(b"anything", signature, PUBKEY).is_err(),
            "a signature over other bytes verified"
        );
    }

    #[test]
    fn an_unusable_key_is_reported_as_such() {
        assert_eq!(
            verify_signature(b"x", "sig", "not base64 at all!!"),
            Err(VerifyError::MalformedKey)
        );
    }

    /// The message a user sees must say the file will not be run, because that
    /// is the consequence.
    #[test]
    fn the_bad_signature_message_says_what_happens_next() {
        let message = VerifyError::BadSignature.to_string();
        assert!(message.contains("not signed"));
        assert!(message.contains("will not be run"));
    }
}
