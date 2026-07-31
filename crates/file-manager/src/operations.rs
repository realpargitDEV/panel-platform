//! The filesystem operations a project's file explorer performs.
//!
//! Every function here takes the project root plus a *client-supplied* relative
//! string, and the first thing it does is turn that string into a [`SafePath`].
//! There is deliberately no variant that accepts an already-trusted path: a
//! caller cannot skip validation by holding on to one from an earlier request,
//! because the containment check is re-run against the live filesystem
//! immediately before the operation ([`SafePath::verify_still_within`]).
//!
//! Symbolic links are never followed. On Windows that also covers junctions and
//! mount points, which `symlink_metadata` reports as reparse points. A link
//! inside a project is listed, and is refused as the target of anything else —
//! following one is the cheapest way out of a sandbox and there is no legitimate
//! use for it in a project directory the product itself manages.

use std::collections::VecDeque;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::safe_path::{PathError, SafePath};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("`{0}` does not exist")]
    NotFound(String),
    #[error("`{0}` already exists")]
    AlreadyExists(String),
    #[error("`{0}` is a directory")]
    NotAFile(String),
    #[error("`{0}` is not a directory")]
    NotADirectory(String),
    #[error("`{path}` is {size} bytes, above the {limit} byte limit for this operation")]
    TooLarge { path: String, size: u64, limit: u64 },
    #[error("`{0}` is a binary file and cannot be edited as text")]
    Binary(String),
    #[error("a directory cannot be moved or copied into itself")]
    IntoItself,
    #[error("{0}")]
    Refused(&'static str),
    #[error("the directory is not empty")]
    NotEmpty,
    #[error("filesystem error: {0}")]
    Io(String),
}

impl FileError {
    fn io(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Ceilings that keep one request from exhausting the agent.
///
/// The editor limit is far below the download limit on purpose: reading a file
/// into an editor buffer costs memory in the desktop client as well as here,
/// while a download streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileLimits {
    /// Largest file the text editor will open.
    pub max_editable_bytes: u64,
    /// Largest single upload accepted into a project.
    pub max_upload_bytes: u64,
    /// Entries returned for one directory before the listing is truncated.
    pub max_listing_entries: usize,
    /// Matches returned by one search.
    pub max_search_results: usize,
    /// Directories a recursive walk (search, copy, size) will visit.
    pub max_walk_entries: usize,
}

impl Default for FileLimits {
    fn default() -> Self {
        Self {
            max_editable_bytes: 4 * 1024 * 1024,
            max_upload_bytes: 512 * 1024 * 1024,
            max_listing_entries: 5_000,
            max_search_results: 500,
            max_walk_entries: 200_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    /// A symbolic link, junction, device or socket. Listed so the user can see
    /// and delete it; refused as the target of every other operation.
    Other,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Other => "other",
        }
    }
}

/// One row in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    /// Relative to the project root, forward-slashed on every platform.
    pub path: String,
    pub kind: EntryKind,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<i64>,
    /// True for links and other reparse points, which are also [`EntryKind::Other`].
    pub is_symlink: bool,
}

/// A directory, plus whether the listing was cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

/// The content of a text file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub path: String,
    pub text: String,
    pub size_bytes: u64,
    /// The language hint the editor should use, derived from the extension.
    pub language: &'static str,
}

fn modified_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    let modified = metadata.modified().ok()?;
    let since_epoch = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

fn kind_of(metadata: &std::fs::Metadata) -> EntryKind {
    if metadata.is_symlink() {
        EntryKind::Other
    } else if metadata.is_dir() {
        EntryKind::Directory
    } else if metadata.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// `symlink_metadata`, so a link is reported as a link rather than as whatever
/// it points at.
fn entry_from(relative: &str, name: &str, absolute: &Path) -> Result<FileEntry, FileError> {
    let metadata = std::fs::symlink_metadata(absolute).map_err(FileError::io)?;
    Ok(FileEntry {
        name: name.to_string(),
        path: relative.to_string(),
        kind: kind_of(&metadata),
        size_bytes: if metadata.is_dir() { 0 } else { metadata.len() },
        modified_unix_ms: modified_ms(&metadata),
        is_symlink: metadata.is_symlink(),
    })
}

/// Resolve a request path and confirm it still points where it did.
fn resolve(root: &Path, relative: &str) -> Result<SafePath, FileError> {
    let safe = if relative.is_empty() || relative == "." || relative == "/" {
        SafePath::root(root)?
    } else {
        SafePath::new(root, relative)?
    };
    safe.verify_still_within(root)?;
    Ok(safe)
}

/// The last component of a relative path, or the root's own display name.
fn name_of(relative: &str) -> String {
    relative
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(".")
        .to_string()
}

fn require_kind(safe: &SafePath, expected: EntryKind) -> Result<std::fs::Metadata, FileError> {
    let metadata = std::fs::symlink_metadata(safe.absolute())
        .map_err(|_| FileError::NotFound(safe.relative().to_string()))?;
    if metadata.is_symlink() {
        return Err(FileError::Refused(
            "symbolic links are not followed; delete the link instead",
        ));
    }
    match (expected, kind_of(&metadata)) {
        (EntryKind::Directory, EntryKind::Directory) => Ok(metadata),
        (EntryKind::File, EntryKind::File) => Ok(metadata),
        (EntryKind::Directory, _) => Err(FileError::NotADirectory(safe.relative().to_string())),
        (EntryKind::File, EntryKind::Directory) => {
            Err(FileError::NotAFile(safe.relative().to_string()))
        }
        (EntryKind::File, _) => Err(FileError::Refused(
            "only regular files and directories can be operated on",
        )),
        (EntryKind::Other, _) => Err(FileError::Refused("unsupported entry kind")),
    }
}

/// List one directory. Directories first, then files, each alphabetically —
/// stable ordering matters because the client renders this without re-sorting.
pub fn list_directory(
    root: &Path,
    relative: &str,
    limits: &FileLimits,
) -> Result<Listing, FileError> {
    let safe = resolve(root, relative)?;
    require_kind(&safe, EntryKind::Directory)?;

    let mut entries = Vec::new();
    let mut truncated = false;

    for item in std::fs::read_dir(safe.absolute()).map_err(FileError::io)? {
        let item = item.map_err(FileError::io)?;
        let name = item.file_name().to_string_lossy().to_string();

        // A name that would fail validation cannot be addressed by a later
        // request, so listing it as usable would be a lie. It is skipped: it
        // can only have arrived from outside the product.
        let child = match safe.join(root, &name) {
            Ok(child) => child,
            // A link whose target leaves the project is still shown. Hiding it
            // would leave an entry the user can neither inspect nor delete,
            // and `entry_from` stats the link rather than the target, so
            // nothing here follows it.
            Err(PathError::Escape) => match safe.join_no_follow(root, &name) {
                Ok(child) => child,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        if entries.len() >= limits.max_listing_entries {
            truncated = true;
            break;
        }

        match entry_from(child.relative(), &name, child.absolute()) {
            Ok(entry) => entries.push(entry),
            // Vanished between read_dir and stat. Routine in a live project.
            Err(FileError::Io(_)) => continue,
            Err(other) => return Err(other),
        }
    }

    entries.sort_by(|a, b| {
        let rank = |kind: EntryKind| match kind {
            EntryKind::Directory => 0,
            EntryKind::File => 1,
            EntryKind::Other => 2,
        };
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(Listing {
        path: safe.relative().to_string(),
        entries,
        truncated,
    })
}

/// Metadata for a single entry.
pub fn stat(root: &Path, relative: &str) -> Result<FileEntry, FileError> {
    let safe = resolve(root, relative)?;
    if !safe.absolute().exists() && std::fs::symlink_metadata(safe.absolute()).is_err() {
        return Err(FileError::NotFound(safe.relative().to_string()));
    }
    entry_from(safe.relative(), &name_of(safe.relative()), safe.absolute())
}

/// A file is binary if it contains a NUL in its first block, or if the bytes
/// are not valid UTF-8.
///
/// Checking a prefix rather than the whole file is the standard heuristic and is
/// what `git` does; a text file with a NUL a megabyte in is pathological enough
/// that refusing to open it in an editor is the right outcome anyway.
pub fn looks_binary(bytes: &[u8]) -> bool {
    const SNIFF: usize = 8000;
    let head = if bytes.len() > SNIFF {
        bytes.get(..SNIFF).unwrap_or(bytes)
    } else {
        bytes
    };
    if head.contains(&0) {
        return true;
    }
    std::str::from_utf8(head).is_err() && std::str::from_utf8(bytes).is_err()
}

/// The editor's syntax hint. Unknown extensions get plain text rather than a
/// guess, because a wrong highlighter is worse than none.
pub fn language_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    match name.as_str() {
        "dockerfile" => return "dockerfile",
        "makefile" => return "makefile",
        ".env" | ".env.example" | ".env.local" => return "dotenv",
        _ => {}
    }
    match name.rsplit('.').next().unwrap_or("") {
        "js" | "cjs" | "mjs" => "javascript",
        "ts" | "cts" | "mts" => "typescript",
        "jsx" => "jsx",
        "tsx" => "tsx",
        "json" => "json",
        "py" | "pyi" => "python",
        "rs" => "rust",
        "go" => "go",
        "rb" => "ruby",
        "java" => "java",
        "cs" => "csharp",
        "php" => "php",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" => "powershell",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "md" | "markdown" => "markdown",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" => "ini",
        "xml" | "svg" => "xml",
        "txt" | "log" => "plaintext",
        _ => "plaintext",
    }
}

/// Read a file as text for the editor.
///
/// The size is checked from metadata *before* the read, so an oversized file
/// costs a stat rather than a buffer.
pub fn read_text_file(
    root: &Path,
    relative: &str,
    limits: &FileLimits,
) -> Result<TextFile, FileError> {
    let safe = resolve(root, relative)?;
    let metadata = require_kind(&safe, EntryKind::File)?;

    if metadata.len() > limits.max_editable_bytes {
        return Err(FileError::TooLarge {
            path: safe.relative().to_string(),
            size: metadata.len(),
            limit: limits.max_editable_bytes,
        });
    }

    let bytes = std::fs::read(safe.absolute()).map_err(FileError::io)?;
    if looks_binary(&bytes) {
        return Err(FileError::Binary(safe.relative().to_string()));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| FileError::Binary(safe.relative().to_string()))?;

    Ok(TextFile {
        path: safe.relative().to_string(),
        size_bytes: metadata.len(),
        language: language_for(safe.relative()),
        text,
    })
}

/// Write a file, replacing it if it exists.
///
/// Written to a sibling temporary file and renamed over the target, so an
/// interrupted write cannot leave a half-saved source file behind. The
/// temporary name is generated here and never derived from client input.
pub fn write_text_file(
    root: &Path,
    relative: &str,
    contents: &str,
    limits: &FileLimits,
) -> Result<FileEntry, FileError> {
    let safe = resolve(root, relative)?;

    if contents.len() as u64 > limits.max_editable_bytes {
        return Err(FileError::TooLarge {
            path: safe.relative().to_string(),
            size: contents.len() as u64,
            limit: limits.max_editable_bytes,
        });
    }

    // Refuse to replace a directory or a link with a file.
    if let Ok(metadata) = std::fs::symlink_metadata(safe.absolute()) {
        if metadata.is_symlink() {
            return Err(FileError::Refused(
                "symbolic links are not followed; delete the link instead",
            ));
        }
        if metadata.is_dir() {
            return Err(FileError::NotAFile(safe.relative().to_string()));
        }
    }

    write_atomically(safe.absolute(), contents.as_bytes())?;
    entry_from(safe.relative(), &name_of(safe.relative()), safe.absolute())
}

/// Write bytes from an upload. Same atomicity, a much higher ceiling, and no
/// text or UTF-8 assumption.
pub fn write_bytes(
    root: &Path,
    relative: &str,
    bytes: &[u8],
    limits: &FileLimits,
) -> Result<FileEntry, FileError> {
    let safe = resolve(root, relative)?;
    if bytes.len() as u64 > limits.max_upload_bytes {
        return Err(FileError::TooLarge {
            path: safe.relative().to_string(),
            size: bytes.len() as u64,
            limit: limits.max_upload_bytes,
        });
    }
    if let Ok(metadata) = std::fs::symlink_metadata(safe.absolute()) {
        if metadata.is_symlink() || metadata.is_dir() {
            return Err(FileError::Refused(
                "the destination exists and is not a regular file",
            ));
        }
    }
    write_atomically(safe.absolute(), bytes)?;
    entry_from(safe.relative(), &name_of(safe.relative()), safe.absolute())
}

/// Reserve a destination for a chunked upload.
///
/// The real destination must not exist, so dropping a file onto the explorer can
/// never silently replace a user's project file. Chunks land in a generated
/// sibling first and are only renamed into place by [`finish_upload`].
pub fn begin_upload(
    root: &Path,
    relative: &str,
    upload_id: &str,
    total_size: u64,
    limits: &FileLimits,
) -> Result<(), FileError> {
    validate_upload_id(upload_id)?;
    let destination = resolve(root, relative)?;
    validate_upload_destination(&destination, total_size, limits)?;
    let temporary = upload_temporary(root, destination.relative(), upload_id)?;

    if destination.relative() == temporary.relative() {
        return Err(FileError::Refused("that file name is reserved for uploads"));
    }
    if std::fs::symlink_metadata(destination.absolute()).is_ok() {
        return Err(FileError::AlreadyExists(destination.relative().to_string()));
    }
    if let Some(parent) = destination.absolute().parent() {
        std::fs::create_dir_all(parent).map_err(FileError::io)?;
    }

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.absolute())
        .map_err(FileError::io)?;
    Ok(())
}

/// Append one chunk to an upload that was reserved by [`begin_upload`].
pub fn append_upload(
    root: &Path,
    relative: &str,
    upload_id: &str,
    offset: u64,
    bytes: &[u8],
    limits: &FileLimits,
) -> Result<u64, FileError> {
    validate_upload_id(upload_id)?;
    let destination = resolve(root, relative)?;
    let next_offset = offset
        .checked_add(bytes.len() as u64)
        .ok_or(FileError::TooLarge {
            path: destination.relative().to_string(),
            size: u64::MAX,
            limit: limits.max_upload_bytes,
        })?;
    if next_offset > limits.max_upload_bytes {
        return Err(FileError::TooLarge {
            path: destination.relative().to_string(),
            size: next_offset,
            limit: limits.max_upload_bytes,
        });
    }

    let temporary = upload_temporary(root, destination.relative(), upload_id)?;
    let metadata = require_kind(&temporary, EntryKind::File)?;
    if metadata.len() != offset {
        return Err(FileError::Refused(
            "upload chunks arrived out of order; retry the upload",
        ));
    }

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.absolute())
        .map_err(FileError::io)?;
    file.seek(SeekFrom::Start(offset)).map_err(FileError::io)?;
    file.write_all(bytes).map_err(FileError::io)?;
    Ok(next_offset)
}

/// Move a complete chunked upload into place if the destination is still free.
pub fn finish_upload(
    root: &Path,
    relative: &str,
    upload_id: &str,
    total_size: u64,
    limits: &FileLimits,
) -> Result<FileEntry, FileError> {
    validate_upload_id(upload_id)?;
    let destination = resolve(root, relative)?;
    validate_upload_destination(&destination, total_size, limits)?;
    let temporary = upload_temporary(root, destination.relative(), upload_id)?;

    let metadata = require_kind(&temporary, EntryKind::File)?;
    if metadata.len() != total_size {
        return Err(FileError::Refused("upload is incomplete; retry the upload"));
    }
    if std::fs::symlink_metadata(destination.absolute()).is_ok() {
        let _ = std::fs::remove_file(temporary.absolute());
        return Err(FileError::AlreadyExists(destination.relative().to_string()));
    }

    std::fs::rename(temporary.absolute(), destination.absolute()).map_err(FileError::io)?;
    entry_from(
        destination.relative(),
        &name_of(destination.relative()),
        destination.absolute(),
    )
}

/// Remove the temporary file for an upload that was cancelled or failed.
pub fn cancel_upload(root: &Path, relative: &str, upload_id: &str) -> Result<(), FileError> {
    validate_upload_id(upload_id)?;
    let destination = resolve(root, relative)?;
    let temporary = upload_temporary(root, destination.relative(), upload_id)?;

    match std::fs::symlink_metadata(temporary.absolute()) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            Err(FileError::Refused("upload temporary path is a directory"))
        }
        Ok(_) => std::fs::remove_file(temporary.absolute()).map_err(FileError::io),
        Err(_) => Ok(()),
    }
}

fn validate_upload_id(upload_id: &str) -> Result<(), FileError> {
    if !upload_id.is_empty()
        && upload_id.len() <= 80
        && upload_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Ok(());
    }
    Err(FileError::Refused("upload id is invalid"))
}

fn validate_upload_destination(
    destination: &SafePath,
    total_size: u64,
    limits: &FileLimits,
) -> Result<(), FileError> {
    if destination.relative().is_empty() {
        return Err(FileError::Refused(
            "the project root cannot be replaced by an upload",
        ));
    }
    if total_size > limits.max_upload_bytes {
        return Err(FileError::TooLarge {
            path: destination.relative().to_string(),
            size: total_size,
            limit: limits.max_upload_bytes,
        });
    }
    Ok(())
}

fn upload_temporary(root: &Path, relative: &str, upload_id: &str) -> Result<SafePath, FileError> {
    let name = format!(".project-host-upload-{upload_id}.tmp");
    let temporary = match relative.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => format!("{parent}/{name}"),
        _ => name,
    };
    resolve(root, &temporary)
}

fn write_atomically(target: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let parent = target.parent().ok_or(FileError::Refused(
        "the destination has no parent directory",
    ))?;
    std::fs::create_dir_all(parent).map_err(FileError::io)?;

    let temporary = parent.join(format!(".project-host-{}.tmp", unique_suffix()));
    let result = (|| {
        std::fs::write(&temporary, bytes).map_err(FileError::io)?;
        // Rename over the target: on both platforms this replaces atomically
        // for files on the same volume, which a sibling always is.
        std::fs::rename(&temporary, target).map_err(FileError::io)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// A collision-resistant suffix without pulling in a UUID dependency here.
/// Monotonic time plus the thread id is enough for a file that lives for
/// microseconds inside a directory only this process writes to.
fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{:?}", std::thread::current().id())
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "")
}

/// Create an empty file. Fails if anything is already there — an explorer's
/// "new file" must never silently truncate.
pub fn create_file(root: &Path, relative: &str) -> Result<FileEntry, FileError> {
    let safe = resolve(root, relative)?;
    if std::fs::symlink_metadata(safe.absolute()).is_ok() {
        return Err(FileError::AlreadyExists(safe.relative().to_string()));
    }
    if let Some(parent) = safe.absolute().parent() {
        std::fs::create_dir_all(parent).map_err(FileError::io)?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(safe.absolute())
        .map_err(FileError::io)?;
    entry_from(safe.relative(), &name_of(safe.relative()), safe.absolute())
}

pub fn create_directory(root: &Path, relative: &str) -> Result<FileEntry, FileError> {
    let safe = resolve(root, relative)?;
    if std::fs::symlink_metadata(safe.absolute()).is_ok() {
        return Err(FileError::AlreadyExists(safe.relative().to_string()));
    }
    std::fs::create_dir_all(safe.absolute()).map_err(FileError::io)?;
    entry_from(safe.relative(), &name_of(safe.relative()), safe.absolute())
}

/// Delete a file, a link, or a directory.
///
/// `recursive` is required for a non-empty directory: the caller has to state
/// that it means it, and the API layer turns that into a confirmation dialog.
pub fn delete(root: &Path, relative: &str, recursive: bool) -> Result<(), FileError> {
    let safe = match resolve(root, relative) {
        Ok(safe) => safe,
        // A link out of the project fails the containment check, but removing
        // the link never touches what it points at — and the listing shows it,
        // so refusing would leave an entry that cannot be got rid of. The
        // fallback is deliberately narrow: only a symlink takes it, and
        // anything else still escapes.
        Err(FileError::Path(PathError::Escape)) => {
            let candidate = SafePath::new_no_follow(root, relative)?;
            let is_link = std::fs::symlink_metadata(candidate.absolute())
                .map(|metadata| metadata.is_symlink())
                .unwrap_or(false);
            if !is_link {
                return Err(FileError::Path(PathError::Escape));
            }
            candidate
        }
        Err(other) => return Err(other),
    };

    if safe.relative().is_empty() {
        return Err(FileError::Refused(
            "the project root cannot be deleted from the file explorer",
        ));
    }

    let metadata = std::fs::symlink_metadata(safe.absolute())
        .map_err(|_| FileError::NotFound(safe.relative().to_string()))?;

    if metadata.is_symlink() {
        // Remove the link itself, never its target. `remove_file` is correct
        // for a file symlink on both platforms; a directory symlink or junction
        // on Windows needs `remove_dir`, which does not touch the target either.
        return std::fs::remove_file(safe.absolute())
            .or_else(|_| std::fs::remove_dir(safe.absolute()))
            .map_err(FileError::io);
    }

    if metadata.is_dir() {
        if recursive {
            return std::fs::remove_dir_all(safe.absolute()).map_err(FileError::io);
        }
        return match std::fs::remove_dir(safe.absolute()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Other => Err(FileError::NotEmpty),
            // Portable detection of "not empty" is unreliable across platforms
            // and Rust versions, so fall back to asking the directory.
            Err(error) => {
                let empty = std::fs::read_dir(safe.absolute())
                    .map(|mut items| items.next().is_none())
                    .unwrap_or(false);
                if empty {
                    Err(FileError::io(error))
                } else {
                    Err(FileError::NotEmpty)
                }
            }
        };
    }

    std::fs::remove_file(safe.absolute()).map_err(FileError::io)
}

/// Rename within the same parent directory.
///
/// `new_name` is a single component, validated by joining it to the parent
/// through [`SafePath`] — so `../elsewhere` is refused by the same rule that
/// refuses it everywhere else, rather than by an ad-hoc check here.
pub fn rename(root: &Path, relative: &str, new_name: &str) -> Result<FileEntry, FileError> {
    if new_name.contains('/') || new_name.contains('\\') {
        return Err(FileError::Refused(
            "a new name must be a single path component; use move instead",
        ));
    }
    let safe = resolve(root, relative)?;
    if safe.relative().is_empty() {
        return Err(FileError::Refused("the project root cannot be renamed"));
    }

    let parent = match safe.relative().rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    };
    let destination = if parent.is_empty() {
        resolve(root, new_name)?
    } else {
        resolve(root, &format!("{parent}/{new_name}"))?
    };

    move_resolved(safe, destination)
}

/// Move an entry anywhere inside the same project.
pub fn move_entry(root: &Path, from: &str, to: &str) -> Result<FileEntry, FileError> {
    let source = resolve(root, from)?;
    let destination = resolve(root, to)?;
    if source.relative().is_empty() {
        return Err(FileError::Refused("the project root cannot be moved"));
    }
    move_resolved(source, destination)
}

fn move_resolved(source: SafePath, destination: SafePath) -> Result<FileEntry, FileError> {
    if std::fs::symlink_metadata(source.absolute()).is_err() {
        return Err(FileError::NotFound(source.relative().to_string()));
    }
    if std::fs::symlink_metadata(destination.absolute()).is_ok() {
        return Err(FileError::AlreadyExists(destination.relative().to_string()));
    }
    if is_prefix_of(source.relative(), destination.relative()) {
        return Err(FileError::IntoItself);
    }
    if let Some(parent) = destination.absolute().parent() {
        std::fs::create_dir_all(parent).map_err(FileError::io)?;
    }
    std::fs::rename(source.absolute(), destination.absolute()).map_err(FileError::io)?;
    entry_from(
        destination.relative(),
        &name_of(destination.relative()),
        destination.absolute(),
    )
}

/// True when `parent` names an ancestor of `child`, compared component-wise so
/// `src` is not treated as an ancestor of `srcfile.js`.
fn is_prefix_of(parent: &str, child: &str) -> bool {
    if parent.is_empty() {
        return true;
    }
    if parent == child {
        return true;
    }
    child.starts_with(parent) && child.as_bytes().get(parent.len()) == Some(&b'/')
}

/// Copy a file or a whole directory tree.
///
/// The walk is bounded by `max_walk_entries` and refuses links rather than
/// following them, so copying cannot be tricked into pulling content in from
/// outside the project.
pub fn copy(
    root: &Path,
    from: &str,
    to: &str,
    limits: &FileLimits,
) -> Result<FileEntry, FileError> {
    let source = resolve(root, from)?;
    let destination = resolve(root, to)?;

    if source.relative().is_empty() {
        return Err(FileError::Refused("the project root cannot be copied"));
    }
    if std::fs::symlink_metadata(destination.absolute()).is_ok() {
        return Err(FileError::AlreadyExists(destination.relative().to_string()));
    }
    if is_prefix_of(source.relative(), destination.relative()) {
        return Err(FileError::IntoItself);
    }

    let metadata = require_kind(&source, EntryKind::Directory)
        .or_else(|_| require_kind(&source, EntryKind::File))?;

    if let Some(parent) = destination.absolute().parent() {
        std::fs::create_dir_all(parent).map_err(FileError::io)?;
    }

    if metadata.is_file() {
        std::fs::copy(source.absolute(), destination.absolute()).map_err(FileError::io)?;
    } else {
        copy_tree(source.absolute(), destination.absolute(), limits)?;
    }

    entry_from(
        destination.relative(),
        &name_of(destination.relative()),
        destination.absolute(),
    )
}

fn copy_tree(source: &Path, destination: &Path, limits: &FileLimits) -> Result<(), FileError> {
    let mut queue = VecDeque::from([(source.to_path_buf(), destination.to_path_buf())]);
    let mut visited = 0usize;

    while let Some((from, to)) = queue.pop_front() {
        visited += 1;
        if visited > limits.max_walk_entries {
            return Err(FileError::Refused(
                "the directory contains too many entries to copy",
            ));
        }

        std::fs::create_dir_all(&to).map_err(FileError::io)?;

        for item in std::fs::read_dir(&from).map_err(FileError::io)? {
            let item = item.map_err(FileError::io)?;
            let metadata = item.metadata().map_err(FileError::io)?;
            let child_to = to.join(item.file_name());

            if metadata.is_symlink() {
                // Copying a link would either duplicate an escape or silently
                // dereference it. Neither is acceptable, so the copy fails.
                return Err(FileError::Refused(
                    "the directory contains a symbolic link and cannot be copied",
                ));
            }
            if metadata.is_dir() {
                queue.push_back((item.path(), child_to));
            } else if metadata.is_file() {
                std::fs::copy(item.path(), &child_to).map_err(FileError::io)?;
            }
        }
    }
    Ok(())
}

/// Search filenames beneath a directory.
///
/// Names only, not content: a content search over an unbounded project tree is
/// a denial-of-service primitive, and the editor's own search covers the file
/// the user is looking at.
pub fn search(
    root: &Path,
    within: &str,
    query: &str,
    limits: &FileLimits,
) -> Result<Vec<FileEntry>, FileError> {
    let start = resolve(root, within)?;
    require_kind(&start, EntryKind::Directory)?;

    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let mut queue = VecDeque::from([start]);
    let mut visited = 0usize;

    while let Some(directory) = queue.pop_front() {
        visited += 1;
        if visited > limits.max_walk_entries || results.len() >= limits.max_search_results {
            break;
        }

        let items = match std::fs::read_dir(directory.absolute()) {
            Ok(items) => items,
            // An unreadable subdirectory should not fail the whole search.
            Err(_) => continue,
        };

        for item in items.flatten() {
            let name = item.file_name().to_string_lossy().to_string();
            let Ok(child) = directory.join(root, &name) else {
                continue;
            };
            let Ok(metadata) = std::fs::symlink_metadata(child.absolute()) else {
                continue;
            };

            if name.to_lowercase().contains(&needle) && results.len() < limits.max_search_results {
                results.push(FileEntry {
                    name: name.clone(),
                    path: child.relative().to_string(),
                    kind: kind_of(&metadata),
                    size_bytes: if metadata.is_dir() { 0 } else { metadata.len() },
                    modified_unix_ms: modified_ms(&metadata),
                    is_symlink: metadata.is_symlink(),
                });
            }

            // Links are never descended into: that is how a search escapes.
            if metadata.is_dir() && !metadata.is_symlink() {
                queue.push_back(child);
            }
        }
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(results)
}

/// Total size of a directory tree, bounded by the same walk limit.
///
/// Used for the project's disk figure and to decide whether a backup will fit.
pub fn directory_size(root: &Path, relative: &str, limits: &FileLimits) -> Result<u64, FileError> {
    let start = resolve(root, relative)?;
    require_kind(&start, EntryKind::Directory)?;

    let mut total = 0u64;
    let mut queue = VecDeque::from([start.absolute().to_path_buf()]);
    let mut visited = 0usize;

    while let Some(directory) = queue.pop_front() {
        visited += 1;
        if visited > limits.max_walk_entries {
            break;
        }
        let Ok(items) = std::fs::read_dir(&directory) else {
            continue;
        };
        for item in items.flatten() {
            let Ok(metadata) = item.metadata() else {
                continue;
            };
            if metadata.is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                queue.push_back(item.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project {
        _dir: tempfile::TempDir,
        root: std::path::PathBuf,
    }

    fn project() -> Project {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("prj_test");
        std::fs::create_dir_all(root.join("src")).expect("create");
        std::fs::write(root.join("index.js"), "console.log('hi');\n").expect("write");
        std::fs::write(root.join("src/app.ts"), "export const a = 1;\n").expect("write");
        std::fs::write(root.join("README.md"), "# hello\n").expect("write");
        Project { _dir: dir, root }
    }

    fn limits() -> FileLimits {
        FileLimits::default()
    }

    #[test]
    fn the_root_listing_puts_directories_first() {
        let p = project();
        let listing = list_directory(&p.root, "", &limits()).expect("list");
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["src", "index.js", "README.md"]);
        assert!(!listing.truncated);
    }

    #[test]
    fn a_listing_reports_sizes_and_kinds() {
        let p = project();
        let listing = list_directory(&p.root, "", &limits()).expect("list");
        let index = listing
            .entries
            .iter()
            .find(|e| e.name == "index.js")
            .expect("index.js");
        assert_eq!(index.kind, EntryKind::File);
        assert_eq!(index.size_bytes, 19);
        assert_eq!(index.path, "index.js");
        assert!(index.modified_unix_ms.is_some());
    }

    #[test]
    fn traversal_is_refused_by_every_operation() {
        let p = project();
        let l = limits();
        assert!(matches!(
            list_directory(&p.root, "../", &l),
            Err(FileError::Path(_))
        ));
        assert!(matches!(
            read_text_file(&p.root, "../secrets.txt", &l),
            Err(FileError::Path(_))
        ));
        assert!(matches!(
            write_text_file(&p.root, "../evil.js", "x", &l),
            Err(FileError::Path(_))
        ));
        assert!(matches!(
            delete(&p.root, "../anything", true),
            Err(FileError::Path(_))
        ));
        assert!(matches!(
            create_directory(&p.root, "../outside"),
            Err(FileError::Path(_))
        ));
        assert!(matches!(
            move_entry(&p.root, "index.js", "../stolen.js"),
            Err(FileError::Path(_))
        ));
        assert!(matches!(
            copy(&p.root, "index.js", "../stolen.js", &l),
            Err(FileError::Path(_))
        ));
    }

    #[test]
    fn a_listing_truncates_rather_than_growing_without_bound() {
        let p = project();
        let bounded = FileLimits {
            max_listing_entries: 2,
            ..limits()
        };
        let listing = list_directory(&p.root, "", &bounded).expect("list");
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.truncated);
    }

    #[test]
    fn reading_a_text_file_returns_its_content_and_language() {
        let p = project();
        let file = read_text_file(&p.root, "src/app.ts", &limits()).expect("read");
        assert_eq!(file.text, "export const a = 1;\n");
        assert_eq!(file.language, "typescript");
        assert_eq!(file.path, "src/app.ts");
    }

    #[test]
    fn reading_a_directory_as_a_file_is_refused() {
        let p = project();
        assert!(matches!(
            read_text_file(&p.root, "src", &limits()),
            Err(FileError::NotAFile(_))
        ));
    }

    #[test]
    fn an_oversized_file_is_refused_without_being_read() {
        let p = project();
        let tiny = FileLimits {
            max_editable_bytes: 4,
            ..limits()
        };
        assert!(matches!(
            read_text_file(&p.root, "index.js", &tiny),
            Err(FileError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_binary_file_is_refused_by_the_editor() {
        let p = project();
        std::fs::write(p.root.join("app.bin"), [0x00, 0x01, 0x02, 0xff]).expect("write");
        assert!(matches!(
            read_text_file(&p.root, "app.bin", &limits()),
            Err(FileError::Binary(_))
        ));
    }

    #[test]
    fn utf8_text_with_accents_is_not_mistaken_for_binary() {
        let p = project();
        std::fs::write(p.root.join("notes.md"), "café — naïve ✅\n").expect("write");
        let file = read_text_file(&p.root, "notes.md", &limits()).expect("read");
        assert!(file.text.contains("café"));
    }

    #[test]
    fn writing_replaces_content_atomically_and_leaves_no_temporary() {
        let p = project();
        write_text_file(&p.root, "index.js", "// replaced\n", &limits()).expect("write");
        assert_eq!(
            std::fs::read_to_string(p.root.join("index.js")).expect("read"),
            "// replaced\n"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&p.root)
            .expect("read dir")
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".project-host-")
            })
            .collect();
        assert!(leftovers.is_empty(), "temporary file left behind");
    }

    #[test]
    fn writing_creates_missing_parent_directories() {
        let p = project();
        write_text_file(&p.root, "a/b/c/new.txt", "hi", &limits()).expect("write");
        assert!(p.root.join("a/b/c/new.txt").is_file());
    }

    #[test]
    fn writing_over_a_directory_is_refused() {
        let p = project();
        assert!(matches!(
            write_text_file(&p.root, "src", "x", &limits()),
            Err(FileError::NotAFile(_))
        ));
    }

    #[test]
    fn an_upload_lands_in_place_after_ordered_chunks() {
        let p = project();
        begin_upload(&p.root, "assets/image.bin", "upload-1", 11, &limits()).expect("begin upload");
        assert!(!p.root.join("assets/image.bin").exists());

        assert_eq!(
            append_upload(
                &p.root,
                "assets/image.bin",
                "upload-1",
                0,
                b"hello ",
                &limits()
            )
            .expect("append first"),
            6
        );
        assert_eq!(
            append_upload(
                &p.root,
                "assets/image.bin",
                "upload-1",
                6,
                b"world",
                &limits()
            )
            .expect("append second"),
            11
        );

        let entry = finish_upload(&p.root, "assets/image.bin", "upload-1", 11, &limits())
            .expect("finish upload");
        assert_eq!(entry.path, "assets/image.bin");
        assert_eq!(
            std::fs::read(p.root.join("assets/image.bin")).expect("read upload"),
            b"hello world"
        );
        assert!(!p
            .root
            .join("assets/.project-host-upload-upload-1.tmp")
            .exists());
    }

    #[test]
    fn an_upload_refuses_to_start_when_the_destination_exists() {
        let p = project();
        assert!(matches!(
            begin_upload(&p.root, "index.js", "upload-2", 3, &limits()),
            Err(FileError::AlreadyExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(p.root.join("index.js")).expect("read"),
            "console.log('hi');\n"
        );
    }

    #[test]
    fn a_cancelled_upload_removes_its_temporary_file() {
        let p = project();
        begin_upload(&p.root, "new.txt", "upload_3", 10, &limits()).expect("begin upload");
        append_upload(&p.root, "new.txt", "upload_3", 0, b"partial", &limits()).expect("append");

        cancel_upload(&p.root, "new.txt", "upload_3").expect("cancel");

        assert!(!p.root.join("new.txt").exists());
        assert!(!p.root.join(".project-host-upload-upload_3.tmp").exists());
    }

    #[test]
    fn finishing_an_upload_refuses_a_late_collision() {
        let p = project();
        begin_upload(&p.root, "fresh.txt", "upload-4", 3, &limits()).expect("begin upload");
        append_upload(&p.root, "fresh.txt", "upload-4", 0, b"new", &limits()).expect("append");
        std::fs::write(p.root.join("fresh.txt"), "old").expect("race write");

        assert!(matches!(
            finish_upload(&p.root, "fresh.txt", "upload-4", 3, &limits()),
            Err(FileError::AlreadyExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(p.root.join("fresh.txt")).expect("read"),
            "old"
        );
        assert!(!p.root.join(".project-host-upload-upload-4.tmp").exists());
    }

    #[test]
    fn creating_a_file_that_exists_is_refused_rather_than_truncating() {
        let p = project();
        assert!(matches!(
            create_file(&p.root, "index.js"),
            Err(FileError::AlreadyExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(p.root.join("index.js")).expect("read"),
            "console.log('hi');\n",
            "the existing file must be untouched"
        );
    }

    #[test]
    fn a_non_empty_directory_needs_the_recursive_flag() {
        let p = project();
        assert!(matches!(
            delete(&p.root, "src", false),
            Err(FileError::NotEmpty)
        ));
        assert!(p.root.join("src/app.ts").exists());
        delete(&p.root, "src", true).expect("recursive delete");
        assert!(!p.root.join("src").exists());
    }

    #[test]
    fn the_project_root_cannot_be_deleted() {
        let p = project();
        assert!(matches!(
            delete(&p.root, "", true),
            Err(FileError::Refused(_))
        ));
        assert!(p.root.exists());
    }

    #[test]
    fn renaming_moves_within_the_same_directory() {
        let p = project();
        let entry = rename(&p.root, "src/app.ts", "main.ts").expect("rename");
        assert_eq!(entry.path, "src/main.ts");
        assert!(p.root.join("src/main.ts").is_file());
        assert!(!p.root.join("src/app.ts").exists());
    }

    #[test]
    fn a_rename_cannot_smuggle_a_path_separator() {
        let p = project();
        for name in ["../escaped.ts", "sub/dir.ts", "..\\escaped.ts"] {
            assert!(
                matches!(
                    rename(&p.root, "src/app.ts", name),
                    Err(FileError::Refused(_))
                ),
                "{name} should be refused"
            );
        }
    }

    #[test]
    fn renaming_onto_an_existing_name_is_refused() {
        let p = project();
        assert!(matches!(
            rename(&p.root, "index.js", "README.md"),
            Err(FileError::AlreadyExists(_))
        ));
    }

    #[test]
    fn moving_a_directory_into_itself_is_refused() {
        let p = project();
        assert!(matches!(
            move_entry(&p.root, "src", "src/nested"),
            Err(FileError::IntoItself)
        ));
    }

    #[test]
    fn a_sibling_with_a_shared_prefix_is_not_treated_as_a_descendant() {
        // `src` must not look like an ancestor of `srcbackup`.
        let p = project();
        let entry = copy(&p.root, "src", "srcbackup", &limits()).expect("copy");
        assert_eq!(entry.path, "srcbackup");
        assert!(p.root.join("srcbackup/app.ts").is_file());
    }

    #[test]
    fn copying_a_tree_reproduces_it() {
        let p = project();
        std::fs::create_dir_all(p.root.join("src/deep")).expect("create");
        std::fs::write(p.root.join("src/deep/x.js"), "1").expect("write");
        copy(&p.root, "src", "copy", &limits()).expect("copy");
        assert!(p.root.join("copy/app.ts").is_file());
        assert!(p.root.join("copy/deep/x.js").is_file());
        assert!(p.root.join("src/app.ts").is_file(), "source must remain");
    }

    #[test]
    fn copying_onto_an_existing_path_is_refused() {
        let p = project();
        assert!(matches!(
            copy(&p.root, "index.js", "README.md", &limits()),
            Err(FileError::AlreadyExists(_))
        ));
    }

    #[test]
    fn search_matches_names_case_insensitively() {
        let p = project();
        let hits = search(&p.root, "", "APP", &limits()).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/app.ts");
    }

    #[test]
    fn search_is_capped_and_an_empty_query_matches_nothing() {
        let p = project();
        assert!(search(&p.root, "", "   ", &limits())
            .expect("search")
            .is_empty());

        for i in 0..10 {
            std::fs::write(p.root.join(format!("match{i}.txt")), "x").expect("write");
        }
        let capped = FileLimits {
            max_search_results: 3,
            ..limits()
        };
        assert_eq!(
            search(&p.root, "", "match", &capped).expect("search").len(),
            3
        );
    }

    #[test]
    fn directory_size_sums_the_tree() {
        let p = project();
        let total = directory_size(&p.root, "", &limits()).expect("size");
        assert_eq!(total, 19 + 20 + 8);
    }

    #[test]
    fn stat_reports_a_missing_file_as_missing() {
        let p = project();
        assert!(matches!(
            stat(&p.root, "nope.js"),
            Err(FileError::NotFound(_))
        ));
    }

    #[test]
    fn language_detection_covers_names_without_extensions() {
        assert_eq!(language_for("Dockerfile"), "dockerfile");
        assert_eq!(language_for("a/b/.env"), "dotenv");
        assert_eq!(language_for("src/main.py"), "python");
        assert_eq!(language_for("weird.qqq"), "plaintext");
        assert_eq!(language_for("noextension"), "plaintext");
    }

    #[test]
    fn binary_sniffing_uses_the_prefix() {
        assert!(looks_binary(&[b'a', 0, b'b']));
        assert!(!looks_binary(b"plain text"));
        assert!(!looks_binary("caf\u{e9}".as_bytes()));
        // Invalid UTF-8 without a NUL is still binary.
        assert!(looks_binary(&[0xff, 0xfe, 0xfd]));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_escaping_the_project_is_refused() {
        use std::os::unix::fs::symlink;
        let p = project();
        let outside = p.root.parent().expect("parent").join("outside.txt");
        std::fs::write(&outside, "secret").expect("write");
        symlink(&outside, p.root.join("link.txt")).expect("symlink");

        assert!(matches!(
            read_text_file(&p.root, "link.txt", &limits()),
            Err(FileError::Path(_)) | Err(FileError::Refused(_))
        ));
        // It is still listed, so the user can see and remove it.
        let listing = list_directory(&p.root, "", &limits()).expect("list");
        let link = listing
            .entries
            .iter()
            .find(|e| e.name == "link.txt")
            .expect("link listed");
        assert!(link.is_symlink);
        assert_eq!(link.kind, EntryKind::Other);

        // Deleting removes the link, not the target.
        delete(&p.root, "link.txt", false).expect("delete link");
        assert!(outside.exists(), "the target must survive");
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_containing_a_symlink_cannot_be_copied() {
        use std::os::unix::fs::symlink;
        let p = project();
        symlink("/etc/passwd", p.root.join("src/link")).expect("symlink");
        assert!(matches!(
            copy(&p.root, "src", "copy", &limits()),
            Err(FileError::Refused(_))
        ));
    }
}
