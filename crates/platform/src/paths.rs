//! Where the agent keeps things, per operating system.
//!
//! Every path in the system comes from here. Nothing concatenates strings, and
//! nothing outside this crate contains a `#[cfg(windows)]` — that is what keeps
//! `project-manager`, `backup-manager` and the API layer free of per-OS
//! branching.
//!
//! Only the path adapter is implemented in Phase 2, because the configuration
//! system needs it. The remaining adapters (services, Docker discovery,
//! keychain, firewall, notifications, metrics) arrive in Phase 3.

use std::path::{Path, PathBuf};

use crate::error::PlatformError;

/// Resolves every directory the agent uses.
pub trait PathProvider: Send + Sync + std::fmt::Debug {
    /// Database and agent state.
    fn data_dir(&self) -> &Path;
    /// `agent.toml`, TLS certificate and key, bootstrap file.
    fn config_dir(&self) -> &Path;
    fn log_dir(&self) -> &Path;
    fn projects_dir(&self) -> &Path;
    fn backups_dir(&self) -> &Path;

    /// Staging area for extraction and restore.
    ///
    /// Deliberately on the same filesystem as [`Self::projects_dir`]: both
    /// operations stage here and then rename into place, and a cross-device
    /// rename is not atomic. The partial-write protection depends on that.
    fn temp_dir(&self) -> &Path;

    /// The database file.
    fn database_path(&self) -> PathBuf {
        self.data_dir().join("project-host.db")
    }

    /// A project's directory, named from its generated identifier — never from
    /// anything the user typed.
    fn project_dir(&self, project_id: &str) -> PathBuf {
        self.projects_dir().join(project_id)
    }

    /// Create every directory. Idempotent: an installer and the agent both run
    /// it, and running it twice must succeed.
    fn ensure_all(&self) -> Result<(), PlatformError> {
        for directory in [
            self.data_dir(),
            self.config_dir(),
            self.log_dir(),
            self.projects_dir(),
            self.backups_dir(),
            self.temp_dir(),
        ] {
            std::fs::create_dir_all(directory).map_err(|source| PlatformError::Directory {
                path: directory.to_path_buf(),
                source,
            })?;
        }
        self.apply_permissions()
    }

    /// Tighten permissions on the directories that hold secrets. Called after
    /// creation and re-asserted on every agent start, so a permissive parent
    /// or a manual `chmod` does not leave the database world-readable.
    fn apply_permissions(&self) -> Result<(), PlatformError>;
}

/// Standard layout rooted at one directory. Used for the real platforms and,
/// with a temporary root, for tests.
#[derive(Debug, Clone)]
pub struct StandardPaths {
    data: PathBuf,
    config: PathBuf,
    logs: PathBuf,
    projects: PathBuf,
    backups: PathBuf,
    temp: PathBuf,
}

impl StandardPaths {
    /// Windows: everything under `ProgramData`.
    ///
    /// `ProgramData` rather than a per-user directory precisely because the
    /// service runs with no user logged in — a per-user path would be
    /// unreadable in exactly the state the agent normally runs in.
    pub fn windows(program_data: &Path) -> Self {
        let root = program_data.join("ProjectHost");
        Self {
            data: root.join("data"),
            config: root.join("config"),
            logs: root.join("logs"),
            projects: root.join("projects"),
            backups: root.join("backups"),
            temp: root.join("tmp"),
        }
    }

    /// Linux: per-user XDG locations.
    ///
    /// These were the FHS system directories — `/var/lib`, `/etc`, `/var/log` —
    /// from the design where a background service owned the data and ran as its
    /// own service user. That service was deleted in the single-process
    /// rewrite. The application now runs as the person using it, and a person
    /// cannot create anything under `/var` or `/etc`.
    ///
    /// Nothing caught it until an installed `.deb` was launched in CI as an
    /// ordinary user and died in under a second with
    /// `could not create /var/lib/project-host` — the first time the Linux
    /// build had ever been started.
    ///
    /// `projects`, `backups` and `tmp` stay under one root, because staging an
    /// extraction and renaming it into place is only atomic within a single
    /// filesystem.
    pub fn linux() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);

        let base = |variable: &str, fallback: &str| -> PathBuf {
            std::env::var_os(variable)
                .map(PathBuf::from)
                // The specification says a relative `XDG_*` value must be
                // ignored. Honouring one would put the database wherever the
                // process happened to be started from.
                .filter(|path| path.is_absolute())
                .or_else(|| home.as_ref().map(|home| home.join(fallback)))
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("project-host")
        };

        let state = base("XDG_DATA_HOME", ".local/share");
        Self {
            config: base("XDG_CONFIG_HOME", ".config"),
            logs: base("XDG_STATE_HOME", ".local/state"),
            projects: state.join("projects"),
            backups: state.join("backups"),
            temp: state.join("tmp"),
            data: state,
        }
    }

    /// Everything under one directory. For development and tests.
    pub fn rooted(root: &Path) -> Self {
        Self {
            data: root.join("data"),
            config: root.join("config"),
            logs: root.join("logs"),
            projects: root.join("projects"),
            backups: root.join("backups"),
            temp: root.join("tmp"),
        }
    }
}

impl PathProvider for StandardPaths {
    fn data_dir(&self) -> &Path {
        &self.data
    }
    fn config_dir(&self) -> &Path {
        &self.config
    }
    fn log_dir(&self) -> &Path {
        &self.logs
    }
    fn projects_dir(&self) -> &Path {
        &self.projects
    }
    fn backups_dir(&self) -> &Path {
        &self.backups
    }
    fn temp_dir(&self) -> &Path {
        &self.temp
    }

    #[cfg(unix)]
    fn apply_permissions(&self) -> Result<(), PlatformError> {
        use std::os::unix::fs::PermissionsExt;

        // 0750 for data the service user owns; 0700 for the staging area,
        // which briefly holds partially extracted archives.
        for (directory, mode) in [
            (&self.data, 0o750),
            (&self.config, 0o750),
            (&self.logs, 0o750),
            (&self.projects, 0o750),
            (&self.backups, 0o750),
            (&self.temp, 0o700),
        ] {
            if !directory.exists() {
                continue;
            }
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(mode)).map_err(
                |source| PlatformError::Permissions {
                    path: directory.clone(),
                    source,
                },
            )?;
        }
        Ok(())
    }

    #[cfg(windows)]
    fn apply_permissions(&self) -> Result<(), PlatformError> {
        // Windows ACLs are applied by the installer, which runs elevated and
        // can disable inheritance (see docs/platform-support.md §2.1). The
        // agent verifies rather than sets them; that verification needs the
        // Windows security APIs and lands with the rest of the Windows adapter
        // in Phase 3. Doing nothing here is correct — silently "succeeding" at
        // a permission change that did not happen would be worse.
        Ok(())
    }
}

/// Resolve the layout for the running platform.
pub fn platform_paths() -> Result<StandardPaths, PlatformError> {
    #[cfg(windows)]
    {
        let program_data = std::env::var_os("ProgramData").map(PathBuf::from).ok_or(
            PlatformError::MissingEnvironment {
                name: "ProgramData",
            },
        )?;
        Ok(StandardPaths::windows(&program_data))
    }
    #[cfg(unix)]
    {
        Ok(StandardPaths::linux())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_layout_sits_under_program_data() {
        let paths = StandardPaths::windows(Path::new("C:\\ProgramData"));
        // `Path::ends_with` matches whole components, and a literal
        // `"ProjectHost\\data"` is two of them only where `\` separates. On
        // Linux it is one component with a backslash in its name, which never
        // matches — so the expected suffix is built with `join` and picks up
        // whatever separator the platform running the test uses.
        assert!(paths
            .data_dir()
            .ends_with(Path::new("ProjectHost").join("data")));
        assert!(paths
            .projects_dir()
            .ends_with(Path::new("ProjectHost").join("projects")));
        assert_eq!(
            paths.database_path().file_name().and_then(|n| n.to_str()),
            Some("project-host.db")
        );
    }

    /// The exact directories depend on the environment this runs in, so the
    /// invariants are asserted instead of literal paths — which also lets this
    /// run on every platform rather than only on Linux. A `#[cfg(unix)]` test
    /// is one that never runs on the machine this is developed on, and the
    /// `/var` layout survived precisely because nothing exercised it.
    #[test]
    fn linux_layout_is_per_user_and_shares_one_root() {
        let paths = StandardPaths::linux();

        for directory in [paths.data_dir(), paths.config_dir(), paths.log_dir()] {
            assert!(
                directory.ends_with("project-host"),
                "{directory:?} should be namespaced"
            );
            // The service that owned these is gone. The application runs as the
            // user, who cannot create anything here — which is exactly how the
            // installed `.deb` failed to start.
            assert!(
                !directory.starts_with("/var") && !directory.starts_with("/etc"),
                "{directory:?} is a system location the user cannot write to"
            );
        }

        // Staging and renaming into place is only atomic within one
        // filesystem, so these three must share a parent.
        assert_eq!(paths.projects_dir().parent(), Some(paths.data_dir()));
        assert_eq!(paths.backups_dir().parent(), Some(paths.data_dir()));
        assert_eq!(paths.temp_dir().parent(), Some(paths.data_dir()));
    }

    #[test]
    fn temp_shares_a_filesystem_with_projects() {
        // Both live under the same root so a rename from temp into a project
        // directory stays atomic. If this ever stops holding, extraction and
        // restore lose their partial-write protection.
        for paths in [
            StandardPaths::linux(),
            StandardPaths::windows(Path::new("C:\\PD")),
        ] {
            let projects_root = paths.projects_dir().parent().map(Path::to_path_buf);
            let temp_root = paths.temp_dir().parent().map(Path::to_path_buf);
            assert_eq!(projects_root, temp_root);
        }
    }

    #[test]
    fn a_project_directory_is_named_from_its_identifier() {
        let paths = StandardPaths::linux();
        let directory = paths.project_dir("prj_0193000000007000a000000000000001");
        assert!(directory.starts_with(paths.projects_dir()));
        assert!(directory.ends_with("prj_0193000000007000a000000000000001"));
    }

    #[test]
    fn ensure_all_is_idempotent() {
        let root = tempfile::tempdir().expect("temp dir");
        let paths = StandardPaths::rooted(root.path());

        paths.ensure_all().expect("first run");
        paths.ensure_all().expect("second run must also succeed");

        for directory in [
            paths.data_dir(),
            paths.config_dir(),
            paths.log_dir(),
            paths.projects_dir(),
            paths.backups_dir(),
            paths.temp_dir(),
        ] {
            assert!(
                directory.is_dir(),
                "{} was not created",
                directory.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_are_tightened() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let paths = StandardPaths::rooted(root.path());
        paths.ensure_all().expect("ensure_all");

        let data_mode = std::fs::metadata(paths.data_dir())
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(data_mode, 0o750, "data dir must not be world-readable");

        let temp_mode = std::fs::metadata(paths.temp_dir())
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(temp_mode, 0o700);
    }
}
