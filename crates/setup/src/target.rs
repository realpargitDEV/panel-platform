//! Choosing which artefact this machine needs.
//!
//! Every input is a parameter, including the platform and which Linux tools
//! exist. Nothing here probes the host, so every branch is reachable in a test
//! on any host — see the note in `lib.rs` about why that matters more here than
//! it usually would.

use crate::release::{Asset, Release};

/// The platforms an artefact is built for. Anything else is refused by name
/// rather than silently offered an installer that cannot run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    LinuxX64,
}

impl Platform {
    /// The platform this binary was compiled for, or `None` where nothing is
    /// built. Callers pass the result in, so this is the only host-dependent
    /// line in the module.
    pub fn detect() -> Option<Platform> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Some(Platform::WindowsX64),
            ("linux", "x86_64") => Some(Platform::LinuxX64),
            _ => None,
        }
    }

    pub fn describe() -> String {
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
    }
}

/// Which of the tools a `.deb` install needs are actually present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinuxTools {
    pub dpkg: bool,
    /// Installing a `.deb` needs root. `pkexec` is how that is asked for
    /// without the stub inventing its own privilege escalation.
    pub pkexec: bool,
}

impl LinuxTools {
    pub fn detect() -> LinuxTools {
        LinuxTools {
            dpkg: on_path("dpkg"),
            pkexec: on_path("pkexec"),
        }
    }

    fn can_install_deb(self) -> bool {
        self.dpkg && self.pkexec
    }
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// What the chosen asset is, which decides how it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The NSIS installer. Chosen over the `.msi` because it bootstraps
    /// WebView2 — the one dependency a Tauri application cannot supply itself.
    WindowsNsis,
    LinuxDeb,
    /// The answer wherever a `.deb` cannot be installed. A stub that demanded
    /// `dpkg` on Fedora would be choosing for its own convenience.
    LinuxAppImage,
}

impl Kind {
    pub fn describe(self) -> &'static str {
        match self {
            Kind::WindowsNsis => "Windows installer",
            Kind::LinuxDeb => "Debian package",
            Kind::LinuxAppImage => "AppImage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection<'a> {
    pub asset: &'a Asset,
    /// The detached minisign signature that must verify before the asset runs.
    pub signature: &'a Asset,
    pub kind: Kind,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectError {
    #[error("Panel Platform is not built for {0}. Windows x64 and Linux x64 are the platforms with installers.")]
    UnsupportedPlatform(String),
    #[error("release {release} has no {kind} to install")]
    NoAsset { release: String, kind: &'static str },
    #[error("release {release} has no signature for {asset}, so it cannot be verified")]
    NoSignature { release: String, asset: String },
}

/// Picks the artefact for a platform, and the signature that authenticates it.
///
/// An artefact with no signature is an error rather than an artefact installed
/// unverified. There is no path through this function that returns something
/// which cannot be checked.
pub fn select_asset<'a>(
    release: &'a Release,
    platform: Platform,
    tools: LinuxTools,
) -> Result<Selection<'a>, SelectError> {
    let kind = match platform {
        Platform::WindowsX64 => Kind::WindowsNsis,
        Platform::LinuxX64 if tools.can_install_deb() => Kind::LinuxDeb,
        Platform::LinuxX64 => Kind::LinuxAppImage,
    };

    let asset = release
        .assets
        .iter()
        .find(|asset| matches(&asset.name, kind))
        .ok_or_else(|| SelectError::NoAsset {
            release: release.version.clone(),
            kind: kind.describe(),
        })?;

    let signature_name = format!("{}.sig", asset.name);
    let signature = release
        .assets
        .iter()
        .find(|candidate| candidate.name == signature_name)
        .ok_or_else(|| SelectError::NoSignature {
            release: release.version.clone(),
            asset: asset.name.clone(),
        })?;

    Ok(Selection {
        asset,
        signature,
        kind,
    })
}

/// A `.sig` is never an installer, however well the rest of the name matches.
fn matches(name: &str, kind: Kind) -> bool {
    if name.ends_with(".sig") {
        return false;
    }
    match kind {
        Kind::WindowsNsis => name.ends_with("-setup.exe"),
        Kind::LinuxDeb => name.ends_with(".deb"),
        Kind::LinuxAppImage => name.ends_with(".AppImage"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_owned(),
            url: format!("https://github.com/x/y/releases/download/v0.1.0/{name}"),
            size: 10,
        }
    }

    /// Exactly the asset list the 0.1.0 release carries.
    fn release() -> Release {
        Release {
            version: "0.1.0".to_owned(),
            assets: vec![
                asset("latest.json"),
                asset("Panel.Platform_0.1.0_amd64.AppImage"),
                asset("Panel.Platform_0.1.0_amd64.AppImage.sig"),
                asset("Panel.Platform_0.1.0_amd64.deb"),
                asset("Panel.Platform_0.1.0_amd64.deb.sig"),
                asset("Panel.Platform_0.1.0_x64-setup.exe"),
                asset("Panel.Platform_0.1.0_x64-setup.exe.sig"),
                asset("Panel.Platform_0.1.0_x64_en-US.msi"),
                asset("Panel.Platform_0.1.0_x64_en-US.msi.sig"),
                asset("SHA256SUMS.txt"),
            ],
        }
    }

    const BOTH: LinuxTools = LinuxTools {
        dpkg: true,
        pkexec: true,
    };

    #[test]
    fn windows_gets_the_nsis_installer_not_the_msi() {
        let release = release();
        let chosen = select_asset(&release, Platform::WindowsX64, LinuxTools::default()).unwrap();

        assert_eq!(chosen.kind, Kind::WindowsNsis);
        assert_eq!(chosen.asset.name, "Panel.Platform_0.1.0_x64-setup.exe");
        assert_eq!(
            chosen.signature.name,
            "Panel.Platform_0.1.0_x64-setup.exe.sig"
        );
    }

    #[test]
    fn linux_with_both_tools_gets_the_deb() {
        let release = release();
        let chosen = select_asset(&release, Platform::LinuxX64, BOTH).unwrap();

        assert_eq!(chosen.kind, Kind::LinuxDeb);
        assert_eq!(chosen.asset.name, "Panel.Platform_0.1.0_amd64.deb");
    }

    /// Either tool missing means the `.deb` cannot be installed, so neither
    /// case may choose it.
    #[test]
    fn linux_missing_either_tool_gets_the_appimage() {
        let release = release();

        for tools in [
            LinuxTools {
                dpkg: true,
                pkexec: false,
            },
            LinuxTools {
                dpkg: false,
                pkexec: true,
            },
            LinuxTools::default(),
        ] {
            let chosen = select_asset(&release, Platform::LinuxX64, tools).unwrap();
            assert_eq!(chosen.kind, Kind::LinuxAppImage, "tools: {tools:?}");
            assert_eq!(chosen.asset.name, "Panel.Platform_0.1.0_amd64.AppImage");
        }
    }

    #[test]
    fn a_signature_is_never_offered_as_the_installer() {
        let release = Release {
            version: "0.1.0".to_owned(),
            assets: vec![asset("Panel.Platform_0.1.0_x64-setup.exe.sig")],
        };

        assert_eq!(
            select_asset(&release, Platform::WindowsX64, LinuxTools::default()),
            Err(SelectError::NoAsset {
                release: "0.1.0".to_owned(),
                kind: "Windows installer",
            })
        );
    }

    /// An unsigned artefact cannot be authenticated, so it is refused rather
    /// than installed on trust.
    #[test]
    fn an_asset_without_a_signature_is_refused() {
        let release = Release {
            version: "0.1.0".to_owned(),
            assets: vec![asset("Panel.Platform_0.1.0_x64-setup.exe")],
        };

        assert_eq!(
            select_asset(&release, Platform::WindowsX64, LinuxTools::default()),
            Err(SelectError::NoSignature {
                release: "0.1.0".to_owned(),
                asset: "Panel.Platform_0.1.0_x64-setup.exe".to_owned(),
            })
        );
    }

    #[test]
    fn every_selection_carries_a_signature() {
        let release = release();

        for (platform, tools) in [
            (Platform::WindowsX64, LinuxTools::default()),
            (Platform::LinuxX64, BOTH),
            (Platform::LinuxX64, LinuxTools::default()),
        ] {
            let chosen = select_asset(&release, platform, tools).unwrap();
            assert_eq!(chosen.signature.name, format!("{}.sig", chosen.asset.name));
        }
    }
}
