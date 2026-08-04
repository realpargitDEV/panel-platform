//! Reading this machine.
//!
//! One submodule per group of facts, each the only place that knows how its
//! group is read on a given OS. This module assembles them into a
//! [`SystemSnapshot`] and is the crate's only public entry point for scanning.
//!
//! Probing goes through [`SystemProbe`] so that consumers depend on the seam
//! rather than on the machine. `project-host-compatibility` never calls a probe
//! at all — it takes the resulting value.
//!
//! **The workspace forbids `unsafe`**, so nothing here uses FFI. Facts come
//! from `sysinfo` or from a subprocess, which is safe and sufficient.

mod hardware;
/// Public because several of its parsers apply to only one platform, and a
/// Linux-only parser is unreachable — and so dead code — in a Windows build.
/// They are part of what this crate offers rather than an internal detail, and
/// their tests run on every platform regardless of which build uses them.
pub mod os;
/// Public for the same reason as [`os`]: its parsers each apply to one
/// platform, and the one that does not apply to this build would otherwise be
/// dead code.
pub mod platform_specific;
mod storage;

use crate::snapshot::{Architecture, SystemSnapshot};

/// Something that can describe this machine.
pub trait SystemProbe: Send + Sync + std::fmt::Debug {
    fn snapshot(&self) -> SystemSnapshot;
}

/// Reads the real machine. The only implementation used in production.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemScanner;

impl SystemProbe for SystemScanner {
    fn snapshot(&self) -> SystemSnapshot {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        system.refresh_cpu_all();

        let mut snapshot = SystemSnapshot::unknown();
        // Always knowable: it is the target this binary was compiled for.
        snapshot.arch = Architecture::from_target(std::env::consts::ARCH);
        snapshot.cpu = hardware::read_cpu(&system);
        snapshot.memory = hardware::read_memory(&system);

        let disks = sysinfo::Disks::new_with_refreshed_list();
        snapshot.volumes = storage::read_volumes(&disks);

        snapshot.os = os::read_os(
            sysinfo::System::name(),
            sysinfo::System::kernel_version(),
            sysinfo::System::os_version(),
        );

        platform_specific::enrich(&mut snapshot);

        snapshot
    }
}

/// Returns a snapshot decided by the test, so results never depend on the
/// machine running them.
#[derive(Debug, Clone)]
pub struct FixedProbe(SystemSnapshot);

impl FixedProbe {
    pub fn new(snapshot: SystemSnapshot) -> Self {
        Self(snapshot)
    }
}

impl SystemProbe for FixedProbe {
    fn snapshot(&self) -> SystemSnapshot {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_probe_returns_exactly_what_it_was_given() {
        // The seam that makes every downstream decision testable: results must
        // never depend on the machine running the test.
        let mut machine = SystemSnapshot::unknown();
        machine.cpu.logical_cores = Some(2);
        machine.memory.total_bytes = Some(4 * 1024 * 1024 * 1024);

        let probe = FixedProbe::new(machine.clone());
        assert_eq!(probe.snapshot(), machine);
    }

    #[test]
    fn the_real_scanner_always_produces_a_snapshot() {
        // No failure case. Whatever this machine is, and whatever refuses to
        // answer, a snapshot comes back.
        let snapshot = SystemScanner.snapshot();
        assert!(
            !matches!(snapshot.arch, Architecture::Other(ref name) if name == "unknown"),
            "the architecture is always knowable from the compiled target"
        );
    }
}
