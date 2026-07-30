//! Panel Platform's setup program.
//!
//! It finds the latest published release, downloads the one artefact this
//! machine can use, proves it came from Panel Platform, and hands it to the
//! installer that owns it.
//!
//! # Why nothing here is behind `#[cfg]`
//!
//! The platform and the tools available are parameters, not things any of these
//! functions look up. That is deliberate. This project shipped an application
//! that had never once started for a non-root Linux user, and it survived
//! because the only test that would have caught it could not run on the machine
//! the project is developed on. `select_asset` and `handoff::plan` are the two
//! places that could repeat that mistake, so both take their platform as an
//! argument and both are tested for every platform on every host.

pub mod download;
pub mod handoff;
pub mod net;
pub mod release;
pub mod target;
pub mod verify;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use target::{Kind, LinuxTools, Platform};

/// Where the program has got to, for anything that wants to show progress.
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    Checking,
    Downloading { done: u64, total: u64 },
    Verifying,
    Installing,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Release(#[from] release::ReleaseError),
    #[error(transparent)]
    Select(#[from] target::SelectError),
    #[error(transparent)]
    Download(#[from] download::DownloadError),
    #[error(transparent)]
    Verify(#[from] verify::VerifyError),
    #[error(transparent)]
    Handoff(#[from] handoff::HandoffError),
    #[error("Panel Platform is not built for {0}. Windows x64 and Linux x64 are the platforms with installers.")]
    UnsupportedPlatform(String),
    #[error("could not write the download to disk: {0}")]
    Io(String),
}

/// What the user is being asked to agree to, before anything is downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub asset: String,
    pub kind: Kind,
    pub bytes: u64,
}

impl Offer {
    /// Sizes people can read. A progress bar counting to 89115128 tells nobody
    /// anything.
    pub fn size(&self) -> String {
        human_size(self.bytes)
    }
}

pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

/// Everything the installer needs, resolved but not yet downloaded.
#[derive(Debug)]
pub struct Resolved {
    pub offer: Offer,
    pub artefact_url: String,
    pub signature_url: String,
    pub sums_url: String,
}

/// Step one: what is available, and what would this machine install?
pub fn resolve(agent: &ureq::Agent) -> Result<Resolved, Error> {
    let platform =
        Platform::detect().ok_or_else(|| Error::UnsupportedPlatform(Platform::describe()))?;
    let tools = if platform == Platform::LinuxX64 {
        LinuxTools::detect()
    } else {
        LinuxTools::default()
    };

    let release = release::fetch_latest(agent)?;
    let selection = target::select_asset(&release, platform, tools)?;

    let sums_url = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS.txt")
        .map(|asset| asset.url.clone())
        .ok_or_else(|| target::SelectError::NoAsset {
            release: release.version.clone(),
            kind: "checksum list",
        })?;

    Ok(Resolved {
        offer: Offer {
            version: release.version.clone(),
            asset: selection.asset.name.clone(),
            kind: selection.kind,
            bytes: selection.asset.size,
        },
        artefact_url: selection.asset.url.clone(),
        signature_url: selection.signature.url.clone(),
        sums_url,
    })
}

/// Step two: download, verify, and install. Returns once the installer has been
/// handed control.
///
/// `dry_run` stops after verification, having proved the whole path without
/// changing the machine. That is what CI runs.
pub fn install(
    agent: &ureq::Agent,
    resolved: &Resolved,
    dry_run: bool,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Stage),
) -> Result<(), Error> {
    report(Stage::Downloading {
        done: 0,
        total: resolved.offer.bytes,
    });

    let bytes = download::fetch(
        agent,
        &resolved.artefact_url,
        resolved.offer.bytes,
        cancel,
        &mut |done, total| report(Stage::Downloading { done, total }),
    )?;

    report(Stage::Verifying);
    let signature = download::fetch_text(agent, &resolved.signature_url)?;
    let sums = download::fetch_text(agent, &resolved.sums_url)?;

    // Nothing is written to disk until it has been proved authentic, so there
    // is never a moment where an unverified installer exists in a place a user
    // could run it by accident.
    verify::verify(
        &bytes,
        &resolved.offer.asset,
        &signature,
        &sums,
        verify::PUBKEY,
    )?;

    if dry_run {
        return Ok(());
    }

    report(Stage::Installing);
    let path = write_private(&resolved.offer.asset, &bytes)?;
    let home = home_directory();
    let plan = handoff::plan(resolved.offer.kind, &path, home.as_deref())?;

    handoff::execute(&plan, &path)?;
    Ok(())
}

/// Writes the verified artefact somewhere only this user can reach.
///
/// Not the shared temp directory: a world-writable path invites another process
/// to swap the file between the check and the run.
fn write_private(name: &str, bytes: &[u8]) -> Result<PathBuf, Error> {
    let dir = std::env::temp_dir().join(format!("panel-platform-setup-{}", std::process::id()));

    std::fs::create_dir_all(&dir).map_err(|error| Error::Io(error.to_string()))?;
    restrict(&dir)?;

    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|error| Error::Io(error.to_string()))?;

    Ok(path)
}

#[cfg(unix)]
fn restrict(dir: &std::path::Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| Error::Io(error.to_string()))
}

#[cfg(not(unix))]
fn restrict(_dir: &std::path::Path) -> Result<(), Error> {
    // The per-user temp directory is already restricted on Windows.
    Ok(())
}

fn home_directory() -> Option<PathBuf> {
    choose_home(
        std::env::var_os("HOME").as_deref(),
        std::env::var_os("USERPROFILE").as_deref(),
    )
}

/// The choice, separated from where the values came from so it can be tested
/// without setting environment variables — which is not a thing tests may do
/// safely when they share a process.
fn choose_home(
    home: Option<&std::ffi::OsStr>,
    user_profile: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    [home, user_profile]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        // A relative home would put the AppImage somewhere that depends on the
        // directory the program happened to be started from.
        .find(|path| path.is_absolute())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_rendered_for_people_not_for_machines() {
        assert_eq!(human_size(89_115_128), "85.0 MB");
        assert_eq!(human_size(9_622_711), "9.2 MB");
        assert_eq!(human_size(896), "1 KB");
    }

    fn os(value: &str) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(value)
    }

    #[test]
    fn home_is_preferred_over_userprofile() {
        let absolute = if cfg!(windows) { "C:\\home" } else { "/home/a" };
        let other = if cfg!(windows) {
            "C:\\profile"
        } else {
            "/home/b"
        };

        assert_eq!(
            choose_home(Some(os(absolute)), Some(os(other))),
            Some(PathBuf::from(absolute))
        );
    }

    /// A relative home would put the AppImage somewhere that depends on the
    /// directory the program happened to be started from, so it is skipped and
    /// the next candidate is tried.
    #[test]
    fn a_relative_home_is_skipped_in_favour_of_an_absolute_one() {
        let absolute = if cfg!(windows) {
            "C:\\profile"
        } else {
            "/home/b"
        };

        assert_eq!(
            choose_home(Some(os("relative/path")), Some(os(absolute))),
            Some(PathBuf::from(absolute))
        );
    }

    #[test]
    fn no_absolute_candidate_means_no_home() {
        assert_eq!(choose_home(Some(os("relative")), None), None);
        assert_eq!(choose_home(None, None), None);
    }

    #[test]
    fn an_offer_describes_itself_in_readable_units() {
        let offer = Offer {
            version: "0.1.0".to_owned(),
            asset: "Panel.Platform_0.1.0_amd64.AppImage".to_owned(),
            kind: Kind::LinuxAppImage,
            bytes: 89_115_128,
        };

        assert_eq!(offer.size(), "85.0 MB");
        assert_eq!(offer.kind.describe(), "AppImage");
    }
}
