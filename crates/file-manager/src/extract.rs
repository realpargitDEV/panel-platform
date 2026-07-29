//! Driving a real ZIP reader through the validation in [`crate::zip_import`].
//!
//! The rules live next door and are unit-tested without touching a disk. This
//! module is the part that cannot be: it opens an archive, walks it, and applies
//! those rules entry by entry while writing to a staging directory.
//!
//! The ordering is the whole point. For each entry: validate the *name* first,
//! then the declared size against the running total, then read with a hard
//! ceiling that does not trust the declared size at all, then write. An archive
//! that lies about any of the three is stopped at the first place it lies.

use std::io::{Read, Seek};
use std::path::Path;

use crate::zip_import::{
    check_ratio, validate_entry, ArchiveEntry, ArchiveError, ArchiveLimits, ImportReport, Staging,
};

/// Extract an archive into an empty directory.
///
/// `destination_root` must already exist and should be a [`Staging`] directory —
/// nothing here writes into a live project, because a failure partway through
/// would leave one half-overwritten.
pub fn extract_into<R: Read + Seek>(
    reader: R,
    destination_root: &Path,
    limits: &ArchiveLimits,
) -> Result<ImportReport, ArchiveError> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|error| match error {
        zip::result::ZipError::InvalidArchive(_) | zip::result::ZipError::UnsupportedArchive(_) => {
            ArchiveError::NotAZip
        }
        other => ArchiveError::Io(other.to_string()),
    })?;

    let count = u32::try_from(archive.len()).unwrap_or(u32::MAX);
    if count > limits.max_entries {
        return Err(ArchiveError::TooManyEntries {
            limit: limits.max_entries,
        });
    }

    let mut report = ImportReport {
        files: 0,
        directories: 0,
        total_bytes: 0,
        skipped: Vec::new(),
    };
    let mut compressed_total = 0u64;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| ArchiveError::Io(error.to_string()))?;

        let entry = ArchiveEntry {
            name: file.name().to_string(),
            is_directory: file.is_dir(),
            is_symlink: false,
            unix_mode: file.unix_mode(),
            compressed_size: file.compressed_size(),
            uncompressed_size: file.size(),
        };

        let Some(safe) = validate_entry(destination_root, &entry, limits, report.total_bytes)?
        else {
            report.skipped.push(entry.name.clone());
            continue;
        };

        if entry.is_directory {
            std::fs::create_dir_all(safe.absolute())
                .map_err(|error| ArchiveError::Io(error.to_string()))?;
            report.directories = report.directories.saturating_add(1);
            continue;
        }

        // A file entry whose parent directory had no entry of its own is normal;
        // archives are not required to list directories.
        if let Some(parent) = safe.absolute().parent() {
            std::fs::create_dir_all(parent).map_err(|error| ArchiveError::Io(error.to_string()))?;
        }

        // The remaining budget, not the declared size, is the ceiling. A header
        // claiming one byte while the stream delivers a gigabyte stops here.
        let remaining = limits
            .max_total_bytes
            .saturating_sub(report.total_bytes)
            .min(limits.max_file_bytes);

        let bytes = read_entry(&mut file, remaining, &entry.name)?;

        std::fs::write(safe.absolute(), &bytes)
            .map_err(|error| ArchiveError::Io(error.to_string()))?;

        report.files = report.files.saturating_add(1);
        report.total_bytes = report.total_bytes.saturating_add(bytes.len() as u64);
        compressed_total = compressed_total.saturating_add(entry.compressed_size);

        // Checked as it inflates rather than at the end, so a bomb is caught
        // while it is still small on disk.
        check_ratio(compressed_total, report.total_bytes, limits)?;
    }

    Ok(report)
}

/// Read one entry with a hard ceiling, naming the entry in the error.
fn read_entry<R: Read>(reader: &mut R, limit: u64, name: &str) -> Result<Vec<u8>, ArchiveError> {
    let mut buffer = Vec::new();
    let mut limited = reader.take(limit.saturating_add(1));
    limited
        .read_to_end(&mut buffer)
        .map_err(|error| ArchiveError::Io(error.to_string()))?;

    if buffer.len() as u64 > limit {
        // Reaching here means the header's declared size was accepted by
        // `validate_entry` and the stream then delivered more, so the entry
        // itself is the problem rather than the archive's total.
        return Err(ArchiveError::FileTooLarge {
            name: name.to_string(),
            limit,
        });
    }
    Ok(buffer)
}

/// Import an archive from disk into a project directory.
///
/// Extraction targets a UUID-named staging directory alongside the destination
/// and is renamed into place only on full success. `import_id` names the staging
/// directory and must be generated by the agent, never taken from the client.
pub fn import_archive_file(
    archive_path: &Path,
    staging_root: &Path,
    destination: &Path,
    import_id: &str,
    limits: &ArchiveLimits,
) -> Result<ImportReport, ArchiveError> {
    let metadata =
        std::fs::metadata(archive_path).map_err(|error| ArchiveError::Io(error.to_string()))?;
    if metadata.len() > limits.max_archive_bytes {
        return Err(ArchiveError::ArchiveTooLarge {
            limit: limits.max_archive_bytes,
        });
    }

    if destination.exists() {
        return Err(ArchiveError::Io(
            "the destination already exists".to_string(),
        ));
    }

    let file =
        std::fs::File::open(archive_path).map_err(|error| ArchiveError::Io(error.to_string()))?;

    let staging = Staging::new(staging_root, import_id)?;
    // On any error the staging directory removes itself as it is dropped, so a
    // partial extraction cannot survive a failure.
    let report = extract_into(std::io::BufReader::new(file), staging.path(), limits)?;
    staging.promote(destination)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// Build an archive in memory so the tests describe the archive rather than
    /// a fixture file nobody can read.
    fn archive_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, contents) in entries {
                if name.ends_with('/') {
                    writer.add_directory(*name, options).expect("add dir");
                } else {
                    writer.start_file(*name, options).expect("start file");
                    writer.write_all(contents).expect("write");
                }
            }
            writer.finish().expect("finish");
        }
        buffer.into_inner()
    }

    fn extract(
        bytes: Vec<u8>,
        limits: &ArchiveLimits,
    ) -> (tempfile::TempDir, Result<ImportReport, ArchiveError>) {
        let dir = tempfile::tempdir().expect("temp dir");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).expect("create");
        let result = extract_into(Cursor::new(bytes), &out, limits);
        (dir, result)
    }

    #[test]
    fn an_ordinary_archive_extracts() {
        let bytes = archive_with(&[
            ("index.js", b"console.log(1);"),
            ("src/", b""),
            ("src/app.ts", b"export const a = 1;"),
        ]);
        let (dir, result) = extract(bytes, &ArchiveLimits::default());
        let report = result.expect("extract");

        assert_eq!(report.files, 2);
        assert_eq!(report.directories, 1);
        assert_eq!(report.total_bytes, 15 + 19);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("out/src/app.ts")).expect("read"),
            "export const a = 1;"
        );
    }

    #[test]
    fn a_file_whose_parent_directory_has_no_entry_still_extracts() {
        let bytes = archive_with(&[("deep/nested/file.txt", b"hi")]);
        let (dir, result) = extract(bytes, &ArchiveLimits::default());
        result.expect("extract");
        assert!(dir.path().join("out/deep/nested/file.txt").is_file());
    }

    #[test]
    fn a_zip_slip_entry_stops_the_import_and_writes_nothing_outside() {
        let bytes = archive_with(&[("good.txt", b"ok"), ("../escaped.txt", b"pwned")]);
        let dir = tempfile::tempdir().expect("temp dir");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).expect("create");

        let result = extract_into(Cursor::new(bytes), &out, &ArchiveLimits::default());
        assert!(matches!(result, Err(ArchiveError::UnsafeEntry { .. })));
        assert!(
            !dir.path().join("escaped.txt").exists(),
            "nothing may be written outside the destination"
        );
    }

    #[test]
    fn a_staged_import_leaves_nothing_behind_when_it_fails() {
        let bytes = archive_with(&[("ok.txt", b"fine"), ("../escape.txt", b"no")]);
        let dir = tempfile::tempdir().expect("temp dir");
        let archive_path = dir.path().join("upload.zip");
        std::fs::write(&archive_path, bytes).expect("write archive");

        let destination = dir.path().join("projects/prj_1");
        let result = import_archive_file(
            &archive_path,
            dir.path(),
            &destination,
            "import-test-id",
            &ArchiveLimits::default(),
        );

        assert!(matches!(result, Err(ArchiveError::UnsafeEntry { .. })));
        assert!(!destination.exists(), "the project must not be created");
        assert!(
            !dir.path().join("import-import-test-id").exists(),
            "the staging directory must have cleaned itself up"
        );
    }

    #[test]
    fn a_successful_staged_import_promotes_into_place() {
        let bytes = archive_with(&[("package.json", b"{}"), ("src/main.js", b"//")]);
        let dir = tempfile::tempdir().expect("temp dir");
        let archive_path = dir.path().join("upload.zip");
        std::fs::write(&archive_path, bytes).expect("write archive");

        let destination = dir.path().join("projects/prj_1");
        let report = import_archive_file(
            &archive_path,
            dir.path(),
            &destination,
            "import-ok",
            &ArchiveLimits::default(),
        )
        .expect("import");

        assert_eq!(report.files, 2);
        assert!(destination.join("src/main.js").is_file());
        assert!(!dir.path().join("import-import-ok").exists());
    }

    #[test]
    fn importing_onto_an_existing_destination_is_refused() {
        let bytes = archive_with(&[("a.txt", b"a")]);
        let dir = tempfile::tempdir().expect("temp dir");
        let archive_path = dir.path().join("upload.zip");
        std::fs::write(&archive_path, bytes).expect("write archive");
        let destination = dir.path().join("existing");
        std::fs::create_dir_all(&destination).expect("create");

        assert!(import_archive_file(
            &archive_path,
            dir.path(),
            &destination,
            "id",
            &ArchiveLimits::default()
        )
        .is_err());
    }

    #[test]
    fn a_compression_bomb_is_stopped_while_it_inflates() {
        // 8 MB of zeroes compresses to a few kilobytes: a ~1000x ratio.
        let bomb = vec![0u8; 8 * 1024 * 1024];
        let bytes = archive_with(&[("bomb.bin", &bomb)]);
        let (_dir, result) = extract(bytes, &ArchiveLimits::default());
        assert!(
            matches!(result, Err(ArchiveError::CompressionRatio { .. })),
            "got {result:?}"
        );
    }

    /// Pseudo-random bytes, so the archive tests the size budget rather than
    /// the compression-ratio check that highly compressible data would trip
    /// first.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state & 0xff) as u8
            })
            .collect()
    }

    #[test]
    fn the_total_budget_stops_an_oversized_archive() {
        let payload = incompressible(200_000);
        let bytes = archive_with(&[("a.bin", &payload), ("b.bin", &payload)]);
        let limits = ArchiveLimits {
            max_total_bytes: 250_000,
            ..ArchiveLimits::default()
        };
        let (_dir, result) = extract(bytes, &limits);
        assert!(matches!(result, Err(ArchiveError::ExpandsTooLarge { .. })));
    }

    #[test]
    fn too_many_entries_is_refused_before_anything_is_written() {
        let bytes = archive_with(&[("a.txt", b"a"), ("b.txt", b"b"), ("c.txt", b"c")]);
        let limits = ArchiveLimits {
            max_entries: 2,
            ..ArchiveLimits::default()
        };
        let (dir, result) = extract(bytes, &limits);
        assert!(matches!(result, Err(ArchiveError::TooManyEntries { .. })));
        assert_eq!(
            std::fs::read_dir(dir.path().join("out"))
                .expect("read dir")
                .count(),
            0
        );
    }

    #[test]
    fn something_that_is_not_a_zip_is_reported_as_such() {
        let dir = tempfile::tempdir().expect("temp dir");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).expect("create");
        let result = extract_into(
            Cursor::new(b"this is not a zip file at all".to_vec()),
            &out,
            &ArchiveLimits::default(),
        );
        assert!(matches!(result, Err(ArchiveError::NotAZip)));
    }

    #[test]
    fn an_archive_larger_than_the_limit_is_refused_without_being_opened() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive_path = dir.path().join("big.zip");
        std::fs::write(&archive_path, vec![0u8; 5000]).expect("write");

        let result = import_archive_file(
            &archive_path,
            dir.path(),
            &dir.path().join("dest"),
            "id",
            &ArchiveLimits {
                max_archive_bytes: 1000,
                ..ArchiveLimits::default()
            },
        );
        assert!(matches!(result, Err(ArchiveError::ArchiveTooLarge { .. })));
    }

    #[test]
    fn metadata_entries_are_skipped_and_reported() {
        let bytes = archive_with(&[
            ("__MACOSX/._index.js", b"junk"),
            ("index.js", b"real"),
            ("src/.DS_Store", b"junk"),
        ]);
        let (dir, result) = extract(bytes, &ArchiveLimits::default());
        let report = result.expect("extract");
        assert_eq!(report.files, 1);
        assert_eq!(report.skipped.len(), 2);
        assert!(!dir.path().join("out/__MACOSX").exists());
    }
}
