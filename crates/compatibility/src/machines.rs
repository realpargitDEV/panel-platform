//! Synthetic machines.
//!
//! Every decision in this crate is tested against these rather than against the
//! machine running the tests. They are `pub` because the tier and selection
//! tests in sibling modules use them, and because a new artefact added to the
//! catalogue must be checked against the whole set.

use project_host_platform::{
    Architecture, CpuInfo, LinuxInfo, MemoryInfo, OsInfo, PackageManager, StorageKind,
    SystemSnapshot, VirtualizationInfo, VolumeInfo, WindowsInfo,
};

pub const GB: u64 = 1024 * 1024 * 1024;

fn volume(mount: &str, total_gb: u64, free_gb: u64) -> VolumeInfo {
    VolumeInfo {
        mount_point: mount.to_string(),
        total_bytes: total_gb * GB,
        free_bytes: free_gb * GB,
        removable: false,
        kind: StorageKind::Ssd,
    }
}

fn windows_base() -> SystemSnapshot {
    SystemSnapshot {
        arch: Architecture::X86_64,
        os: OsInfo {
            name: Some("Windows".to_string()),
            edition: Some("Microsoft Windows 11 Pro".to_string()),
            version: Some("11 (26200)".to_string()),
            build: Some(26200),
            kernel: Some("26200".to_string()),
        },
        virtualization: VirtualizationInfo {
            supported: Some(true),
            enabled: Some(true),
            hypervisor_present: Some(false),
        },
        windows: Some(WindowsInfo {
            wsl_present: true,
            wsl_version: Some(2),
            default_distro: Some("Ubuntu".to_string()),
        }),
        ..SystemSnapshot::unknown()
    }
}

/// 16 logical cores, 32 GB, plenty of disk.
pub fn windows_11_workstation() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("GenuineIntel".to_string()),
            model: Some("Intel(R) Core(TM) i9-13900K".to_string()),
            physical_cores: Some(8),
            logical_cores: Some(16),
        },
        memory: MemoryInfo {
            total_bytes: Some(32 * GB),
            available_bytes: Some(20 * GB),
        },
        volumes: vec![volume("C:\\", 2000, 1200)],
        ..windows_base()
    }
}

/// A 2015-era laptop: 2 logical cores, 4 GB.
pub fn windows_11_low_end() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("GenuineIntel".to_string()),
            model: Some("Intel(R) Core(TM) i3-5005U".to_string()),
            physical_cores: Some(2),
            logical_cores: Some(2),
        },
        memory: MemoryInfo {
            total_bytes: Some(4 * GB),
            available_bytes: Some(GB),
        },
        volumes: vec![VolumeInfo {
            kind: StorageKind::Hdd,
            ..volume("C:\\", 250, 60)
        }],
        ..windows_base()
    }
}

/// Exactly on both Standard boundaries: 4 logical cores and 8 GB.
///
/// The set needs a machine that lands on `Standard`, and a boundary is the
/// right place to put it — the thresholds are `>=`, so this machine proves the
/// comparison is not off by one in either direction.
pub fn windows_11_midrange() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("GenuineIntel".to_string()),
            model: Some("Intel(R) Core(TM) i5-8250U".to_string()),
            physical_cores: Some(4),
            logical_cores: Some(4),
        },
        memory: MemoryInfo {
            total_bytes: Some(8 * GB),
            available_bytes: Some(3 * GB),
        },
        volumes: vec![volume("C:\\", 500, 120)],
        ..windows_base()
    }
}

/// Capable in every respect, on an architecture with no Docker Desktop build.
pub fn windows_on_arm() -> SystemSnapshot {
    SystemSnapshot {
        arch: Architecture::Aarch64,
        ..windows_11_workstation()
    }
}

/// Supported by the CPU, switched off in firmware. The reboot-into-BIOS case.
pub fn windows_virtualization_disabled() -> SystemSnapshot {
    SystemSnapshot {
        virtualization: VirtualizationInfo {
            supported: Some(true),
            enabled: Some(false),
            hypervisor_present: Some(false),
        },
        ..windows_11_workstation()
    }
}

/// Capable, but with 4 GB free.
pub fn windows_full_disk() -> SystemSnapshot {
    SystemSnapshot {
        volumes: vec![volume("C:\\", 500, 4)],
        ..windows_11_workstation()
    }
}

/// 8 logical cores, 16 GB, apt.
pub fn ubuntu_desktop() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("AuthenticAMD".to_string()),
            model: Some("AMD Ryzen 7 5800X".to_string()),
            physical_cores: Some(8),
            logical_cores: Some(8),
        },
        memory: MemoryInfo {
            total_bytes: Some(16 * GB),
            available_bytes: Some(9 * GB),
        },
        volumes: vec![volume("/", 1000, 400)],
        arch: Architecture::X86_64,
        os: OsInfo {
            name: Some("Ubuntu".to_string()),
            version: Some("24.04".to_string()),
            kernel: Some("6.8.0-40-generic".to_string()),
            ..OsInfo::default()
        },
        virtualization: VirtualizationInfo {
            supported: Some(true),
            enabled: Some(true),
            hypervisor_present: Some(false),
        },
        linux: Some(LinuxInfo {
            distro_id: Some("ubuntu".to_string()),
            version_id: Some("24.04".to_string()),
            package_manager: Some(PackageManager::Apt),
        }),
        ..SystemSnapshot::unknown()
    }
}

/// Answers no probe at all. Every decision must still produce a defensible
/// result rather than a panic or an optimistic guess.
pub fn knows_nothing() -> SystemSnapshot {
    SystemSnapshot::unknown()
}

/// Every machine, by name. New artefacts are checked against all of them.
pub fn golden_set() -> Vec<(&'static str, SystemSnapshot)> {
    vec![
        ("windows-11-workstation", windows_11_workstation()),
        ("windows-11-midrange", windows_11_midrange()),
        ("windows-11-low-end", windows_11_low_end()),
        ("windows-on-arm", windows_on_arm()),
        (
            "windows-virtualization-disabled",
            windows_virtualization_disabled(),
        ),
        ("windows-full-disk", windows_full_disk()),
        ("ubuntu-desktop", ubuntu_desktop()),
        ("knows-nothing", knows_nothing()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_golden_set_covers_every_case_the_design_names() {
        let names: Vec<&str> = golden_set().iter().map(|(name, _)| *name).collect();
        for expected in [
            "windows-11-workstation",
            "windows-11-midrange",
            "windows-11-low-end",
            "windows-on-arm",
            "windows-virtualization-disabled",
            "windows-full-disk",
            "ubuntu-desktop",
            "knows-nothing",
        ] {
            assert!(names.contains(&expected), "{expected} is missing");
        }
    }

    #[test]
    fn the_low_end_machine_is_actually_low_end() {
        // If this drifts upward the tier tests stop testing what they claim to.
        let machine = windows_11_low_end();
        assert!(machine.cpu.logical_cores.unwrap() < 4);
        assert!(machine.memory.total_bytes.unwrap() < 8 * GB);
    }

    #[test]
    fn the_workstation_is_actually_capable() {
        let machine = windows_11_workstation();
        assert!(machine.cpu.logical_cores.unwrap() > 8);
        assert!(machine.memory.total_bytes.unwrap() > 16 * GB);
        assert!(machine.largest_fixed_free_bytes().unwrap() > 20 * GB);
    }

    #[test]
    fn the_disabled_machine_supports_virtualization_but_has_it_off() {
        // The distinction the whole firmware-blocker path depends on.
        let machine = windows_virtualization_disabled();
        assert_eq!(machine.virtualization.supported, Some(true));
        assert_eq!(machine.virtualization.enabled, Some(false));
    }
}
