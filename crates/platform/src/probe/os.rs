//! Which operating system this is, and precisely which version.

use crate::snapshot::{LinuxInfo, OsInfo, PackageManager};

/// A build number is at least four digits. Windows builds are five, and the
/// floor exists to stop a marketing version — the `11` in `"11 (26200)"` —
/// being mistaken for one.
const SMALLEST_PLAUSIBLE_BUILD: u32 = 1000;

/// The Windows build number, from whichever field on this machine carries it.
///
/// There is no single format. On the machine this was written on `sysinfo`
/// reports `os_version` as `"11 (26200)"` and `kernel_version` as `"26200"`;
/// elsewhere `os_version` is the dotted `"10.0.26200"`. The first
/// implementation read only the dotted form and returned `None` on real
/// hardware, which would have selected a Docker backend from a missing build.
/// All three forms are therefore tried, most specific first.
///
/// Returns `None` rather than a default. A wrong build number selects a wrong
/// Docker backend, so "unknown" must stay distinguishable from "old".
pub fn parse_build(version: &str, kernel: Option<&str>) -> Option<u32> {
    let plausible = |value: u32| (value >= SMALLEST_PLAUSIBLE_BUILD).then_some(value);

    // "11 (26200)" — the parenthesised number is the build.
    if let Some(open) = version.find('(') {
        if let Some(close) = version[open..].find(')') {
            if let Some(build) = version[open + 1..open + close]
                .trim()
                .parse()
                .ok()
                .and_then(plausible)
            {
                return Some(build);
            }
        }
    }

    // "10.0.26200" — the third dotted component.
    if let Some(build) = version
        .split('.')
        .nth(2)
        .and_then(|part| part.trim().parse().ok())
        .and_then(plausible)
    {
        return Some(build);
    }

    // Windows reports the build as its "kernel version". On Linux this field
    // is something like "6.8.0-40-generic", which fails to parse and is
    // correctly rejected — Linux has no build number and must stay `None`.
    kernel?.trim().parse().ok().and_then(plausible)
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
        build: version
            .as_deref()
            .and_then(|version| parse_build(version, kernel.as_deref())),
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
    fn a_dotted_windows_version_yields_its_build_number() {
        // The number that decides whether the WSL2 backend exists.
        assert_eq!(parse_build("10.0.26200", None), Some(26200));
        assert_eq!(parse_build("10.0.19045", None), Some(19045));
    }

    #[test]
    fn the_format_this_machine_actually_reports_yields_its_build_number() {
        // Regression guard. `sysinfo` reports "11 (26200)" here, not the dotted
        // form; the first implementation read only the dotted form and returned
        // None on real hardware, which would have selected a Docker backend
        // from a missing build number.
        assert_eq!(parse_build("11 (26200)", Some("26200")), Some(26200));
        assert_eq!(parse_build("10 (19045)", None), Some(19045));
    }

    #[test]
    fn the_build_is_taken_from_the_kernel_field_when_the_version_lacks_it() {
        // Windows reports the build as its "kernel version".
        assert_eq!(parse_build("11", Some("26200")), Some(26200));
    }

    #[test]
    fn a_marketing_version_is_never_mistaken_for_a_build() {
        // "11" is the product name, not a build. Accepting it would compare 11
        // against a floor of 19045 and reject a perfectly capable machine.
        assert_eq!(parse_build("11", None), None);
        assert_eq!(parse_build("11 (7)", None), None);
    }

    #[test]
    fn a_linux_kernel_version_is_not_a_build_number() {
        // Linux has no build number and must stay None rather than acquiring
        // one from a field that means something else.
        assert_eq!(parse_build("24.04", Some("6.8.0-40-generic")), None);
    }

    #[test]
    fn an_unparseable_version_yields_no_build_rather_than_a_guess() {
        // A wrong build number selects a wrong Docker backend, which is the
        // exact failure this whole design exists to prevent.
        assert_eq!(parse_build("", None), None);
        assert_eq!(parse_build("10.0", None), None);
        assert_eq!(parse_build("Windows", None), None);
        assert_eq!(parse_build("10.0.notanumber", None), None);
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

    #[test]
    fn this_windows_machines_fields_produce_a_build() {
        // The exact three values `sysinfo` returned on the development machine.
        let info = read_os(
            Some("Windows".to_string()),
            Some("26200".to_string()),
            Some("11 (26200)".to_string()),
        );
        assert_eq!(info.build, Some(26200));
    }
}
