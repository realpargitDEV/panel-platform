//! Which operating system this is, and precisely which version.

use crate::snapshot::{LinuxInfo, OsInfo, PackageManager};

/// The third dotted component of a Windows version string.
///
/// Returns `None` rather than a default. A wrong build number selects a wrong
/// Docker backend, so "unknown" must stay distinguishable from "old".
pub fn parse_build(version: &str) -> Option<u32> {
    version.split('.').nth(2)?.trim().parse().ok()
}

pub fn package_manager_for(distro_id: &str) -> Option<PackageManager> {
    match distro_id.trim().to_ascii_lowercase().as_str() {
        "ubuntu" | "debian" | "linuxmint" | "pop" | "raspbian" => Some(PackageManager::Apt),
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" => Some(PackageManager::Dnf),
        "arch" | "manjaro" | "endeavouros" => Some(PackageManager::Pacman),
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" => {
            Some(PackageManager::Zypper)
        }
        // Guessing a manager for an unrecognised distro would produce a command
        // that fails in a way the user cannot interpret.
        _ => None,
    }
}

/// Parse `/etc/os-release`, whose values may or may not be quoted.
pub fn parse_os_release(contents: &str) -> LinuxInfo {
    let value_of = |wanted: &str| -> Option<String> {
        contents.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == wanted)
                .then(|| value.trim().trim_matches('"').to_string())
                .filter(|value| !value.is_empty())
        })
    };

    let distro_id = value_of("ID");
    LinuxInfo {
        package_manager: distro_id.as_deref().and_then(package_manager_for),
        version_id: value_of("VERSION_ID"),
        distro_id,
    }
}

pub fn read_os(
    system_name: Option<String>,
    kernel: Option<String>,
    version: Option<String>,
) -> OsInfo {
    OsInfo {
        build: version.as_deref().and_then(parse_build),
        name: system_name,
        // Edition needs a platform-specific read and is filled in by
        // `probe::platform_specific`. On Linux there is no such concept.
        edition: None,
        version,
        kernel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_version_yields_its_build_number() {
        // The number that decides whether the WSL2 backend exists.
        assert_eq!(parse_build("10.0.26200"), Some(26200));
        assert_eq!(parse_build("10.0.19045"), Some(19045));
    }

    #[test]
    fn an_unparseable_version_yields_no_build_rather_than_a_guess() {
        // A wrong build number selects a wrong Docker backend, which is the
        // exact failure this whole design exists to prevent.
        assert_eq!(parse_build(""), None);
        assert_eq!(parse_build("10.0"), None);
        assert_eq!(parse_build("Windows"), None);
        assert_eq!(parse_build("10.0.notanumber"), None);
    }

    #[test]
    fn os_release_is_parsed_into_a_distro_and_manager() {
        let contents = "\
NAME=\"Ubuntu\"
VERSION_ID=\"24.04\"
ID=ubuntu
PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"
";
        let info = parse_os_release(contents);
        assert_eq!(info.distro_id.as_deref(), Some("ubuntu"));
        assert_eq!(info.version_id.as_deref(), Some("24.04"));
        assert_eq!(info.package_manager, Some(PackageManager::Apt));
    }

    #[test]
    fn os_release_quotes_are_stripped_and_unknown_keys_ignored() {
        let info = parse_os_release("ID=\"fedora\"\nVERSION_ID=41\nUNRELATED=x\n");
        assert_eq!(info.distro_id.as_deref(), Some("fedora"));
        assert_eq!(info.version_id.as_deref(), Some("41"));
        assert_eq!(info.package_manager, Some(PackageManager::Dnf));
    }

    #[test]
    fn an_unknown_distro_gets_no_package_manager() {
        // Guessing apt for an unknown distro would produce a command that fails
        // in a way the user cannot interpret.
        assert_eq!(package_manager_for("plan9"), None);
        assert_eq!(parse_os_release("").distro_id, None);
    }

    #[test]
    fn known_distros_map_to_their_managers() {
        assert_eq!(package_manager_for("debian"), Some(PackageManager::Apt));
        assert_eq!(package_manager_for("rhel"), Some(PackageManager::Dnf));
        assert_eq!(package_manager_for("arch"), Some(PackageManager::Pacman));
        assert_eq!(
            package_manager_for("opensuse"),
            Some(PackageManager::Zypper)
        );
    }

    #[test]
    fn a_version_string_with_no_build_still_yields_the_rest() {
        // The scan never fails; an unreadable build leaves the other fields
        // intact rather than discarding the whole group.
        let info = read_os(
            Some("Ubuntu".to_string()),
            Some("6.8.0-40-generic".to_string()),
            Some("24.04".to_string()),
        );
        assert_eq!(info.build, None);
        assert_eq!(info.name.as_deref(), Some("Ubuntu"));
        assert_eq!(info.kernel.as_deref(), Some("6.8.0-40-generic"));
    }
}
