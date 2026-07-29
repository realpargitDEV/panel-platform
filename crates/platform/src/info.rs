//! What the running platform is, and what it can actually do.

use serde::{Deserialize, Serialize};

/// Reported to the client so the UI can hide controls that would do nothing
/// here, rather than showing one that silently fails.
///
/// See `docs/platform-support.md` §5. The rule the fields encode: enforce where
/// the platform allows it, measure and warn where it does not, and never claim
/// a limit is enforced when it is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub cpu_temperature: bool,
    pub per_container_disk_io: bool,
    pub storage_quota_enforcement: bool,
    pub linux_capability_dropping: bool,
    pub read_only_root_filesystem: bool,
    pub firewall_management: bool,
}

impl Capabilities {
    pub fn for_current_platform() -> Self {
        Self {
            // Windows rarely exposes a usable CPU temperature; Linux usually
            // does through hwmon.
            cpu_temperature: cfg!(unix),
            per_container_disk_io: cfg!(unix),
            // Needs cgroup or filesystem quota support. Accounted and warned
            // about elsewhere, never silently claimed.
            storage_quota_enforcement: cfg!(unix),
            linux_capability_dropping: cfg!(unix),
            read_only_root_filesystem: true,
            firewall_management: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub family: String,
    pub capabilities: Capabilities,
}

impl PlatformInfo {
    pub fn detect() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            family: std::env::consts::FAMILY.to_string(),
            capabilities: Capabilities::for_current_platform(),
        }
    }

    pub fn is_windows(&self) -> bool {
        self.family == "windows"
    }

    pub fn is_unix(&self) -> bool {
        self.family == "unix"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_exactly_one_family() {
        let info = PlatformInfo::detect();
        assert!(
            info.is_windows() ^ info.is_unix(),
            "got family {}",
            info.family
        );
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn read_only_root_is_available_on_both_platforms() {
        // Container hardening that must not silently degrade: if this ever
        // becomes platform-dependent, the Docker spec assertions need updating
        // at the same time.
        assert!(Capabilities::for_current_platform().read_only_root_filesystem);
    }

    #[test]
    fn capabilities_round_trip() {
        let capabilities = Capabilities::for_current_platform();
        let json = serde_json::to_string(&capabilities).expect("serialise");
        let back: Capabilities = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, capabilities);
    }
}
