//! What can be installed, and what each thing requires.
//!
//! Requirements are declared beside the artefact rather than embedded in the
//! selection logic, so that adding an artefact is a data change and the
//! property test in [`crate::select`] covers it automatically.
//!
//! Minimum builds are taken from the vendor's published requirements. Where a
//! value is a floor for a *backend* rather than for the product, the comment
//! says which.

use project_host_platform::{Architecture, PackageManager};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerProduct {
    DockerDesktop,
    DockerEngine,
}

impl DockerProduct {
    pub fn display_name(self) -> &'static str {
        match self {
            DockerProduct::DockerDesktop => "Docker Desktop",
            DockerProduct::DockerEngine => "Docker Engine",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Windows,
    Linux,
}

/// What a machine must be for an artefact to be offered to it.
#[derive(Debug, Clone)]
pub struct Requirements {
    pub arch: &'static [Architecture],
    /// Windows build floor. `None` on Linux, where the kernel floor is old
    /// enough that every distribution this can detect clears it.
    pub min_os_build: Option<u32>,
    /// Substrings, any of which the reported edition may contain.
    pub required_editions: Option<&'static [&'static str]>,
    pub requires_virtualization: bool,
    pub min_free_bytes: u64,
    pub package_managers: Option<&'static [PackageManager]>,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub id: &'static str,
    pub product: DockerProduct,
    pub host: HostKind,
    pub requirements: Requirements,
}

const GB: u64 = 1024 * 1024 * 1024;

/// x86_64 only: Docker publishes no Docker Desktop build for Windows on ARM,
/// which is why that machine is blocked rather than given the x64 installer.
static WINDOWS_ARCHES: &[Architecture] = &[Architecture::X86_64];
static LINUX_ARCHES: &[Architecture] = &[Architecture::X86_64, Architecture::Aarch64];

static LINUX_MANAGERS: &[PackageManager] = &[
    PackageManager::Apt,
    PackageManager::Dnf,
    PackageManager::Pacman,
    PackageManager::Zypper,
];

static CATALOG: &[Artifact] = &[
    Artifact {
        id: "docker-desktop-windows-x86_64",
        product: DockerProduct::DockerDesktop,
        host: HostKind::Windows,
        requirements: Requirements {
            arch: WINDOWS_ARCHES,
            // The WSL2 backend's floor. Below this, Docker Desktop's supported
            // configurations do not include this machine.
            min_os_build: Some(19045),
            required_editions: None,
            requires_virtualization: true,
            min_free_bytes: 20 * GB,
            package_managers: None,
        },
    },
    Artifact {
        id: "docker-engine-linux",
        product: DockerProduct::DockerEngine,
        host: HostKind::Linux,
        requirements: Requirements {
            arch: LINUX_ARCHES,
            min_os_build: None,
            required_editions: None,
            // Docker Engine runs natively on the host kernel; there is no VM
            // and therefore no virtualization requirement.
            requires_virtualization: false,
            min_free_bytes: 10 * GB,
            package_managers: Some(LINUX_MANAGERS),
        },
    },
];

pub fn catalog() -> &'static [Artifact] {
    CATALOG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_artefact_has_a_distinct_id() {
        // The id is how a step machine and a log line refer to a choice; two
        // artefacts sharing one would make both unidentifiable.
        let mut ids: Vec<&str> = catalog().iter().map(|artifact| artifact.id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate artefact id");
    }

    #[test]
    fn no_artefact_requires_an_empty_set_of_architectures() {
        // An empty list would be satisfied by nothing and the artefact would be
        // silently unreachable.
        for artifact in catalog() {
            assert!(
                !artifact.requirements.arch.is_empty(),
                "{} lists no architecture",
                artifact.id
            );
        }
    }

    #[test]
    fn windows_on_arm_is_absent_from_the_windows_artefact() {
        // The reason the ARM machine is blocked rather than mis-served.
        let desktop = catalog()
            .iter()
            .find(|artifact| artifact.product == DockerProduct::DockerDesktop)
            .expect("the catalogue offers Docker Desktop");
        assert!(!desktop.requirements.arch.contains(&Architecture::Aarch64));
    }
}
