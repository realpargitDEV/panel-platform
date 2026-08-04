//! Finding an executable that was installed after this process started.
//!
//! A process inherits `PATH` at launch and never sees a change to it. So the
//! moment after winget installs Node, a probe using the inherited `PATH` finds
//! nothing — and reporting that as a failed install sends the user to reinstall
//! software they already have.
//!
//! The fix is to rebuild the search path from where the installer actually
//! wrote it, which on Windows is the registry. Both halves here are pure: the
//! registry values and the existence check are arguments, so the whole path is
//! tested on a machine where none of these directories exist.

use std::path::{Path, PathBuf};

/// Rebuild the search path from the machine and user `PATH` values, falling
/// back to what this process inherited.
///
/// Order matches how Windows composes it — machine, then user — with anything
/// the process holds that the registry does not appended rather than dropped,
/// because a directory injected into this process is still a real place to look.
/// `windows` decides the separator rather than `std::env::split_paths`, so a
/// Windows path list is parsed correctly while running the tests on Linux, and
/// the reverse. This is the same rule `setup::handoff::plan` follows.
pub fn merged_path(
    machine: Option<&str>,
    user: Option<&str>,
    process: &str,
    windows: bool,
) -> Vec<PathBuf> {
    let separator = if windows { ';' } else { ':' };
    let mut directories: Vec<PathBuf> = Vec::new();

    for source in [
        machine.unwrap_or_default(),
        user.unwrap_or_default(),
        process,
    ] {
        for segment in source.split(separator) {
            let trimmed = segment.trim();
            // An empty segment means the current working directory, which is
            // never a place an executable should be resolved from.
            if trimmed.is_empty() {
                continue;
            }

            let path = PathBuf::from(trimmed);
            if !directories.contains(&path) {
                directories.push(path);
            }
        }
    }

    directories
}

/// Look for `name` in `directories`, trying each executable suffix in turn.
///
/// `exists` is injected so the tests decide what the filesystem appears to
/// hold. A test whose result changes when someone installs Node is not a test.
pub fn find_executable(
    directories: &[PathBuf],
    name: &str,
    suffixes: &[&str],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    directories.iter().find_map(|directory| {
        suffixes
            .iter()
            .map(|suffix| directory.join(format!("{name}{suffix}")))
            .find(|candidate| exists(candidate))
    })
}

/// The suffixes an executable can carry on this platform.
///
/// On Windows this is not decoration: `node` is `node.exe` but `npm` is
/// `npm.cmd`, and a search for a bare name finds neither.
pub fn suffixes_for(windows: bool) -> &'static [&'static str] {
    if windows {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<PathBuf> {
        values.iter().map(PathBuf::from).collect()
    }

    /// The whole point: winget wrote the directory into the machine `PATH`
    /// after this process started, so the inherited copy does not have it.
    #[test]
    fn a_directory_added_after_launch_is_found_in_the_registry_value() {
        let merged = merged_path(
            Some("C:\\Windows;C:\\Program Files\\nodejs"),
            None,
            "C:\\Windows",
            true,
        );

        assert!(merged.contains(&PathBuf::from("C:\\Program Files\\nodejs")));
    }

    #[test]
    fn the_machine_path_is_searched_before_the_user_path() {
        let merged = merged_path(Some("C:\\machine"), Some("C:\\user"), "", true);

        assert_eq!(merged, paths(&["C:\\machine", "C:\\user"]));
    }

    /// A directory this process holds but the registry does not is still a real
    /// place to look, so it is appended rather than dropped.
    #[test]
    fn a_directory_only_the_process_knows_about_is_kept() {
        let merged = merged_path(Some("C:\\machine"), None, "C:\\machine;C:\\injected", true);

        assert_eq!(merged, paths(&["C:\\machine", "C:\\injected"]));
    }

    #[test]
    fn a_directory_listed_twice_is_searched_once() {
        let merged = merged_path(Some("C:\\a;C:\\a"), Some("C:\\a"), "C:\\a", true);

        assert_eq!(merged, paths(&["C:\\a"]));
    }

    /// A trailing separator produces an empty segment, which as a search
    /// directory means the current working directory — somewhere an executable
    /// must never be resolved from.
    #[test]
    fn empty_segments_are_discarded() {
        let merged = merged_path(Some("C:\\a;;"), None, "", true);

        assert_eq!(merged, paths(&["C:\\a"]));
    }

    /// With no registry values at all, the inherited path is still better than
    /// nothing.
    #[test]
    fn the_process_path_is_used_when_the_registry_says_nothing() {
        let merged = merged_path(None, None, "/usr/bin:/usr/local/bin", false);

        assert_eq!(merged, paths(&["/usr/bin", "/usr/local/bin"]));
    }

    /// Directories are joined rather than concatenated, and the expected path
    /// is built the same way the code builds it. A literal `"a\\b"` would be
    /// one component on Linux and two on Windows, which is how a test like
    /// this passes on the machine it was written on and fails in CI.
    #[test]
    fn an_executable_is_found_in_the_first_directory_that_has_it() {
        let directories = paths(&["first", "second"]);
        let present = directories[1].join("node.exe");
        let exists = |path: &Path| path == present;

        let found = find_executable(&directories, "node", suffixes_for(true), &exists);

        assert_eq!(found, Some(present));
    }

    /// `npm` is `npm.cmd`, not `npm.exe`. A search that only tried `.exe` would
    /// report a working Node installation as broken.
    #[test]
    fn a_windows_executable_is_found_under_any_of_its_suffixes() {
        let directories = paths(&["nodejs"]);
        let present = directories[0].join("npm.cmd");
        let exists = |path: &Path| path == present;

        let found = find_executable(&directories, "npm", suffixes_for(true), &exists);

        assert_eq!(found, Some(present));
    }

    #[test]
    fn a_missing_executable_is_reported_as_missing() {
        let exists = |_: &Path| false;

        assert_eq!(
            find_executable(&paths(&["nodejs"]), "node", suffixes_for(true), &exists),
            None
        );
    }

    /// On Linux the name is the name; appending `.exe` would be nonsense.
    #[test]
    fn a_unix_executable_is_looked_up_under_its_bare_name() {
        let present = PathBuf::from("/usr/bin/python3");
        let exists = |path: &Path| path == present;

        let found = find_executable(
            &paths(&["/usr/bin"]),
            "python3",
            suffixes_for(false),
            &exists,
        );

        assert_eq!(found, Some(present));
    }
}
