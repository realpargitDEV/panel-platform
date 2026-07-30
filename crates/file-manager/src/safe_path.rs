//! The only way to name a file inside a project.
//!
//! Every filesystem function in this crate takes a [`SafePath`] and nothing
//! else. A `&str` from a request cannot reach `std::fs`, because the functions
//! that would accept one do not exist.
//!
//! Validation order matters and is deliberate: **reject before normalising.**
//! Normalising attacker input and then checking the result is how traversal
//! bugs survive review — `a/../../b` normalises to something plausible, and the
//! interesting question is why `..` was there at all.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("path must be relative to the project root")]
    Absolute,
    #[error("path must not contain `..`")]
    Traversal,
    #[error("path contains an invalid character")]
    InvalidCharacter,
    #[error("`{0}` is a reserved device name on Windows")]
    ReservedName(String),
    #[error("path component must not end with a dot or a space")]
    TrailingDotOrSpace,
    #[error("path escapes the project directory")]
    Escape,
    #[error("path is too long")]
    TooLong,
    #[error("too many path components")]
    TooDeep,
    #[error("the project root could not be resolved: {0}")]
    UnresolvableRoot(String),
}

/// Windows resolves these regardless of extension or directory, so `CON.txt`
/// is still the console device. Rejected on every platform: an archive is
/// portable, and a Linux agent must not produce files a Windows client cannot
/// handle.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub const MAX_PATH_LENGTH: usize = 1024;
pub const MAX_DEPTH: usize = 32;

/// A path proven to be inside a project root.
///
/// Construction is the only way to obtain one, and construction validates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafePath {
    /// Canonical absolute path on this host.
    absolute: PathBuf,
    /// The relative form, for display and for API responses.
    relative: String,
}

impl SafePath {
    /// Validate a client-supplied relative path against a project root.
    ///
    /// The root is canonicalised first, so a symlinked projects directory does
    /// not make every containment check fail.
    pub fn new(root: &Path, relative: &str) -> Result<Self, PathError> {
        let canonical_root = canonicalise(root)?;
        let cleaned = validate_relative(relative)?;

        let joined = canonical_root.join(&cleaned);

        // Resolve links where the path exists. Paths that do not exist yet are
        // routine — a file about to be created, or a whole directory tree about
        // to be extracted — so resolution walks up to the deepest ancestor that
        // *does* exist, canonicalises that, and re-appends the rest.
        //
        // Resolving only the immediate parent would be wrong: extracting
        // `src/deep/file.js` into an empty project has no existing parent
        // either.
        let resolved = resolve_deepest_existing(&canonical_root, &joined)?;

        if !is_within(&canonical_root, &resolved) {
            return Err(PathError::Escape);
        }

        Ok(Self {
            absolute: resolved,
            relative: cleaned.to_string_lossy().replace('\\', "/"),
        })
    }

    /// Name the final component *without following it*.
    ///
    /// [`SafePath::new`] resolves the whole path, so a symlink pointing out of
    /// the project is an [`PathError::Escape`] and cannot be addressed at all.
    /// That is right for reading and writing — following such a link is the
    /// cheapest way out of the sandbox — but it also made the link invisible to
    /// listing and impossible to delete, which contradicts this module's
    /// contract that a link is shown and refused as the target of anything
    /// else. A user cannot remove what they cannot see.
    ///
    /// So the *parent* is resolved and must lie within the root, and the last
    /// component is appended literally. The result names the link itself, which
    /// is what `symlink_metadata` and `remove_file` act on; neither touches the
    /// target. Reading and writing still go through [`SafePath::new`] and are
    /// still refused.
    pub fn new_no_follow(root: &Path, relative: &str) -> Result<Self, PathError> {
        let canonical_root = canonicalise(root)?;
        let cleaned = validate_relative(relative)?;

        let name = cleaned.file_name().ok_or(PathError::Empty)?.to_os_string();
        let parent = match cleaned.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                canonicalise(&canonical_root.join(parent))?
            }
            _ => canonical_root.clone(),
        };

        // The containment check moves to the parent. A name within a directory
        // that is itself inside the project cannot escape, whatever it points
        // at, because nothing here dereferences it.
        if !is_within(&canonical_root, &parent) {
            return Err(PathError::Escape);
        }

        Ok(Self {
            absolute: parent.join(&name),
            relative: cleaned.to_string_lossy().replace('\\', "/"),
        })
    }

    /// The project root itself.
    pub fn root(root: &Path) -> Result<Self, PathError> {
        let canonical = canonicalise(root)?;
        Ok(Self {
            absolute: canonical,
            relative: String::new(),
        })
    }

    pub fn absolute(&self) -> &Path {
        &self.absolute
    }

    pub fn relative(&self) -> &str {
        &self.relative
    }

    /// Re-verify containment immediately before use.
    ///
    /// This is the TOCTOU guard. Steps in [`SafePath::new`] validate a *name*,
    /// and a name can be swapped for a symlink between validation and open. A
    /// caller that is about to act re-checks against the live filesystem.
    pub fn verify_still_within(&self, root: &Path) -> Result<(), PathError> {
        let canonical_root = canonicalise(root)?;
        let current = match canonicalise(&self.absolute) {
            Ok(path) => path,
            // Still absent: the parent check in `new` already covered it.
            Err(_) => return Ok(()),
        };
        if is_within(&canonical_root, &current) {
            Ok(())
        } else {
            Err(PathError::Escape)
        }
    }

    /// Join a further relative segment, revalidating from the root.
    pub fn join(&self, root: &Path, segment: &str) -> Result<Self, PathError> {
        let combined = if self.relative.is_empty() {
            segment.to_string()
        } else {
            format!("{}/{}", self.relative, segment)
        };
        SafePath::new(root, &combined)
    }

    /// [`SafePath::join`], without following the joined component.
    ///
    /// See [`SafePath::new_no_follow`] for why this exists and what it does not
    /// permit.
    pub fn join_no_follow(&self, root: &Path, segment: &str) -> Result<Self, PathError> {
        let combined = if self.relative.is_empty() {
            segment.to_string()
        } else {
            format!("{}/{}", self.relative, segment)
        };
        SafePath::new_no_follow(root, &combined)
    }
}

/// Reject anything suspicious, then return the cleaned relative path.
fn validate_relative(relative: &str) -> Result<PathBuf, PathError> {
    if relative.is_empty() {
        return Err(PathError::Empty);
    }
    if relative.len() > MAX_PATH_LENGTH {
        return Err(PathError::TooLong);
    }

    // NUL truncates a path in every C API underneath us; other control
    // characters produce filenames nothing can display or delete.
    if relative.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(PathError::InvalidCharacter);
    }

    let normalised = relative.replace('\\', "/");

    // A leading slash, a drive letter, or a UNC prefix all mean "not relative".
    if normalised.starts_with('/') {
        return Err(PathError::Absolute);
    }
    if normalised.starts_with("//") {
        return Err(PathError::Absolute);
    }
    if normalised.as_bytes().get(1) == Some(&b':') {
        return Err(PathError::Absolute);
    }

    // Rejected before normalisation, on purpose.
    if normalised.split('/').any(|component| component == "..") {
        return Err(PathError::Traversal);
    }

    let mut cleaned = PathBuf::new();
    let mut depth = 0usize;

    for component in normalised.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }

        depth += 1;
        if depth > MAX_DEPTH {
            return Err(PathError::TooDeep);
        }

        // An alternate data stream: `file.txt:hidden` writes to a second,
        // invisible stream on NTFS.
        if component.contains(':') {
            return Err(PathError::InvalidCharacter);
        }

        // Windows silently strips these, so `evil.txt.` and `evil.txt` are the
        // same file — a way to bypass an extension check.
        if component.ends_with('.') || component.ends_with(' ') {
            return Err(PathError::TrailingDotOrSpace);
        }

        let stem = component
            .split('.')
            .next()
            .unwrap_or(component)
            .to_ascii_uppercase();
        if RESERVED_NAMES.contains(&stem.as_str()) {
            return Err(PathError::ReservedName(component.to_string()));
        }

        cleaned.push(component);
    }

    if cleaned.as_os_str().is_empty() {
        return Err(PathError::Empty);
    }

    Ok(cleaned)
}

/// Canonicalise as much of `target` as exists, keeping the remainder literal.
///
/// The existing prefix is resolved, so a symlinked ancestor pointing outside
/// the root is still caught. The non-existent tail cannot contain a link — it
/// does not exist — so appending it literally is sound.
fn resolve_deepest_existing(root: &Path, target: &Path) -> Result<PathBuf, PathError> {
    let mut existing = target.to_path_buf();
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();

    loop {
        if existing.exists() {
            break;
        }
        let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
            // Ran past the filesystem root without finding anything.
            return Err(PathError::Escape);
        };
        remainder.push(name);
        match existing.parent() {
            Some(parent) => existing = parent.to_path_buf(),
            None => return Err(PathError::Escape),
        }
        // Never climb above the project root while searching.
        if existing.components().count() < root.components().count() {
            return Err(PathError::Escape);
        }
    }

    let mut resolved = canonicalise(&existing)?;
    for name in remainder.iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// Canonicalise without the `\\?\` prefix Windows adds, which would otherwise
/// leak into display and break prefix comparison.
fn canonicalise(path: &Path) -> Result<PathBuf, PathError> {
    dunce::canonicalize(path).map_err(|error| PathError::UnresolvableRoot(error.to_string()))
}

/// Component-wise containment, case-folded on Windows.
///
/// A string `starts_with` would accept `/projects/abc-evil` as inside
/// `/projects/abc`; comparing components cannot make that mistake. A
/// case-sensitive comparison on Windows would be a straightforward bypass.
fn is_within(root: &Path, candidate: &Path) -> bool {
    let root_components: Vec<_> = root.components().collect();
    let candidate_components: Vec<_> = candidate.components().collect();

    if candidate_components.len() < root_components.len() {
        return false;
    }

    root_components
        .iter()
        .zip(candidate_components.iter())
        .all(|(expected, actual)| components_match(expected, actual))
}

fn components_match(expected: &Component<'_>, actual: &Component<'_>) -> bool {
    let expected = expected.as_os_str().to_string_lossy();
    let actual = actual.as_os_str().to_string_lossy();
    if cfg!(windows) {
        expected.eq_ignore_ascii_case(&actual)
    } else {
        expected == actual
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(directory.path().join("src")).expect("create src");
        std::fs::write(directory.path().join("index.js"), "// hi").expect("write");
        std::fs::write(directory.path().join("src/app.js"), "// app").expect("write");
        directory
    }

    #[test]
    fn ordinary_paths_are_accepted() {
        let dir = project();
        for path in ["index.js", "src/app.js", "src", "./index.js"] {
            let safe = SafePath::new(dir.path(), path)
                .unwrap_or_else(|error| panic!("{path} should be accepted: {error}"));
            assert!(safe
                .absolute()
                .starts_with(dunce::canonicalize(dir.path()).expect("canonical")));
        }
    }

    #[test]
    fn a_path_that_does_not_exist_yet_is_accepted() {
        // Creating a file requires naming one that is not there.
        let dir = project();
        let safe = SafePath::new(dir.path(), "src/new-file.ts").expect("accept");
        assert_eq!(safe.relative(), "src/new-file.ts");
    }

    #[test]
    fn traversal_is_refused() {
        let dir = project();
        for path in [
            "../outside",
            "../../etc/passwd",
            "src/../../escape",
            "..",
            "a/b/../../../c",
            "src/../..",
        ] {
            assert_eq!(
                SafePath::new(dir.path(), path),
                Err(PathError::Traversal),
                "{path} should be refused"
            );
        }
    }

    #[test]
    fn backslash_traversal_is_refused() {
        // Windows separators must be normalised before the `..` check, or
        // `..\..\evil` would slip past a check that only splits on `/`.
        let dir = project();
        for path in ["..\\outside", "src\\..\\..\\escape"] {
            assert_eq!(SafePath::new(dir.path(), path), Err(PathError::Traversal));
        }
    }

    #[test]
    fn absolute_paths_are_refused() {
        let dir = project();
        for path in [
            "/etc/passwd",
            "C:\\Windows\\System32",
            "c:/windows",
            "//server/share",
            "\\\\server\\share",
        ] {
            assert_eq!(
                SafePath::new(dir.path(), path),
                Err(PathError::Absolute),
                "{path} should be refused"
            );
        }
    }

    #[test]
    fn nul_and_control_characters_are_refused() {
        let dir = project();
        for path in ["evil\0.txt", "line\nbreak.txt", "bell\u{7}.txt"] {
            assert_eq!(
                SafePath::new(dir.path(), path),
                Err(PathError::InvalidCharacter),
                "{path:?} should be refused"
            );
        }
    }

    #[test]
    fn windows_reserved_names_are_refused_on_every_platform() {
        let dir = project();
        for path in [
            "CON", "con.txt", "PRN.log", "aux", "NUL", "COM1", "lpt9.dat",
        ] {
            assert!(
                matches!(
                    SafePath::new(dir.path(), path),
                    Err(PathError::ReservedName(_))
                ),
                "{path} should be refused"
            );
        }
    }

    #[test]
    fn trailing_dots_and_spaces_are_refused() {
        // Windows strips them, making `evil.txt.` and `evil.txt` the same file.
        let dir = project();
        for path in ["evil.txt.", "evil.txt ", "folder./file"] {
            assert_eq!(
                SafePath::new(dir.path(), path),
                Err(PathError::TrailingDotOrSpace),
                "{path:?} should be refused"
            );
        }
    }

    #[test]
    fn alternate_data_streams_are_refused() {
        let dir = project();
        assert_eq!(
            SafePath::new(dir.path(), "file.txt:hidden"),
            Err(PathError::InvalidCharacter)
        );
    }

    #[test]
    fn empty_paths_are_refused() {
        let dir = project();
        assert_eq!(SafePath::new(dir.path(), ""), Err(PathError::Empty));
        assert_eq!(SafePath::new(dir.path(), "."), Err(PathError::Empty));
        assert_eq!(SafePath::new(dir.path(), "./"), Err(PathError::Empty));
    }

    #[test]
    fn absurd_paths_are_bounded() {
        let dir = project();
        assert_eq!(
            SafePath::new(dir.path(), &"a".repeat(MAX_PATH_LENGTH + 1)),
            Err(PathError::TooLong)
        );

        let deep = (0..MAX_DEPTH + 5)
            .map(|_| "a")
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(SafePath::new(dir.path(), &deep), Err(PathError::TooDeep));
    }

    #[test]
    fn a_sibling_directory_with_a_shared_prefix_is_not_inside() {
        // The bug a string `starts_with` would introduce: `/projects/abc-evil`
        // shares a prefix with `/projects/abc` but is a different directory.
        let parent = tempfile::tempdir().expect("temp dir");
        let inside = parent.path().join("abc");
        let sibling = parent.path().join("abc-evil");
        std::fs::create_dir_all(&inside).expect("create");
        std::fs::create_dir_all(&sibling).expect("create");

        let root = dunce::canonicalize(&inside).expect("canonical");
        let candidate = dunce::canonicalize(&sibling).expect("canonical");
        assert!(!is_within(&root, &candidate));
    }

    #[test]
    fn containment_accepts_the_root_itself_and_its_children() {
        let dir = project();
        let root = dunce::canonicalize(dir.path()).expect("canonical");
        assert!(is_within(&root, &root));
        assert!(is_within(&root, &root.join("src")));
        assert!(is_within(&root, &root.join("src").join("app.js")));
    }

    #[test]
    fn the_relative_form_uses_forward_slashes() {
        let dir = project();
        let safe = SafePath::new(dir.path(), "src\\app.js").expect("accept");
        assert_eq!(safe.relative(), "src/app.js");
    }

    /// Not following the last component must not become a way to skip the
    /// checks that apply to every other one. Runs on all platforms, because the
    /// escape it guards against does not need a symlink to attempt.
    #[test]
    fn not_following_the_last_component_still_validates_the_rest() {
        let dir = project();

        assert_eq!(
            SafePath::new_no_follow(dir.path(), "../escape"),
            Err(PathError::Traversal)
        );
        assert_eq!(
            SafePath::new_no_follow(dir.path(), "src/../../escape"),
            Err(PathError::Traversal)
        );
        assert_eq!(
            SafePath::new_no_follow(dir.path(), "/etc/passwd"),
            Err(PathError::Absolute)
        );
        assert_eq!(
            SafePath::new_no_follow(dir.path(), ""),
            Err(PathError::Empty)
        );

        // A name in a directory that is inside the project is fine, and the
        // relative form is unchanged from the following version.
        let safe = SafePath::new_no_follow(dir.path(), "src/app.js").expect("accept");
        assert_eq!(safe.relative(), "src/app.js");

        // Naming something that does not exist is fine: nothing is stat'd here.
        let absent = SafePath::new_no_follow(dir.path(), "src/nothing.txt").expect("accept");
        assert_eq!(absent.relative(), "src/nothing.txt");
    }

    #[test]
    fn joining_revalidates_from_the_root() {
        let dir = project();
        let src = SafePath::new(dir.path(), "src").expect("accept");
        assert_eq!(
            src.join(dir.path(), "app.js").expect("join").relative(),
            "src/app.js"
        );
        // Joining cannot be used to climb out.
        assert_eq!(
            src.join(dir.path(), "../../escape"),
            Err(PathError::Traversal)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_outside_is_caught() {
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret.txt"), "secret").expect("write");

        let dir = project();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .expect("symlink");

        // Canonicalisation resolves the link, and the result is outside.
        assert_eq!(
            SafePath::new(dir.path(), "link.txt"),
            Err(PathError::Escape)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_pointing_outside_is_caught() {
        let outside = tempfile::tempdir().expect("outside");
        std::fs::create_dir_all(outside.path().join("etc")).expect("create");

        let dir = project();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).expect("symlink");

        assert_eq!(
            SafePath::new(dir.path(), "escape/etc"),
            Err(PathError::Escape)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_created_after_validation_is_caught_by_the_toctou_recheck() {
        // The whole reason `verify_still_within` exists: validation approves a
        // name, and the name can become a link before it is opened.
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret.txt"), "secret").expect("write");

        let dir = project();
        let safe = SafePath::new(dir.path(), "later.txt").expect("accept");
        assert!(safe.verify_still_within(dir.path()).is_ok());

        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("later.txt"),
        )
        .expect("symlink");

        assert_eq!(
            safe.verify_still_within(dir.path()),
            Err(PathError::Escape),
            "a path that became a link must fail the recheck"
        );
    }

    #[test]
    fn the_root_helper_resolves_to_the_root() {
        let dir = project();
        let root = SafePath::root(dir.path()).expect("root");
        assert_eq!(root.relative(), "");
        assert_eq!(
            root.absolute(),
            dunce::canonicalize(dir.path()).expect("canonical")
        );
    }
}
