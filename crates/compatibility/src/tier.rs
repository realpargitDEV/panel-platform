//! How much this machine can be asked to do.
//!
//! A pure function of the snapshot, over three measured axes: logical cores,
//! total memory, and free space on the roomiest fixed volume.
//!
//! **CPU age is deliberately not an input.** Release year is a weak predictor
//! of throughput — a 2013 Xeon with 64 GB outruns a 2023 Celeron with 4 GB —
//! and tiering on it would misjudge precisely the low-end machines that most
//! need correct limits. It would also require a model-string-to-year table,
//! which is large, fuzzy, and stale the day it is written.

use project_host_platform::SystemSnapshot;
use serde::{Deserialize, Serialize};

/// The floor for a memory default, in MB. The schema's own minimum is 64
/// (`0001_initial.sql`), and the 12.5% cap must never drive a default below it.
pub const MIN_MEMORY_MB: i64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceTier {
    Minimal,
    Standard,
    Performance,
}

impl PerformanceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            PerformanceTier::Minimal => "MINIMAL",
            PerformanceTier::Standard => "STANDARD",
            PerformanceTier::Performance => "PERFORMANCE",
        }
    }
}

/// What a newly created project starts with. The field names match the
/// `projects` columns exactly, so a call site that transposed two would not
/// silently compile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResourceDefaults {
    pub memory_limit_mb: i64,
    pub cpu_limit_cores: f64,
    pub process_limit: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub tier: PerformanceTier,
    pub defaults: ResourceDefaults,
}

const GB: u64 = 1024 * 1024 * 1024;

/// Memory thresholds sit slightly below the round number they stand for.
///
/// A machine sold as 16 GB never reports 16 GiB: firmware reserves some, and
/// the advertised figure is decimal GB against `total_memory`'s binary GiB. The
/// development machine reports 16_216_674_304 bytes — 15.1 GiB — so a literal
/// `>= 16 GiB` test put it, and effectively every real 16 GB machine, in the
/// tier below the one it belongs to. The 6% allowance is what makes the tier
/// table describe machines as they are sold rather than as they measure.
const fn advertised(gibibytes: u64) -> u64 {
    gibibytes * GB / 100 * 94
}

fn tier_of_cores(cores: Option<u32>) -> PerformanceTier {
    match cores {
        Some(cores) if cores >= 8 => PerformanceTier::Performance,
        Some(cores) if cores >= 4 => PerformanceTier::Standard,
        // Unknown is the weakest value: a machine that will not say how many
        // cores it has is not assumed to have many.
        _ => PerformanceTier::Minimal,
    }
}

fn tier_of_memory(total: Option<u64>) -> PerformanceTier {
    match total {
        Some(bytes) if bytes >= advertised(16) => PerformanceTier::Performance,
        Some(bytes) if bytes >= advertised(8) => PerformanceTier::Standard,
        _ => PerformanceTier::Minimal,
    }
}

fn tier_of_disk(free: Option<u64>) -> PerformanceTier {
    match free {
        Some(bytes) if bytes >= 20 * GB => PerformanceTier::Performance,
        _ => PerformanceTier::Minimal,
    }
}

fn table_defaults(tier: PerformanceTier) -> ResourceDefaults {
    match tier {
        PerformanceTier::Minimal => ResourceDefaults {
            memory_limit_mb: 512,
            cpu_limit_cores: 0.5,
            process_limit: 128,
        },
        PerformanceTier::Standard => ResourceDefaults {
            memory_limit_mb: 1024,
            cpu_limit_cores: 1.0,
            process_limit: 256,
        },
        PerformanceTier::Performance => ResourceDefaults {
            memory_limit_mb: 2048,
            cpu_limit_cores: 2.0,
            process_limit: 512,
        },
    }
}

/// Tier this machine and produce the defaults a new project should start with.
///
/// The tier is the **weakest** of the three axes, not an average. A 32-core
/// machine with 6 GB of RAM is a `Minimal` machine, and averaging would hand it
/// defaults it cannot honour.
pub fn assess(snapshot: &SystemSnapshot) -> Assessment {
    let tier = tier_of_cores(snapshot.cpu.logical_cores)
        .min(tier_of_memory(snapshot.memory.total_bytes))
        .min(tier_of_disk(snapshot.largest_fixed_free_bytes()));

    let mut defaults = table_defaults(tier);

    // The invariant that outranks the table. The table is a set of round
    // numbers chosen for legibility; this is what keeps them safe on a machine
    // the table did not anticipate.
    if let Some(total) = snapshot.memory.total_bytes {
        let cap_mb = i64::try_from(total / 8 / 1024 / 1024).unwrap_or(i64::MAX);
        defaults.memory_limit_mb = defaults.memory_limit_mb.min(cap_mb).max(MIN_MEMORY_MB);
    }

    Assessment { tier, defaults }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machines::{self, GB};

    #[test]
    fn each_golden_machine_gets_the_tier_it_deserves() {
        assert_eq!(
            assess(&machines::windows_11_workstation()).tier,
            PerformanceTier::Performance
        );
        // 8 cores and 16 GB clears both Performance thresholds exactly.
        assert_eq!(
            assess(&machines::ubuntu_desktop()).tier,
            PerformanceTier::Performance
        );
        assert_eq!(
            assess(&machines::windows_11_midrange()).tier,
            PerformanceTier::Standard
        );
        assert_eq!(
            assess(&machines::windows_11_low_end()).tier,
            PerformanceTier::Minimal
        );
    }

    #[test]
    fn the_thresholds_are_inclusive_at_both_boundaries() {
        // The midrange machine sits exactly on 4 cores and 8 GB. One core less
        // must drop it, which is what proves the comparison is not off by one.
        let boundary = machines::windows_11_midrange();
        assert_eq!(assess(&boundary).tier, PerformanceTier::Standard);

        let mut one_core_short = boundary.clone();
        one_core_short.cpu.logical_cores = Some(3);
        assert_eq!(assess(&one_core_short).tier, PerformanceTier::Minimal);

        let mut well_short = boundary;
        well_short.memory.total_bytes = Some(6 * GB);
        assert_eq!(assess(&well_short).tier, PerformanceTier::Minimal);
    }

    #[test]
    fn a_machine_sold_as_16gb_reaches_the_performance_tier() {
        // Regression guard, from the development machine's actual reading. A
        // literal `>= 16 GiB` threshold rejected 16_216_674_304 bytes — 15.1
        // GiB — and would have put effectively every real 16 GB machine one
        // tier below where it belongs.
        let mut machine = machines::windows_11_workstation();
        machine.memory.total_bytes = Some(16_216_674_304);
        assert_eq!(assess(&machine).tier, PerformanceTier::Performance);

        // An 8 GB machine reporting the same shortfall still clears Standard.
        let mut eight = machines::windows_11_midrange();
        eight.memory.total_bytes = Some(8_105_337_152);
        assert_eq!(assess(&eight).tier, PerformanceTier::Standard);
    }

    #[test]
    fn the_allowance_does_not_promote_a_genuinely_smaller_machine() {
        // 12 GB must not be mistaken for a 16 GB machine reporting low.
        let mut twelve = machines::windows_11_workstation();
        twelve.memory.total_bytes = Some(12 * GB);
        assert_eq!(assess(&twelve).tier, PerformanceTier::Standard);

        // Nor 6 GB for an 8 GB one.
        let mut six = machines::windows_11_midrange();
        six.memory.total_bytes = Some(6 * GB);
        assert_eq!(assess(&six).tier, PerformanceTier::Minimal);
    }

    #[test]
    fn every_tier_is_reachable() {
        // A tier no machine reaches is a tier whose defaults are never tested.
        let reached: Vec<PerformanceTier> = machines::golden_set()
            .iter()
            .map(|(_, machine)| assess(machine).tier)
            .collect();
        for tier in [
            PerformanceTier::Minimal,
            PerformanceTier::Standard,
            PerformanceTier::Performance,
        ] {
            assert!(reached.contains(&tier), "{tier:?} is unreachable");
        }
    }

    #[test]
    fn the_tier_is_the_weakest_axis_not_an_average() {
        // A 32-core machine with 6 GB of RAM is a Minimal machine. Averaging
        // would hand it defaults it cannot honour.
        let mut machine = machines::windows_11_workstation();
        machine.memory.total_bytes = Some(6 * GB);
        assert_eq!(assess(&machine).tier, PerformanceTier::Minimal);
    }

    #[test]
    fn a_full_disk_drops_a_capable_machine_to_minimal() {
        assert_eq!(
            assess(&machines::windows_full_disk()).tier,
            PerformanceTier::Minimal
        );
    }

    #[test]
    fn an_unknown_axis_is_treated_as_its_weakest_value() {
        // A machine that will not say how much memory it has is not assumed to
        // have plenty.
        assert_eq!(
            assess(&machines::knows_nothing()).tier,
            PerformanceTier::Minimal
        );
    }

    #[test]
    fn no_default_exceeds_an_eighth_of_total_memory() {
        // The invariant that outranks the table: it is what makes the round
        // numbers safe on a machine the table did not anticipate.
        for (name, machine) in machines::golden_set() {
            let assessment = assess(&machine);
            if let Some(total) = machine.memory.total_bytes {
                let cap_mb = i64::try_from(total / 8 / 1024 / 1024).unwrap_or(i64::MAX);
                assert!(
                    assessment.defaults.memory_limit_mb <= cap_mb.max(MIN_MEMORY_MB),
                    "{name}: {} MB exceeds the 12.5% cap of {cap_mb} MB",
                    assessment.defaults.memory_limit_mb
                );
            }
        }
    }

    #[test]
    fn every_default_satisfies_the_schema_constraints() {
        // 0001_initial.sql lines 123-126. A default the CHECK rejects fails at
        // the moment a user presses Create.
        for (name, machine) in machines::golden_set() {
            let defaults = assess(&machine).defaults;
            assert!(
                (64..=65536).contains(&defaults.memory_limit_mb),
                "{name}: memory {}",
                defaults.memory_limit_mb
            );
            assert!(
                defaults.cpu_limit_cores > 0.0 && defaults.cpu_limit_cores <= 64.0,
                "{name}: cpu {}",
                defaults.cpu_limit_cores
            );
            assert!(
                (8..=4096).contains(&defaults.process_limit),
                "{name}: pids {}",
                defaults.process_limit
            );
        }
    }

    #[test]
    fn a_tiny_machine_still_gets_a_usable_floor() {
        // The 12.5% cap must never drive a default below what the schema allows
        // or below what any container could start with.
        let mut machine = machines::windows_11_low_end();
        machine.memory.total_bytes = Some(GB / 2);
        let defaults = assess(&machine).defaults;
        assert_eq!(defaults.memory_limit_mb, MIN_MEMORY_MB);
    }

    #[test]
    fn cpu_model_does_not_change_the_tier() {
        // Release year is not an input, deliberately. A 2013 Xeon with 64 GB
        // outruns a 2023 Celeron with 4 GB.
        let mut old = machines::windows_11_workstation();
        old.cpu.model = Some("Intel(R) Xeon(R) CPU E5-2670 0 @ 2.60GHz".to_string());
        assert_eq!(
            assess(&old).tier,
            assess(&machines::windows_11_workstation()).tier
        );
    }

    #[test]
    fn an_assessment_round_trips_through_json() {
        let assessment = assess(&machines::ubuntu_desktop());
        let json = serde_json::to_string(&assessment).expect("serialise");
        let back: Assessment = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, assessment);
    }
}
