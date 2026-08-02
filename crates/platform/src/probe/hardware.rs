//! CPU and memory counts, from `sysinfo`.

use crate::snapshot::{CpuInfo, MemoryInfo};

/// `sysinfo` reports 0 for a count it could not determine. Zero is not a
/// hardware fact, and letting it through would classify a measurement failure
/// as the weakest possible machine.
pub(crate) fn non_zero(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

pub(crate) fn read_cpu(system: &sysinfo::System) -> CpuInfo {
    let first = system.cpus().first();
    CpuInfo {
        vendor: first
            .map(|cpu| cpu.vendor_id().trim().to_string())
            .filter(|vendor| !vendor.is_empty()),
        model: first
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|model| !model.is_empty()),
        physical_cores: sysinfo::System::physical_core_count()
            .and_then(|count| u32::try_from(count).ok())
            .and_then(non_zero),
        logical_cores: u32::try_from(system.cpus().len()).ok().and_then(non_zero),
    }
}

pub(crate) fn read_memory(system: &sysinfo::System) -> MemoryInfo {
    let total = system.total_memory();
    MemoryInfo {
        total_bytes: (total > 0).then_some(total),
        available_bytes: (total > 0).then(|| system.available_memory()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reports_cores_and_memory() {
        // Any machine that can run this test has at least one core and some
        // memory. Asserting specific values would assert the test runner's
        // hardware, which is why every *decision* is tested against a
        // constructed snapshot instead.
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        system.refresh_cpu_all();

        let cpu = read_cpu(&system);
        let memory = read_memory(&system);

        assert!(cpu.logical_cores.is_some_and(|cores| cores >= 1));
        assert!(memory.total_bytes.is_some_and(|bytes| bytes > 0));
        assert!(
            memory
                .available_bytes
                .is_some_and(|free| free <= memory.total_bytes.unwrap_or(u64::MAX)),
            "available memory cannot exceed total"
        );
    }

    #[test]
    fn a_zero_core_count_is_reported_as_unknown() {
        // sysinfo returns 0 rather than an error when it cannot tell. A 0 that
        // reached the tier would read as the weakest possible machine for a
        // reason that is a measurement failure, not a hardware fact.
        assert_eq!(non_zero(0), None);
        assert_eq!(non_zero(8), Some(8));
    }
}
