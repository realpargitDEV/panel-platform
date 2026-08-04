//! What this machine is.
//!
//! A plain value, produced by [`crate::probe`] and consumed by
//! `project-host-compatibility`. Splitting the data from the reading is what
//! lets every compatibility decision be tested against a machine that was
//! constructed rather than one that happens to be running the test.
//!
//! **Every field is optional, and the scan has no failure case.** A probe that
//! cannot answer yields `None`. A machine that will not report its GPU still
//! gets Docker installed, and a scan that could fail would be a scan that
//! blocks setup on something irrelevant to it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CpuInfo {
    pub vendor: Option<String>,
    /// Reported, never used to decide anything. See the tier rules in
    /// `project-host-compatibility`: release year is a weak predictor of
    /// throughput, and tiering on it would misjudge the low-end machines that
    /// most need correct limits.
    pub model: Option<String>,
    pub physical_cores: Option<u32>,
    pub logical_cores: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoryInfo {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Removable volumes are excluded from every capacity decision: a USB stick
    /// with 400 GB free must not make a machine look roomy.
    pub removable: bool,
    pub kind: StorageKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
    Aarch64,
    Other(String),
}

impl Architecture {
    /// From a Rust target arch string, e.g. `std::env::consts::ARCH`.
    pub fn from_target(arch: &str) -> Self {
        match arch {
            "x86_64" => Architecture::X86_64,
            "aarch64" => Architecture::Aarch64,
            other => Architecture::Other(other.to_string()),
        }
    }

    /// The name to show a user. `Other` keeps its own name, because reporting
    /// "other" without saying which is a bug report nobody can act on.
    pub fn display_name(&self) -> &str {
        match self {
            Architecture::X86_64 => "x86_64",
            Architecture::Aarch64 => "aarch64",
            Architecture::Other(name) => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OsInfo {
    /// e.g. "Windows", "Ubuntu"
    pub name: Option<String>,
    /// e.g. "Windows 11 Pro". Decides whether a given backend is available.
    pub edition: Option<String>,
    /// e.g. "10.0.26200"
    pub version: Option<String>,
    /// The number that decides whether the WSL2 backend exists on a given
    /// Windows install. A selection made without it is a guess.
    pub build: Option<u32>,
    pub kernel: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VirtualizationInfo {
    /// The CPU has VT-x or SVM.
    pub supported: Option<bool>,
    /// It is switched on in firmware. No API can change this; only a reboot
    /// into firmware setup can.
    pub enabled: Option<bool>,
    /// Something is already running as a hypervisor — Hyper-V, WSL2, or this
    /// machine is itself a guest.
    pub hypervisor_present: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WindowsInfo {
    pub wsl_present: bool,
    pub wsl_version: Option<u32>,
    pub default_distro: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
}

impl PackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageManager::Apt => "apt",
            PackageManager::Dnf => "dnf",
            PackageManager::Pacman => "pacman",
            PackageManager::Zypper => "zypper",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LinuxInfo {
    pub distro_id: Option<String>,
    pub version_id: Option<String>,
    pub package_manager: Option<PackageManager>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: Option<String>,
    pub model: Option<String>,
}

/// Everything the scan learned about this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub volumes: Vec<VolumeInfo>,
    pub arch: Architecture,
    pub os: OsInfo,
    pub virtualization: VirtualizationInfo,
    /// Present only on Windows.
    pub windows: Option<WindowsInfo>,
    /// Present only on Linux.
    pub linux: Option<LinuxInfo>,
    /// Reported, never used to decide anything: no container here requests GPU
    /// access. Collected because a setup failure reported without it is a
    /// failure nobody can diagnose remotely.
    pub gpus: Vec<GpuInfo>,
}

impl SystemSnapshot {
    /// A snapshot that knows nothing. The starting point for the real scan, and
    /// the base for constructing test machines.
    pub fn unknown() -> Self {
        Self {
            cpu: CpuInfo::default(),
            memory: MemoryInfo::default(),
            volumes: Vec::new(),
            arch: Architecture::Other("unknown".to_string()),
            os: OsInfo::default(),
            virtualization: VirtualizationInfo::default(),
            windows: None,
            linux: None,
            gpus: Vec::new(),
        }
    }

    /// Free bytes on the roomiest fixed volume.
    ///
    /// Docker's images and containers land on one volume, and which one is a
    /// setting we do not read. The roomiest fixed volume is the right question
    /// for tiering: if any of them has space, disk is not what constrains this
    /// machine. Removable volumes are excluded.
    pub fn largest_fixed_free_bytes(&self) -> Option<u64> {
        self.volumes
            .iter()
            .filter(|volume| !volume.removable)
            .map(|volume| volume.free_bytes)
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_snapshot_claims_nothing() {
        // The scan cannot fail. A machine that answers no probe at all must
        // still produce a valid snapshot, because every decision downstream is
        // a function of this value.
        let snapshot = SystemSnapshot::unknown();
        assert_eq!(snapshot.cpu.logical_cores, None);
        assert_eq!(snapshot.memory.total_bytes, None);
        assert!(snapshot.volumes.is_empty());
        assert_eq!(snapshot.virtualization.enabled, None);
        assert!(snapshot.gpus.is_empty());
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let mut snapshot = SystemSnapshot::unknown();
        snapshot.cpu.logical_cores = Some(8);
        snapshot.memory.total_bytes = Some(16 * 1024 * 1024 * 1024);
        snapshot.arch = Architecture::X86_64;
        snapshot.os.build = Some(26200);
        snapshot.volumes.push(VolumeInfo {
            mount_point: "C:\\".to_string(),
            total_bytes: 500_000_000_000,
            free_bytes: 200_000_000_000,
            removable: false,
            kind: StorageKind::Ssd,
        });

        let json = serde_json::to_string(&snapshot).expect("serialise");
        let back: SystemSnapshot = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn an_unrecognised_architecture_keeps_its_name() {
        // Reporting "other" without saying which is a bug report nobody can act
        // on.
        let arch = Architecture::from_target("riscv64");
        assert_eq!(arch, Architecture::Other("riscv64".to_string()));
        assert_eq!(arch.display_name(), "riscv64");
        assert_eq!(Architecture::from_target("x86_64"), Architecture::X86_64);
        assert_eq!(Architecture::from_target("aarch64"), Architecture::Aarch64);
    }
}
