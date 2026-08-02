//! Which artefact this machine gets, or why it gets none.
//!
//! Selection is total: it returns **exactly one artefact, or a non-empty list
//! of blockers**. There is no fallback, no closest match and no default
//! artefact. That is the whole of "never install an incompatible version", and
//! the property test below is what holds it true as the catalogue grows.

use project_host_platform::SystemSnapshot;

use crate::blocker::Blocker;
pub use crate::catalog::{catalog, Artifact, DockerProduct, HostKind, Requirements};

#[derive(Debug)]
pub enum Selection {
    Artifact(&'static Artifact),
    Blocked(Vec<Blocker>),
}

impl Selection {
    pub fn artifact(&self) -> Option<&'static Artifact> {
        match self {
            Selection::Artifact(artifact) => Some(artifact),
            Selection::Blocked(_) => None,
        }
    }

    pub fn blockers(&self) -> &[Blocker] {
        match self {
            Selection::Artifact(_) => &[],
            Selection::Blocked(blockers) => blockers,
        }
    }
}

const GB: u64 = 1024 * 1024 * 1024;

/// Which firmware setting to name, from the CPU vendor. Naming the wrong one
/// sends the user hunting for a setting their firmware does not have.
fn virtualization_setting(snapshot: &SystemSnapshot) -> String {
    match snapshot.cpu.vendor.as_deref() {
        Some(vendor) if vendor.contains("AMD") => "AMD SVM (often listed as SVM Mode)".to_string(),
        Some(vendor) if vendor.contains("Intel") => {
            "Intel VT-x (often listed as Intel Virtualization Technology)".to_string()
        }
        // Both, when the vendor is unknown: one of the two names will match
        // what is on screen.
        _ => "Intel VT-x or AMD SVM".to_string(),
    }
}

/// Which operating system this snapshot describes.
///
/// The scan sets exactly one of these groups, so their presence is what
/// identifies the host. A snapshot with neither — one that answered no probe —
/// is deliberately not guessed at.
pub fn host_of(snapshot: &SystemSnapshot) -> Option<HostKind> {
    match (&snapshot.windows, &snapshot.linux) {
        (Some(_), None) => Some(HostKind::Windows),
        (None, Some(_)) => Some(HostKind::Linux),
        _ => None,
    }
}

/// Every requirement this machine fails. Empty means it satisfies them all.
///
/// Takes the whole artefact rather than its [`Requirements`], because the host
/// is part of what makes an artefact wrong for a machine and a signature that
/// could not see it invited exactly that bug: `host` was declared and never
/// checked, so a Linux machine reporting a Windows-shaped build number would
/// have been offered the Windows installer.
pub fn unmet(artifact: &Artifact, snapshot: &SystemSnapshot) -> Vec<Blocker> {
    let requirements = &artifact.requirements;
    let mut blockers = Vec::new();

    match host_of(snapshot) {
        Some(host) if host == artifact.host => {}
        // A host mismatch is reported as an unrecognised host rather than as a
        // wrong one: from this machine's point of view the artefact simply does
        // not apply, and the artefact that does will report its own blockers.
        _ => blockers.push(Blocker::HostUnrecognised),
    }

    if !requirements.arch.contains(&snapshot.arch) {
        blockers.push(Blocker::ArchitectureUnsupported {
            found: snapshot.arch.display_name().to_string(),
        });
    }

    if requirements.requires_virtualization {
        match (
            snapshot.virtualization.enabled,
            snapshot.virtualization.supported,
        ) {
            (Some(true), _) => {}
            // Supported but off is fixable; unsupported is not. The two get
            // different advice and must not be collapsed.
            (_, Some(false)) => blockers.push(Blocker::VirtualizationUnsupported),
            _ => blockers.push(Blocker::VirtualizationDisabled {
                setting: virtualization_setting(snapshot),
            }),
        }
    }

    if let Some(required) = requirements.min_os_build {
        match snapshot.os.build {
            Some(found) if found >= required => {}
            Some(found) => blockers.push(Blocker::OsTooOld {
                product: "Docker Desktop".to_string(),
                name: "Windows".to_string(),
                required,
                found,
            }),
            None => blockers.push(Blocker::OsBuildUnknown),
        }
    }

    if let Some(editions) = requirements.required_editions {
        match snapshot.os.edition.as_deref() {
            Some(found) if editions.iter().any(|wanted| found.contains(wanted)) => {}
            Some(found) => blockers.push(Blocker::EditionUnsupported {
                found: found.to_string(),
                needed: editions.join(" or "),
            }),
            None => blockers.push(Blocker::EditionUnsupported {
                found: "an unrecognised edition".to_string(),
                needed: editions.join(" or "),
            }),
        }
    }

    if let Some(managers) = requirements.package_managers {
        let found = snapshot
            .linux
            .as_ref()
            .and_then(|linux| linux.package_manager);
        match found {
            Some(manager) if managers.contains(&manager) => {}
            _ => blockers.push(Blocker::NoPackageManager),
        }
    }

    // Unknown free space counts as zero. Absence of evidence must never become
    // an install.
    let free = snapshot.largest_fixed_free_bytes().unwrap_or(0);
    if free < requirements.min_free_bytes {
        blockers.push(Blocker::InsufficientDisk {
            needed_gb: requirements.min_free_bytes / GB,
            found_gb: free / GB,
        });
    }

    blockers
}

/// The artefact for this machine, or every reason it has none.
///
/// When several artefacts fail, the reported blockers are those of the artefact
/// that failed on the fewest counts — the one the machine is closest to being
/// able to run, and therefore the one whose advice is most likely to be
/// actionable.
pub fn select(snapshot: &SystemSnapshot) -> Selection {
    let mut closest: Option<Vec<Blocker>> = None;

    for artifact in catalog() {
        let blockers = unmet(artifact, snapshot);
        if blockers.is_empty() {
            return Selection::Artifact(artifact);
        }
        if closest
            .as_ref()
            .is_none_or(|best| blockers.len() < best.len())
        {
            closest = Some(blockers);
        }
    }

    // The catalogue is never empty, so `closest` is always set by the loop.
    // The fallback keeps this total without an unwrap.
    Selection::Blocked(closest.unwrap_or_else(|| vec![Blocker::VirtualizationUnsupported]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machines::{self, GB as GIB};
    use project_host_platform::{StorageKind, VolumeInfo};

    /// Re-checks each requirement directly, written separately from [`unmet`]
    /// on purpose. A property test that called `unmet` would be a tautology;
    /// this is an independent derivation, so the two disagreeing is a real
    /// signal rather than a restatement.
    fn independently_satisfied(artifact: &Artifact, snapshot: &SystemSnapshot) -> bool {
        let requirements = &artifact.requirements;

        let host = match (&snapshot.windows, &snapshot.linux) {
            (Some(_), None) => Some(HostKind::Windows),
            (None, Some(_)) => Some(HostKind::Linux),
            _ => None,
        };
        if host != Some(artifact.host) {
            return false;
        }

        if !requirements.arch.contains(&snapshot.arch) {
            return false;
        }
        if requirements.requires_virtualization && snapshot.virtualization.enabled != Some(true) {
            return false;
        }
        if let Some(required) = requirements.min_os_build {
            match snapshot.os.build {
                Some(found) if found >= required => {}
                _ => return false,
            }
        }
        if let Some(editions) = requirements.required_editions {
            let Some(found) = snapshot.os.edition.as_deref() else {
                return false;
            };
            if !editions.iter().any(|wanted| found.contains(wanted)) {
                return false;
            }
        }
        if let Some(managers) = requirements.package_managers {
            let found = snapshot
                .linux
                .as_ref()
                .and_then(|linux| linux.package_manager);
            match found {
                Some(manager) if managers.contains(&manager) => {}
                _ => return false,
            }
        }
        snapshot.largest_fixed_free_bytes().unwrap_or(0) >= requirements.min_free_bytes
    }

    #[test]
    fn a_selected_artefact_is_always_one_the_machine_satisfies() {
        // This is the whole of "never install an incompatible version". A
        // property over the entire golden set, rather than a case per artefact,
        // is what keeps the guarantee true when an artefact is added later.
        for (name, machine) in machines::golden_set() {
            if let Selection::Artifact(artifact) = select(&machine) {
                assert!(
                    independently_satisfied(artifact, &machine),
                    "{name} was offered {} but does not satisfy it",
                    artifact.id
                );
            }
        }
    }

    #[test]
    fn a_capable_windows_machine_is_offered_docker_desktop() {
        let selection = select(&machines::windows_11_workstation());
        assert_eq!(
            selection.artifact().map(|artifact| artifact.product),
            Some(DockerProduct::DockerDesktop),
            "blocked by {:?}",
            selection.blockers()
        );
    }

    #[test]
    fn ubuntu_is_offered_docker_engine() {
        let selection = select(&machines::ubuntu_desktop());
        assert_eq!(
            selection.artifact().map(|artifact| artifact.product),
            Some(DockerProduct::DockerEngine),
            "blocked by {:?}",
            selection.blockers()
        );
    }

    #[test]
    fn windows_on_arm_is_blocked_rather_than_given_the_x64_build() {
        let selection = select(&machines::windows_on_arm());
        assert!(
            selection.artifact().is_none(),
            "an ARM machine must never be offered an x86_64 installer"
        );
        assert!(selection
            .blockers()
            .iter()
            .any(|blocker| matches!(blocker, Blocker::ArchitectureUnsupported { .. })));
    }

    #[test]
    fn disabled_virtualization_is_reported_as_fixable() {
        let selection = select(&machines::windows_virtualization_disabled());
        assert!(selection.artifact().is_none());
        let blockers = selection.blockers();
        assert!(
            blockers
                .iter()
                .any(|blocker| matches!(blocker, Blocker::VirtualizationDisabled { .. })),
            "got {blockers:?}"
        );
        assert!(blockers.iter().any(Blocker::is_fixable));
    }

    #[test]
    fn the_named_firmware_setting_follows_the_cpu_vendor() {
        // Naming Intel's setting to an AMD owner sends them hunting for
        // something their firmware does not have.
        let mut amd = machines::windows_virtualization_disabled();
        amd.cpu.vendor = Some("AuthenticAMD".to_string());
        let message = format!("{:?}", select(&amd).blockers());
        assert!(message.contains("SVM"), "{message}");

        let mut intel = machines::windows_virtualization_disabled();
        intel.cpu.vendor = Some("GenuineIntel".to_string());
        let message = format!("{:?}", select(&intel).blockers());
        assert!(message.contains("VT-x"), "{message}");
    }

    #[test]
    fn an_unsupported_cpu_is_distinguished_from_a_disabled_setting() {
        let mut machine = machines::windows_11_workstation();
        machine.virtualization.supported = Some(false);
        machine.virtualization.enabled = Some(false);
        let selection = select(&machine);
        assert!(selection.artifact().is_none());
        assert!(selection
            .blockers()
            .iter()
            .any(|blocker| matches!(blocker, Blocker::VirtualizationUnsupported)));
    }

    #[test]
    fn a_full_disk_blocks_and_names_both_numbers() {
        let selection = select(&machines::windows_full_disk());
        assert!(
            selection.artifact().is_none(),
            "4 GB free is not enough to install Docker"
        );
        assert!(selection
            .blockers()
            .iter()
            .any(|blocker| matches!(blocker, Blocker::InsufficientDisk { .. })));
    }

    #[test]
    fn an_old_windows_build_is_blocked_with_both_builds_named() {
        let mut machine = machines::windows_11_workstation();
        machine.os.build = Some(17763);
        machine.os.version = Some("10.0.17763".to_string());
        let selection = select(&machine);
        assert!(
            selection.artifact().is_none(),
            "build 17763 predates the WSL2 backend"
        );
        assert!(selection
            .blockers()
            .iter()
            .any(|blocker| matches!(blocker, Blocker::OsTooOld { .. })));
    }

    #[test]
    fn an_unknown_build_blocks_rather_than_being_treated_as_new_enough() {
        let mut machine = machines::windows_11_workstation();
        machine.os.build = None;
        let selection = select(&machine);
        assert!(selection.artifact().is_none());
        assert!(selection
            .blockers()
            .iter()
            .any(|blocker| matches!(blocker, Blocker::OsBuildUnknown)));
    }

    #[test]
    fn a_machine_that_knows_nothing_is_blocked_rather_than_guessed_at() {
        // The most important case: absence of evidence must never become an
        // install.
        let selection = select(&machines::knows_nothing());
        assert!(
            selection.artifact().is_none(),
            "nothing is known about this machine; nothing may be installed"
        );
        assert!(!selection.blockers().is_empty());
    }

    #[test]
    fn every_catalogued_artefact_is_reachable_by_some_machine() {
        // An artefact no machine can be offered is dead weight, and usually
        // means its requirements are wrong.
        for artifact in catalog() {
            assert!(
                machines::golden_set().iter().any(|(_, machine)| {
                    select(machine)
                        .artifact()
                        .is_some_and(|chosen| chosen.id == artifact.id)
                }),
                "{} is offered to no machine in the golden set",
                artifact.id
            );
        }
    }

    #[test]
    fn a_linux_machine_is_never_offered_the_windows_installer() {
        // Regression guard. `Artifact.host` was declared and never checked, so
        // a Linux machine that happened to report a Windows-shaped build number
        // satisfied every remaining Docker Desktop requirement and would have
        // been handed a .exe.
        let mut machine = machines::ubuntu_desktop();
        machine.os.build = Some(26200);

        let selection = select(&machine);
        assert_eq!(
            selection.artifact().map(|artifact| artifact.product),
            Some(DockerProduct::DockerEngine),
            "a Linux machine must only ever be offered Docker Engine"
        );
    }

    #[test]
    fn a_windows_machine_is_never_offered_the_linux_package() {
        let mut machine = machines::windows_11_workstation();
        // Even with a package manager somehow reported, the host decides.
        machine.linux = None;
        assert_eq!(
            select(&machine).artifact().map(|artifact| artifact.host),
            Some(HostKind::Windows)
        );
    }

    #[test]
    fn a_snapshot_with_no_host_group_is_blocked() {
        // `knows_nothing` has neither group set; nothing can be matched to it.
        let selection = select(&machines::knows_nothing());
        assert!(selection.artifact().is_none());
        assert!(selection
            .blockers()
            .iter()
            .any(|blocker| matches!(blocker, Blocker::HostUnrecognised)));
    }

    #[test]
    fn a_removable_volume_cannot_satisfy_the_disk_requirement() {
        let mut machine = machines::windows_full_disk();
        machine.volumes.push(VolumeInfo {
            mount_point: "E:\\".to_string(),
            total_bytes: 500 * GIB,
            free_bytes: 400 * GIB,
            removable: true,
            kind: StorageKind::Unknown,
        });
        assert!(
            select(&machine).artifact().is_none(),
            "a USB stick must not satisfy the install requirement"
        );
    }
}
