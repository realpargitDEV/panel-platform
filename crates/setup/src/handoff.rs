//! Handing the verified file to whatever actually installs it.
//!
//! On Windows and for the `.deb` this program adds no install logic of its own.
//! Those installers exist, and CI smoke-tests them on every tag; a second
//! implementation here would be a second thing to get wrong and a second thing
//! to keep in step. The AppImage is the exception, because no packager owns it.
//!
//! What to run is decided by a pure function and tested without spawning
//! anything. Only `execute` touches the machine.

use std::path::{Path, PathBuf};

use crate::target::Kind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawn {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Start an installer and let its own interface take over.
    Handoff(Spawn),
    /// Place the AppImage where the user can run it, and give them a menu entry
    /// so they are not expected to remember a path.
    PlaceAppImage {
        destination: PathBuf,
        desktop_entry: PathBuf,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandoffError {
    #[error("this installer could not find your home directory")]
    NoHome,
    #[error("the installer could not be started: {0}")]
    NotStarted(String),
    #[error("installing needs your permission, and the request was dismissed")]
    NotAuthorised,
    #[error("the installer exited with code {0}")]
    Failed(i32),
    #[error("could not write {path}: {reason}")]
    Io { path: String, reason: String },
}

/// Decides what to run. Pure: every argument is passed in, nothing is read from
/// the environment, so all three platforms' plans are checked on any host.
pub fn plan(kind: Kind, artefact: &Path, home: Option<&Path>) -> Result<Plan, HandoffError> {
    let artefact = artefact.to_string_lossy().into_owned();

    match kind {
        Kind::WindowsNsis => Ok(Plan::Handoff(Spawn {
            program: artefact,
            args: Vec::new(),
        })),

        // `pkexec` rather than `sudo`: it prompts through the desktop, which is
        // where this program's user already is, and it does not require a
        // terminal that a double-clicked binary does not have.
        Kind::LinuxDeb => Ok(Plan::Handoff(Spawn {
            program: "pkexec".to_owned(),
            args: vec!["dpkg".to_owned(), "-i".to_owned(), artefact],
        })),

        Kind::LinuxAppImage => {
            let home = home.ok_or(HandoffError::NoHome)?;
            Ok(Plan::PlaceAppImage {
                // `~/.local/bin` is on the default PATH on every distribution
                // that follows the XDG layout, which is the same layout the
                // application's own data directories now use.
                destination: home.join(".local/bin/panel-platform.AppImage"),
                desktop_entry: home.join(".local/share/applications/panel-platform.desktop"),
            })
        }
    }
}

pub fn desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Panel Platform\n\
         Exec={}\n\
         Icon=panel-platform\n\
         Categories=Development;\n\
         Terminal=false\n",
        executable.display()
    )
}

/// Carries out a plan. The only function here that touches the machine.
pub fn execute(plan: &Plan, artefact: &Path) -> Result<(), HandoffError> {
    match plan {
        Plan::Handoff(spawn) => {
            let status = std::process::Command::new(&spawn.program)
                .args(&spawn.args)
                .status()
                .map_err(|error| HandoffError::NotStarted(error.to_string()))?;

            match status.code() {
                Some(0) | None => Ok(()),
                // pkexec's own codes for "dismissed" and "not available". Told
                // apart from a failed install because the answer is different:
                // one is try again, the other is something is wrong.
                Some(126) | Some(127) => Err(HandoffError::NotAuthorised),
                Some(code) => Err(HandoffError::Failed(code)),
            }
        }

        Plan::PlaceAppImage {
            destination,
            desktop_entry: entry_path,
        } => {
            place_appimage(artefact, destination)?;
            write_file(entry_path, desktop_entry(destination).as_bytes())
        }
    }
}

fn place_appimage(artefact: &Path, destination: &Path) -> Result<(), HandoffError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| HandoffError::Io {
            path: parent.display().to_string(),
            reason: error.to_string(),
        })?;
    }

    std::fs::copy(artefact, destination).map_err(|error| HandoffError::Io {
        path: destination.display().to_string(),
        reason: error.to_string(),
    })?;

    make_executable(destination)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), HandoffError> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|error| {
        HandoffError::Io {
            path: path.display().to_string(),
            reason: error.to_string(),
        }
    })
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), HandoffError> {
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), HandoffError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| HandoffError::Io {
            path: parent.display().to_string(),
            reason: error.to_string(),
        })?;
    }

    std::fs::write(path, contents).map_err(|error| HandoffError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/someone")
    }

    #[test]
    fn windows_runs_the_installer_itself_with_no_arguments() {
        let plan = plan(Kind::WindowsNsis, Path::new("C:\\tmp\\setup.exe"), None).unwrap();

        assert_eq!(
            plan,
            Plan::Handoff(Spawn {
                program: "C:\\tmp\\setup.exe".to_owned(),
                args: Vec::new(),
            })
        );
    }

    /// Installing a `.deb` needs root, and this is the only place that is
    /// asked for. A plan that shells out to `sudo`, or writes outside the
    /// user's home without asking, is a regression.
    #[test]
    fn the_deb_is_installed_through_pkexec() {
        let plan = plan(Kind::LinuxDeb, Path::new("/tmp/panel.deb"), Some(&home())).unwrap();

        assert_eq!(
            plan,
            Plan::Handoff(Spawn {
                program: "pkexec".to_owned(),
                args: vec![
                    "dpkg".to_owned(),
                    "-i".to_owned(),
                    "/tmp/panel.deb".to_owned()
                ],
            })
        );
    }

    #[test]
    fn the_appimage_goes_under_the_users_home_and_never_needs_root() {
        let plan = plan(
            Kind::LinuxAppImage,
            Path::new("/tmp/p.AppImage"),
            Some(&home()),
        )
        .unwrap();

        let Plan::PlaceAppImage {
            destination,
            desktop_entry,
        } = plan
        else {
            panic!("the AppImage must not be handed to another installer");
        };

        assert!(destination.starts_with(home()), "{destination:?}");
        assert!(desktop_entry.starts_with(home()), "{desktop_entry:?}");
        assert!(destination.ends_with("panel-platform.AppImage"));
        assert!(desktop_entry.ends_with("panel-platform.desktop"));
    }

    #[test]
    fn the_appimage_cannot_be_placed_without_a_home() {
        assert_eq!(
            plan(Kind::LinuxAppImage, Path::new("/tmp/p.AppImage"), None),
            Err(HandoffError::NoHome)
        );
    }

    #[test]
    fn the_desktop_entry_points_at_the_installed_path() {
        let entry = desktop_entry(Path::new(
            "/home/someone/.local/bin/panel-platform.AppImage",
        ));

        assert!(entry.starts_with("[Desktop Entry]"));
        assert!(entry.contains("Exec=/home/someone/.local/bin/panel-platform.AppImage"));
        assert!(entry.contains("Name=Panel Platform"));
    }

    /// A dismissed password prompt is not a failed install, and telling them
    /// apart is the difference between "try again" and "something is wrong".
    #[test]
    fn a_dismissed_prompt_reads_differently_from_a_failure() {
        assert_ne!(
            HandoffError::NotAuthorised.to_string(),
            HandoffError::Failed(1).to_string()
        );
        assert!(HandoffError::NotAuthorised
            .to_string()
            .contains("permission"));
    }
}
