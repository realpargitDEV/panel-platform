//! Why this machine cannot have the Docker it would otherwise get.
//!
//! Every variant names concrete values. "Virtualization is disabled" without
//! the key to press is the failure mode this design exists to replace.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Blocker {
    #[error(
        "Virtualization is supported by this CPU but switched off in firmware. \
         Restart, open firmware setup (usually F2, F10, Del or Esc during \
         startup — the key is shown on the first screen), and enable \
         {setting}. No application can change this setting."
    )]
    VirtualizationDisabled { setting: String },

    #[error(
        "This CPU does not support hardware virtualization, which Docker \
         requires. There is no setting that changes this."
    )]
    VirtualizationUnsupported,

    #[error("No Docker build is published for {found} processors.")]
    ArchitectureUnsupported { found: String },

    #[error(
        "{product} needs {name} build {required} or newer; this machine \
         reports build {found}."
    )]
    OsTooOld {
        product: String,
        name: String,
        required: u32,
        found: u32,
    },

    #[error(
        "Could not determine the operating system build, which decides which \
         Docker backend applies. Nothing is installed on a guess."
    )]
    OsBuildUnknown,

    #[error("{found} does not provide {needed}.")]
    EditionUnsupported { found: String, needed: String },

    #[error(
        "{needed_gb} GB of free space is required; the roomiest fixed volume \
         has {found_gb} GB."
    )]
    InsufficientDisk { needed_gb: u64, found_gb: u64 },

    #[error(
        "No supported package manager was found. Docker Engine is installed \
         through apt, dnf, pacman or zypper."
    )]
    NoPackageManager,

    #[error(
        "Could not determine which operating system this is, so no installer \
         can be matched to it."
    )]
    HostUnrecognised,
}

impl Blocker {
    /// Whether the user can do something about this on this machine.
    ///
    /// Drives whether the interface offers a retry or explains that there is
    /// nothing to retry. Firmware is fixable; a CPU is not.
    pub fn is_fixable(&self) -> bool {
        match self {
            Blocker::VirtualizationDisabled { .. }
            | Blocker::InsufficientDisk { .. }
            | Blocker::OsTooOld { .. }
            | Blocker::NoPackageManager => true,
            Blocker::VirtualizationUnsupported
            | Blocker::ArchitectureUnsupported { .. }
            | Blocker::EditionUnsupported { .. }
            | Blocker::OsBuildUnknown
            | Blocker::HostUnrecognised => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_firmware_blocker_names_the_setting_and_how_to_reach_it() {
        // The distinction between this and VirtualizationUnsupported is the
        // difference between a fixable machine and an unfixable one.
        let message = Blocker::VirtualizationDisabled {
            setting: "Intel VT-x".to_string(),
        }
        .to_string();
        assert!(message.contains("Intel VT-x"));
        assert!(message.contains("firmware"));
        assert!(
            message.contains("F2"),
            "naming a key is the difference between advice and a complaint"
        );
    }

    #[test]
    fn a_disk_blocker_names_both_numbers() {
        let message = Blocker::InsufficientDisk {
            needed_gb: 20,
            found_gb: 4,
        }
        .to_string();
        assert!(message.contains("20"));
        assert!(message.contains('4'));
    }

    #[test]
    fn an_os_blocker_names_the_build_found_and_the_build_required() {
        let message = Blocker::OsTooOld {
            product: "Docker Desktop".to_string(),
            name: "Windows".to_string(),
            required: 19045,
            found: 17763,
        }
        .to_string();
        assert!(message.contains("19045"));
        assert!(message.contains("17763"));
    }

    #[test]
    fn a_missing_cpu_feature_is_not_offered_as_something_to_retry() {
        // Offering a retry for a CPU that lacks the instruction set would send
        // the user round a loop that cannot terminate.
        assert!(!Blocker::VirtualizationUnsupported.is_fixable());
        assert!(Blocker::VirtualizationDisabled {
            setting: "AMD SVM".to_string()
        }
        .is_fixable());
    }

    #[test]
    fn a_blocker_round_trips_through_json() {
        let blocker = Blocker::InsufficientDisk {
            needed_gb: 20,
            found_gb: 4,
        };
        let json = serde_json::to_string(&blocker).expect("serialise");
        let back: Blocker = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, blocker);
    }
}
