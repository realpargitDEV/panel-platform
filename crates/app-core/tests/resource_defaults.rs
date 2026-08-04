//! The defaults a new project starts with come from the machine, not a
//! constant.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use project_host_compatibility::machines::{windows_11_low_end, windows_11_workstation};
use project_host_compatibility::{assess, PerformanceTier};
use project_host_platform::SystemSnapshot;

#[test]
fn a_low_end_machine_gets_smaller_defaults_than_a_workstation() {
    let low = assess(&windows_11_low_end());
    let high = assess(&windows_11_workstation());

    assert!(low.defaults.memory_limit_mb < high.defaults.memory_limit_mb);
    assert!(low.defaults.cpu_limit_cores < high.defaults.cpu_limit_cores);
    assert!(low.defaults.process_limit < high.defaults.process_limit);
    assert_eq!(low.tier, PerformanceTier::Minimal);
}

#[test]
fn an_unknown_machine_gets_the_defaults_that_were_hardcoded_before() {
    // The regression guard on this whole change: a machine we cannot measure
    // must behave exactly as the application did when 512/1.0/128 were literals
    // at the creation call site.
    let assessment = assess(&SystemSnapshot::unknown());
    assert_eq!(assessment.defaults.memory_limit_mb, 512);
    assert_eq!(assessment.defaults.process_limit, 128);
}

#[test]
fn every_golden_machine_produces_defaults_the_schema_will_accept() {
    // create_project writes these straight into columns carrying CHECK
    // constraints. A default outside them fails at the moment a user presses
    // Create, which is the worst possible place to discover it.
    for (name, machine) in project_host_compatibility::machines::golden_set() {
        let defaults = assess(&machine).defaults;
        assert!(
            (64..=65536).contains(&defaults.memory_limit_mb),
            "{name}: memory_limit_mb {} is outside the CHECK",
            defaults.memory_limit_mb
        );
        assert!(
            defaults.cpu_limit_cores > 0.0 && defaults.cpu_limit_cores <= 64.0,
            "{name}: cpu_limit_cores {} is outside the CHECK",
            defaults.cpu_limit_cores
        );
        assert!(
            (8..=4096).contains(&defaults.process_limit),
            "{name}: process_limit {} is outside the CHECK",
            defaults.process_limit
        );
    }
}
