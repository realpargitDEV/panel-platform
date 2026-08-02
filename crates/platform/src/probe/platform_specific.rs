//! The reads that differ per operating system.
//!
//! **Every `#[cfg]` in the scan lives in this file.** Each parser is separated
//! from the subprocess that feeds it, so the parsing is tested on any machine
//! and only the invocation is platform-dependent. The parsers are `pub` for the
//! same reason as those in [`super::os`]: a Linux-only parser is unreachable,
//! and so dead code, in a Windows build.
//!
//! **Verified on Windows only.** The machine this was written on has no Linux
//! and no macOS; the `#[cfg(unix)]` invocations below have never been run.

use crate::snapshot::{GpuInfo, SystemSnapshot, VirtualizationInfo, WindowsInfo};

fn parse_bool(field: &str) -> Option<bool> {
    match field.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn vendor_of(model: &str) -> Option<String> {
    let lowered = model.to_ascii_lowercase();
    for (needle, vendor) in [
        ("nvidia", "NVIDIA"),
        ("amd", "AMD"),
        ("radeon", "AMD"),
        ("intel", "Intel"),
        ("apple", "Apple"),
    ] {
        if lowered.contains(needle) {
            return Some(vendor.to_string());
        }
    }
    None
}

/// `<VirtualizationFirmwareEnabled>,<HypervisorPresent>`.
///
/// Windows reports the firmware flag as `False` whenever a hypervisor is
/// already running, because the flag describes what firmware exposes to the
/// host rather than whether virtualization works. Trusting it unconditionally
/// would tell a machine already running Hyper-V to reboot into its BIOS, so a
/// present hypervisor is itself proof that virtualization is enabled.
pub fn parse_virtualization_csv(stdout: &str) -> VirtualizationInfo {
    let mut fields = stdout.trim().split(',');
    let firmware = fields.next().and_then(parse_bool);
    let hypervisor = fields.next().and_then(parse_bool);

    let enabled = match (firmware, hypervisor) {
        (_, Some(true)) => Some(true),
        (Some(flag), _) => Some(flag),
        (None, _) => None,
    };

    VirtualizationInfo {
        // On Windows the firmware flag only exists on a CPU that supports it,
        // and a running hypervisor proves support outright.
        supported: enabled.or(firmware),
        enabled,
        hypervisor_present: hypervisor,
    }
}

/// `vmx` (Intel) or `svm` (AMD) in `/proc/cpuinfo` flags.
pub fn parse_cpuinfo_flags(contents: &str) -> VirtualizationInfo {
    let Some(line) = contents
        .lines()
        .find(|line| line.trim_start().starts_with("flags"))
    else {
        return VirtualizationInfo::default();
    };
    let supported = line
        .split_whitespace()
        .any(|flag| flag == "vmx" || flag == "svm");

    VirtualizationInfo {
        supported: Some(supported),
        // A flag present in `/proc/cpuinfo` means the kernel can see the
        // feature, which on Linux means firmware has already enabled it.
        enabled: Some(supported),
        hypervisor_present: None,
    }
}

pub fn parse_wsl_status(stdout: &str) -> WindowsInfo {
    let value_after = |label: &str| -> Option<String> {
        stdout.lines().find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.trim().eq_ignore_ascii_case(label))
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    };

    let version = value_after("Default Version").and_then(|value| value.parse().ok());
    WindowsInfo {
        wsl_present: !stdout.trim().is_empty(),
        wsl_version: version,
        default_distro: value_after("Default Distribution"),
    }
}

pub fn parse_gpu_lines(stdout: &str) -> Vec<GpuInfo> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|model| GpuInfo {
            vendor: vendor_of(model),
            model: Some(model.to_string()),
        })
        .collect()
}

/// Run a command and return its stdout, or `None` if it could not be run.
///
/// Never propagates a failure: a machine where PowerShell is unavailable still
/// gets a snapshot, with these fields left unknown.
#[allow(dead_code)] // Used from the `#[cfg]` blocks below, one platform at a time.
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// Fill in the platform-specific groups. Failures leave fields unknown.
pub(crate) fn enrich(snapshot: &mut SystemSnapshot) {
    #[cfg(windows)]
    {
        // A subprocess rather than FFI: the workspace forbids `unsafe`, so
        // `WinVerifyTrust`-style direct calls are unavailable. This is the same
        // reasoning that chose `taskkill` in the host run mode design.
        if let Some(stdout) = output_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$c = Get-CimInstance Win32_ComputerSystem; \
                 $p = Get-CimInstance Win32_Processor | Select-Object -First 1; \
                 \"$($p.VirtualizationFirmwareEnabled),$($c.HypervisorPresent)\"",
            ],
        ) {
            snapshot.virtualization = parse_virtualization_csv(&stdout);
        }

        if let Some(stdout) = output_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_OperatingSystem).Caption",
            ],
        ) {
            let caption = stdout.trim().to_string();
            snapshot.os.edition = (!caption.is_empty()).then_some(caption);
        }

        if let Some(stdout) = output_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ],
        ) {
            snapshot.gpus = parse_gpu_lines(&stdout);
        }

        snapshot.windows = Some(
            output_of("wsl", &["--status"])
                .map(|stdout| parse_wsl_status(&stdout))
                .unwrap_or_default(),
        );
    }

    #[cfg(unix)]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/cpuinfo") {
            snapshot.virtualization = super::platform_specific::parse_cpuinfo_flags(&contents);
        }
        snapshot.linux = Some(
            std::fs::read_to_string("/etc/os-release")
                .map(|contents| super::os::parse_os_release(&contents))
                .unwrap_or_default(),
        );
        if let Some(stdout) = output_of("sh", &["-c", "lspci | grep -i vga"]) {
            snapshot.gpus = parse_gpu_lines(&stdout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_virtualization_output_is_parsed() {
        // `VirtualizationFirmwareEnabled,HypervisorPresent` from Win32_Processor
        // and Win32_ComputerSystem, emitted as one CSV line.
        let info = parse_virtualization_csv("True,False");
        assert_eq!(info.supported, Some(true));
        assert_eq!(info.enabled, Some(true));
        assert_eq!(info.hypervisor_present, Some(false));
    }

    #[test]
    fn virtualization_supported_but_disabled_is_distinguished_from_absent() {
        // These two produce completely different advice: one is a reboot into
        // firmware, the other has no fix on this machine at all.
        let disabled = parse_virtualization_csv("False,False");
        assert_eq!(disabled.enabled, Some(false));

        // When a hypervisor is already running, Windows reports the firmware
        // flag as False even though virtualization plainly works. Trusting it
        // would tell a working machine to reboot into its BIOS.
        let running = parse_virtualization_csv("False,True");
        assert_eq!(running.enabled, Some(true));
        assert_eq!(running.hypervisor_present, Some(true));
    }

    #[test]
    fn unreadable_virtualization_output_is_unknown_rather_than_false() {
        let info = parse_virtualization_csv("");
        assert_eq!(info.enabled, None);
        assert_eq!(parse_virtualization_csv("nonsense").enabled, None);
    }

    #[test]
    fn cpuinfo_flags_reveal_virtualization_support() {
        let intel = parse_cpuinfo_flags("flags\t: fpu vme de pse vmx est tm2\n");
        assert_eq!(intel.supported, Some(true));

        let amd = parse_cpuinfo_flags("flags\t: fpu vme de pse svm nx\n");
        assert_eq!(amd.supported, Some(true));

        let neither = parse_cpuinfo_flags("flags\t: fpu vme de pse\n");
        assert_eq!(neither.supported, Some(false));

        assert_eq!(parse_cpuinfo_flags("").supported, None);
    }

    #[test]
    fn wsl_status_reports_its_default_version() {
        let status = parse_wsl_status("Default Version: 2\nDefault Distribution: Ubuntu\n");
        assert!(status.wsl_present);
        assert_eq!(status.wsl_version, Some(2));
        assert_eq!(status.default_distro.as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn absent_wsl_is_reported_as_absent() {
        let status = parse_wsl_status("");
        assert!(!status.wsl_present);
        assert_eq!(status.wsl_version, None);
    }

    #[test]
    fn gpu_lines_become_entries_and_blanks_are_dropped() {
        let gpus = parse_gpu_lines("NVIDIA GeForce RTX 4070\n\nIntel(R) UHD Graphics\n");
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].vendor.as_deref(), Some("NVIDIA"));
        assert_eq!(gpus[0].model.as_deref(), Some("NVIDIA GeForce RTX 4070"));
        assert_eq!(gpus[1].vendor.as_deref(), Some("Intel"));
        assert!(parse_gpu_lines("").is_empty());
    }
}
