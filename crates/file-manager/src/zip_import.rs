//! Importing a ZIP archive.
//!
//! An archive is the most hostile input the product accepts: it arrives as one
//! opaque blob and expands into arbitrary filenames and arbitrary bytes. Every
//! limit below is checked *while* extracting, against running totals, so a
//! bomb is caught as it inflates rather than after it has filled the disk.
//!
//! Extraction targets a UUID-named staging directory on the same filesystem as
//! the projects directory, and is renamed into place only after full success.
//! Nothing is ever written directly into a live project.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::safe_path::{PathError, SafePath};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchiveError {
    #[error("the archive is not a valid ZIP file")]
    NotAZip,
    #[error("the archive is larger than the {limit} byte limit")]
    ArchiveTooLarge { limit: u64 },
    #[error("the archive contains more than {limit} entries")]
    TooManyEntries { limit: u32 },
    #[error("the archive expands to more than {limit} bytes")]
    ExpandsTooLarge { limit: u64 },
    #[error("`{name}` is larger than the {limit} byte per-file limit")]
    FileTooLarge { name: String, limit: u64 },
    #[error("the archive expands {ratio}x, which exceeds the {limit}x limit")]
    CompressionRatio { ratio: u64, limit: u64 },
    /// The entry name itself was hostile. The specific rule is deliberately not
    /// reported to a remote caller — see `docs/security.md` §4.
    #[error("the archive contains an unsafe entry")]
    UnsafeEntry { name: String, reason: PathError },
    #[error("the archive contains a {kind} entry, which is not permitted")]
    ForbiddenEntryKind { name: String, kind: &'static str },
    #[error("extraction failed: {0}")]
    Io(String),
}

/// Limits, all configurable but with safe defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_archive_bytes: u64,
    pub max_entries: u32,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    /// Overall expansion ratio. A legitimate source tree compresses perhaps
    /// 5–10x; 100x is generous and still catches a bomb early.
    pub max_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 2 * 1024 * 1024 * 1024,
            max_entries: 50_000,
            max_total_bytes: 10 * 1024 * 1024 * 1024,
            max_file_bytes: 1024 * 1024 * 1024,
            max_ratio: 100,
        }
    }
}

/// What an import produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub files: u32,
    pub directories: u32,
    pub total_bytes: u64,
    /// Entries skipped because they were metadata rather than content.
    pub skipped: Vec<String>,
}

/// One entry, as read from the archive. Kept separate from the ZIP crate's
/// types so the validation logic is testable without building real archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub unix_mode: Option<u32>,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

impl ArchiveEntry {
    /// Anything that is not a regular file or a directory.
    ///
    /// Their presence is evidence of intent rather than accident, so an archive
    /// containing one fails the whole import rather than being silently
    /// filtered — a user should know their archive was crafted.
    pub fn forbidden_kind(&self) -> Option<&'static str> {
        if self.is_symlink {
            return Some("symbolic link");
        }
        let mode = self.unix_mode?;
        // Upper four bits of the mode are the file type.
        match mode & 0o170_000 {
            0o140_000 => Some("socket"),
            0o120_000 => Some("symbolic link"),
            0o060_000 => Some("block device"),
            0o020_000 => Some("character device"),
            0o010_000 => Some("FIFO"),
            _ => {
                // setuid or setgid on an extracted file is never wanted.
                if mode & 0o6000 != 0 {
                    Some("setuid or setgid")
                } else {
                    None
                }
            }
        }
    }
}

/// Metadata directories that are noise rather than content.
fn is_metadata(name: &str) -> bool {
    let normalised = name.replace('\\', "/");
    normalised.starts_with("__MACOSX/")
        || normalised.split('/').any(|part| part == ".DS_Store")
        || normalised.ends_with("/Thumbs.db")
}

/// Validate one entry against the root and the running totals.
///
/// Separated from extraction so the entire rule set is unit-testable without
/// constructing archives on disk.
pub fn validate_entry(
    root: &Path,
    entry: &ArchiveEntry,
    limits: &ArchiveLimits,
    running_total: u64,
) -> Result<Option<SafePath>, ArchiveError> {
    if is_metadata(&entry.name) {
        return Ok(None);
    }

    if let Some(kind) = entry.forbidden_kind() {
        return Err(ArchiveError::ForbiddenEntryKind {
            name: entry.name.clone(),
            kind,
        });
    }

    // Entry names go through exactly the same validation as a request path.
    // Zip Slip is not a separate defence; it is this one, reused.
    let trimmed = entry.name.trim_end_matches('/').trim_end_matches('\\');
    let safe = SafePath::new(root, trimmed).map_err(|reason| ArchiveError::UnsafeEntry {
        name: entry.name.clone(),
        reason,
    })?;

    if entry.uncompressed_size > limits.max_file_bytes {
        return Err(ArchiveError::FileTooLarge {
            name: entry.name.clone(),
            limit: limits.max_file_bytes,
        });
    }

    if running_total.saturating_add(entry.uncompressed_size) > limits.max_total_bytes {
        return Err(ArchiveError::ExpandsTooLarge {
            limit: limits.max_total_bytes,
        });
    }

    Ok(Some(safe))
}

/// Check the overall expansion ratio.
///
/// Applied to the totals rather than per entry: a single highly compressible
/// file is normal, while an archive whose *whole* content expands 100x is not.
pub fn check_ratio(
    compressed_total: u64,
    uncompressed_total: u64,
    limits: &ArchiveLimits,
) -> Result<(), ArchiveError> {
    // Below this, ratios are meaningless — a 4-byte archive expanding to 4 KB
    // is not an attack.
    const MINIMUM_MEANINGFUL: u64 = 64 * 1024;
    if uncompressed_total < MINIMUM_MEANINGFUL || compressed_total == 0 {
        return Ok(());
    }
    let ratio = uncompressed_total / compressed_total.max(1);
    if ratio > limits.max_ratio {
        return Err(ArchiveError::CompressionRatio {
            ratio,
            limit: limits.max_ratio,
        });
    }
    Ok(())
}

/// A staging directory that removes itself unless explicitly kept.
///
/// Partial extraction must never survive a failure, and relying on every error
/// path to remember to clean up is how partial extractions survive failures.
#[derive(Debug)]
pub struct Staging {
    path: PathBuf,
    keep: bool,
}

impl Staging {
    /// Create a UUID-named staging directory. The name never comes from user
    /// input, so a hostile archive cannot influence where it lands.
    pub fn new(temp_root: &Path, id: &str) -> Result<Self, ArchiveError> {
        let path = temp_root.join(format!("import-{id}"));
        std::fs::create_dir_all(&path).map_err(|error| ArchiveError::Io(error.to_string()))?;
        Ok(Self { path, keep: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Move the staged tree into its final location and stop cleaning up.
    ///
    /// A rename is atomic only within a filesystem, which is why the temp
    /// directory shares one with the projects directory.
    pub fn promote(mut self, destination: &Path) -> Result<(), ArchiveError> {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ArchiveError::Io(e.to_string()))?;
        }
        std::fs::rename(&self.path, destination)
            .map_err(|error| ArchiveError::Io(error.to_string()))?;
        self.keep = true;
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Read an entire entry with a hard byte ceiling.
///
/// `take` is what makes a declared size untrustworthy but harmless: a ZIP
/// header claiming one byte while the stream delivers a gigabyte is stopped at
/// the limit rather than believed.
pub fn read_bounded<R: Read>(reader: &mut R, limit: u64) -> Result<Vec<u8>, ArchiveError> {
    let mut buffer = Vec::new();
    let mut limited = reader.take(limit.saturating_add(1));
    limited
        .read_to_end(&mut buffer)
        .map_err(|error| ArchiveError::Io(error.to_string()))?;

    if buffer.len() as u64 > limit {
        return Err(ArchiveError::FileTooLarge {
            name: "<entry>".to_string(),
            limit,
        });
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn entry(name: &str) -> ArchiveEntry {
        ArchiveEntry {
            name: name.to_string(),
            is_directory: false,
            is_symlink: false,
            unix_mode: None,
            compressed_size: 100,
            uncompressed_size: 1000,
        }
    }

    #[test]
    fn an_ordinary_entry_is_accepted() {
        let dir = root();
        let safe = validate_entry(
            dir.path(),
            &entry("src/index.js"),
            &ArchiveLimits::default(),
            0,
        )
        .expect("validate")
        .expect("not skipped");
        assert_eq!(safe.relative(), "src/index.js");
    }

    #[test]
    fn zip_slip_entries_are_refused() {
        let dir = root();
        let limits = ArchiveLimits::default();
        for name in [
            "../../etc/passwd",
            "../escape.txt",
            "src/../../../evil",
            "/etc/passwd",
            "C:\\Windows\\System32\\evil.dll",
            "\\\\server\\share\\evil",
            "..\\..\\Windows",
        ] {
            let result = validate_entry(dir.path(), &entry(name), &limits, 0);
            assert!(
                matches!(result, Err(ArchiveError::UnsafeEntry { .. })),
                "{name} should be refused, got {result:?}"
            );
        }
    }

    #[test]
    fn reserved_names_and_control_characters_are_refused() {
        let dir = root();
        let limits = ArchiveLimits::default();
        for name in ["CON.txt", "evil\0.js", "trailing.", "file.txt:stream"] {
            assert!(
                matches!(
                    validate_entry(dir.path(), &entry(name), &limits, 0),
                    Err(ArchiveError::UnsafeEntry { .. })
                ),
                "{name:?} should be refused"
            );
        }
    }

    #[test]
    fn symlink_entries_fail_the_whole_import() {
        // Skipping silently would let a crafted archive look like it imported
        // cleanly. The user should know.
        let dir = root();
        let mut link = entry("link");
        link.is_symlink = true;
        assert!(matches!(
            validate_entry(dir.path(), &link, &ArchiveLimits::default(), 0),
            Err(ArchiveError::ForbiddenEntryKind {
                kind: "symbolic link",
                ..
            })
        ));
    }

    #[test]
    fn device_fifo_and_socket_entries_are_refused() {
        let dir = root();
        let limits = ArchiveLimits::default();
        for (mode, expected) in [
            (0o120_777, "symbolic link"),
            (0o140_777, "socket"),
            (0o060_644, "block device"),
            (0o020_644, "character device"),
            (0o010_644, "FIFO"),
        ] {
            let mut special = entry("thing");
            special.unix_mode = Some(mode);
            let result = validate_entry(dir.path(), &special, &limits, 0);
            match result {
                Err(ArchiveError::ForbiddenEntryKind { kind, .. }) => assert_eq!(kind, expected),
                other => panic!("mode {mode:o} should be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn setuid_entries_are_refused() {
        let dir = root();
        let mut suid = entry("payload");
        suid.unix_mode = Some(0o104_755);
        assert!(matches!(
            validate_entry(dir.path(), &suid, &ArchiveLimits::default(), 0),
            Err(ArchiveError::ForbiddenEntryKind {
                kind: "setuid or setgid",
                ..
            })
        ));
    }

    #[test]
    fn a_regular_file_mode_is_allowed() {
        let dir = root();
        let mut regular = entry("index.js");
        regular.unix_mode = Some(0o100_644);
        assert!(validate_entry(dir.path(), &regular, &ArchiveLimits::default(), 0).is_ok());
    }

    #[test]
    fn oversized_files_are_refused() {
        let dir = root();
        let limits = ArchiveLimits {
            max_file_bytes: 1000,
            ..ArchiveLimits::default()
        };
        let mut big = entry("big.bin");
        big.uncompressed_size = 1001;
        assert!(matches!(
            validate_entry(dir.path(), &big, &limits, 0),
            Err(ArchiveError::FileTooLarge { .. })
        ));
    }

    #[test]
    fn the_running_total_stops_a_bomb_partway_through() {
        // The important property: the limit applies as the archive inflates,
        // not after it has already been written.
        let dir = root();
        let limits = ArchiveLimits {
            max_total_bytes: 10_000,
            ..ArchiveLimits::default()
        };
        let mut chunk = entry("chunk.bin");
        chunk.uncompressed_size = 6000;

        assert!(validate_entry(dir.path(), &chunk, &limits, 0).is_ok());
        assert!(matches!(
            validate_entry(dir.path(), &chunk, &limits, 6000),
            Err(ArchiveError::ExpandsTooLarge { .. })
        ));
    }

    #[test]
    fn an_extreme_compression_ratio_is_refused() {
        let limits = ArchiveLimits::default();
        // 1 MB compressing to 1 KB is 1000x — a classic bomb signature.
        assert!(matches!(
            check_ratio(1024, 1024 * 1024, &limits),
            Err(ArchiveError::CompressionRatio { .. })
        ));
    }

    #[test]
    fn ordinary_compression_is_allowed() {
        let limits = ArchiveLimits::default();
        // A source tree at ~8x.
        assert!(check_ratio(1024 * 1024, 8 * 1024 * 1024, &limits).is_ok());
    }

    #[test]
    fn tiny_archives_are_not_judged_on_ratio() {
        // A 4-byte archive expanding to 4 KB is not an attack.
        let limits = ArchiveLimits::default();
        assert!(check_ratio(4, 4096, &limits).is_ok());
    }

    #[test]
    fn metadata_entries_are_skipped_rather_than_refused() {
        let dir = root();
        let limits = ArchiveLimits::default();
        for name in ["__MACOSX/._file", "src/.DS_Store", "folder/Thumbs.db"] {
            let result = validate_entry(dir.path(), &entry(name), &limits, 0).expect("validate");
            assert!(result.is_none(), "{name} should be skipped");
        }
    }

    #[test]
    fn staging_removes_itself_on_failure() {
        let dir = root();
        let path = {
            let staging = Staging::new(dir.path(), "abc123").expect("staging");
            std::fs::write(staging.path().join("partial.txt"), "half").expect("write");
            assert!(staging.path().exists());
            staging.path().to_path_buf()
        };
        // Dropped without promote: a partial extraction must not survive.
        assert!(!path.exists(), "staging should have been cleaned up");
    }

    #[test]
    fn promoted_staging_survives_and_moves() {
        let dir = root();
        let destination = dir.path().join("projects").join("prj_1");

        let staging = Staging::new(dir.path(), "abc123").expect("staging");
        std::fs::write(staging.path().join("index.js"), "// hi").expect("write");
        let staged_path = staging.path().to_path_buf();

        staging.promote(&destination).expect("promote");

        assert!(
            !staged_path.exists(),
            "the staging directory should be gone"
        );
        assert!(
            destination.join("index.js").is_file(),
            "content should have moved"
        );
    }

    #[test]
    fn a_declared_size_smaller_than_the_stream_does_not_help_an_attacker() {
        // A ZIP header can lie. The reader is bounded regardless.
        let data = vec![0u8; 5000];
        let mut cursor = std::io::Cursor::new(data);
        assert!(matches!(
            read_bounded(&mut cursor, 1000),
            Err(ArchiveError::FileTooLarge { .. })
        ));
    }

    #[test]
    fn reading_within_the_limit_succeeds() {
        let data = vec![7u8; 500];
        let mut cursor = std::io::Cursor::new(data);
        let read = read_bounded(&mut cursor, 1000).expect("read");
        assert_eq!(read.len(), 500);
    }

    #[test]
    fn a_deeply_nested_entry_is_refused() {
        let dir = root();
        let deep = (0..40).map(|_| "a").collect::<Vec<_>>().join("/");
        assert!(matches!(
            validate_entry(dir.path(), &entry(&deep), &ArchiveLimits::default(), 0),
            Err(ArchiveError::UnsafeEntry { .. })
        ));
    }
}
