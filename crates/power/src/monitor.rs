//! What the machine is doing right now.
//!
//! One sampler, sampled on a timer, read by everything. The alternative — each
//! caller asking the operating system when it needs a number — is what turns a
//! resource monitor into a resource problem: walking the process table and
//! reading every temperature sensor is not free, and doing it per render on a
//! busy machine makes the interface unusable exactly when it is most needed.
//!
//! # Cost
//!
//! The expensive parts are separated from the cheap ones and asked for less
//! often. Memory and CPU are cheap and read every tick. Temperatures, disks and
//! network counters are not, and are read every fourth tick. Battery is read
//! through a separate library that opens a device handle, and is also on the
//! slow path.
//!
//! # Honesty about what is not there
//!
//! Every field that can be absent is an `Option`, and absence is reported as
//! absence rather than as zero:
//!
//! * Most Windows desktops expose no readable CPU temperature at all. A
//!   monitor that showed `0°C` would be inventing a reading.
//! * GPU use has no cross-platform interface that does not mean shipping a
//!   vendor SDK. It is reported when a sensor names one and left absent
//!   otherwise, rather than estimated.
//! * A desktop has no battery, which is not the same as a battery at 0%.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::policy::PowerSource;

/// How many fast ticks pass between the expensive readings.
const SLOW_EVERY: u32 = 4;

/// One reading of the machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Across all cores, 0–100. `None` on the first tick: the figure is a delta
    /// between two refreshes, and the first has nothing to compare against.
    pub cpu_percent: Option<f32>,
    /// The nominal frequency of the first core, in MHz. `None` where the
    /// platform will not say.
    pub cpu_frequency_mhz: Option<u64>,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    /// The hottest sensor, and its label. Absent on a machine with none
    /// readable, which is most Windows desktops.
    pub hottest_celsius: Option<f32>,
    pub hottest_sensor: Option<String>,
    /// A GPU sensor's temperature, when one names itself as such.
    pub gpu_celsius: Option<f32>,
    /// Bytes read and written across all disks since the previous slow tick.
    pub disk_read_bytes_per_second: u64,
    pub disk_write_bytes_per_second: u64,
    /// Bytes in and out across all interfaces since the previous slow tick.
    pub network_in_bytes_per_second: u64,
    pub network_out_bytes_per_second: u64,
    pub power_source: PowerSource,
    /// 0–100. `None` on a machine with no battery.
    pub battery_percent: Option<f32>,
    /// Whether the battery is charging. `None` when there is no battery.
    pub charging: Option<bool>,
    /// Seconds since this application last saw a project start, stop, or a
    /// command arrive. Not the operating system's idle timer, which cannot be
    /// read without FFI — and saying which is meant is better than reporting a
    /// number that means something else.
    pub app_idle_seconds: u64,
}

impl Sample {
    /// Memory in use as a fraction of the total, 0.0–1.0.
    pub fn memory_used_fraction(&self) -> f32 {
        if self.memory_total_bytes == 0 {
            return 0.0;
        }
        self.memory_used_bytes as f32 / self.memory_total_bytes as f32
    }

    /// Whether anything has actually been measured yet.
    pub fn is_measured(&self) -> bool {
        self.memory_total_bytes > 0
    }
}

/// Where a sample comes from.
///
/// A trait so the manager can be driven by a described machine — one on a flat
/// battery, one at 97°C — none of which can be arranged on the machine running
/// the tests.
pub trait SystemMonitor: Send + Sync + std::fmt::Debug {
    fn sample(&self) -> Sample;
}

/// Reads this machine.
#[derive(Debug)]
pub struct MachineMonitor {
    inner: std::sync::Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    system: sysinfo::System,
    components: sysinfo::Components,
    networks: sysinfo::Networks,
    disks: sysinfo::Disks,
    /// How many times CPU has been refreshed. The authority on whether the
    /// reading means anything — `sysinfo` reports a figure on the first refresh
    /// and it is not a measurement.
    cpu_samples: u32,
    ticks: u32,
    /// The last slow reading, carried forward on fast ticks so the numbers on
    /// screen do not blink out three ticks in four.
    slow: SlowReading,
    last_slow_at: Option<Instant>,
    activity: Instant,
}

/// The half of a sample that is expensive to take.
#[derive(Debug, Clone, Default)]
struct SlowReading {
    hottest_celsius: Option<f32>,
    hottest_sensor: Option<String>,
    gpu_celsius: Option<f32>,
    disk_read_bytes_per_second: u64,
    disk_write_bytes_per_second: u64,
    network_in_bytes_per_second: u64,
    network_out_bytes_per_second: u64,
    power_source: PowerSource,
    battery_percent: Option<f32>,
    charging: Option<bool>,
}

impl Default for MachineMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineMonitor {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(Inner {
                system: sysinfo::System::new(),
                components: sysinfo::Components::new(),
                networks: sysinfo::Networks::new_with_refreshed_list(),
                disks: sysinfo::Disks::new_with_refreshed_list(),
                cpu_samples: 0,
                ticks: 0,
                slow: SlowReading::default(),
                last_slow_at: None,
                activity: Instant::now(),
            }),
        }
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Record that something happened, so the idle figure means something.
    ///
    /// Called when a project starts or stops. This is *this application's*
    /// idea of idleness and is documented as such: the operating system's own
    /// idle timer cannot be read without FFI, and reporting a different number
    /// under the same name would be worse than reporting this one honestly.
    pub fn note_activity(&self) {
        self.locked().activity = Instant::now();
    }
}

impl SystemMonitor for MachineMonitor {
    fn sample(&self) -> Sample {
        let inner = &mut *self.locked();
        inner.ticks = inner.ticks.wrapping_add(1);

        inner.system.refresh_memory();
        inner.system.refresh_cpu_usage();
        inner.cpu_samples = inner.cpu_samples.saturating_add(1);

        if inner.ticks % SLOW_EVERY == 1 || inner.last_slow_at.is_none() {
            inner.slow = slow_reading(inner);
        }

        Sample {
            cpu_percent: (inner.cpu_samples >= 2).then(|| inner.system.global_cpu_usage()),
            cpu_frequency_mhz: inner
                .system
                .cpus()
                .first()
                .map(sysinfo::Cpu::frequency)
                .filter(|frequency| *frequency > 0),
            memory_used_bytes: inner.system.used_memory(),
            memory_total_bytes: inner.system.total_memory(),
            memory_available_bytes: inner.system.available_memory(),
            hottest_celsius: inner.slow.hottest_celsius,
            hottest_sensor: inner.slow.hottest_sensor.clone(),
            gpu_celsius: inner.slow.gpu_celsius,
            disk_read_bytes_per_second: inner.slow.disk_read_bytes_per_second,
            disk_write_bytes_per_second: inner.slow.disk_write_bytes_per_second,
            network_in_bytes_per_second: inner.slow.network_in_bytes_per_second,
            network_out_bytes_per_second: inner.slow.network_out_bytes_per_second,
            power_source: inner.slow.power_source,
            battery_percent: inner.slow.battery_percent,
            charging: inner.slow.charging,
            app_idle_seconds: inner.activity.elapsed().as_secs(),
        }
    }
}

/// The expensive half: temperatures, counters and the battery.
fn slow_reading(inner: &mut Inner) -> SlowReading {
    let now = Instant::now();
    let elapsed = inner
        .last_slow_at
        .map(|last| now.duration_since(last))
        .unwrap_or(Duration::from_secs(1))
        .as_secs_f64()
        .max(0.001);
    inner.last_slow_at = Some(now);

    inner.components.refresh(true);
    let mut hottest: Option<(f32, String)> = None;
    let mut gpu: Option<f32> = None;
    for component in &inner.components {
        let Some(celsius) = component.temperature() else {
            continue;
        };
        // A sensor reading an implausible value is a broken sensor, and acting
        // on one would mean easing off a machine that is fine.
        if !(0.0..=125.0).contains(&celsius) {
            continue;
        }
        let label = component.label().to_string();
        if label.to_lowercase().contains("gpu") {
            gpu = Some(gpu.map_or(celsius, |current: f32| current.max(celsius)));
        }
        if hottest.as_ref().is_none_or(|(current, _)| celsius > *current) {
            hottest = Some((celsius, label));
        }
    }

    inner.networks.refresh(true);
    let (mut network_in, mut network_out) = (0u64, 0u64);
    for (_, data) in &inner.networks {
        network_in = network_in.saturating_add(data.received());
        network_out = network_out.saturating_add(data.transmitted());
    }

    inner.disks.refresh(true);
    let (mut disk_read, mut disk_write) = (0u64, 0u64);
    for disk in inner.disks.list() {
        let usage = disk.usage();
        disk_read = disk_read.saturating_add(usage.read_bytes);
        disk_write = disk_write.saturating_add(usage.written_bytes);
    }

    let per_second = |bytes: u64| (bytes as f64 / elapsed) as u64;
    let battery = read_battery();

    SlowReading {
        hottest_celsius: hottest.as_ref().map(|(celsius, _)| *celsius),
        hottest_sensor: hottest.map(|(_, label)| label),
        gpu_celsius: gpu,
        disk_read_bytes_per_second: per_second(disk_read),
        disk_write_bytes_per_second: per_second(disk_write),
        network_in_bytes_per_second: per_second(network_in),
        network_out_bytes_per_second: per_second(network_out),
        power_source: battery.0,
        battery_percent: battery.1,
        charging: battery.2,
    }
}

/// The battery, if there is one.
///
/// A machine with no battery answers `(Unknown, None, None)` — a desktop, which
/// is not a laptop at 0%. A failure to read the battery answers the same way
/// and is logged once per occurrence rather than being fatal: a monitor that
/// refused to report anything because one sensor was unavailable would be
/// useless on exactly the machines that need watching.
fn read_battery() -> (PowerSource, Option<f32>, Option<bool>) {
    let manager = match starship_battery::Manager::new() {
        Ok(manager) => manager,
        Err(error) => {
            tracing::debug!(%error, "no battery interface on this machine");
            return (PowerSource::Unknown, None, None);
        }
    };

    let Ok(batteries) = manager.batteries() else {
        return (PowerSource::Unknown, None, None);
    };

    // The first battery that reads. A machine with two reports the first
    // rather than an average: an average of a full battery and a missing one
    // is a number describing neither.
    let Some(battery) = batteries.flatten().next() else {
        return (PowerSource::Unknown, None, None);
    };

    let percent = battery.state_of_charge().value * 100.0;
    let (source, charging) = match battery.state() {
        starship_battery::State::Charging | starship_battery::State::Full => {
            (PowerSource::Ac, true)
        }
        starship_battery::State::Discharging => (PowerSource::Battery, false),
        // Idle or unknown on a plugged-in laptop that has stopped
        // charging at its configured limit, which is AC.
        _ => (PowerSource::Ac, false),
    };

    (source, Some(percent.clamp(0.0, 100.0)), Some(charging))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reports_a_plausible_sample() {
        let monitor = MachineMonitor::new();
        let sample = monitor.sample();

        assert!(sample.is_measured(), "nothing was measured at all");
        assert!(
            sample.memory_total_bytes > 1024 * 1024 * 1024,
            "a machine running this test has more than a gigabyte"
        );
        assert!(sample.memory_used_bytes <= sample.memory_total_bytes);
        assert!((0.0..=1.0).contains(&sample.memory_used_fraction()));
    }

    /// The first reading has no delta to work from. `sysinfo` still returns a
    /// number, and on the machine this was written on it is 100.0 — so
    /// reporting it would tell a policy engine that an idle machine is pinned.
    #[test]
    fn cpu_is_unknown_until_there_is_something_to_compare_against() {
        let monitor = MachineMonitor::new();
        assert_eq!(monitor.sample().cpu_percent, None, "the first sample");

        std::thread::sleep(Duration::from_millis(250));
        let second = monitor.sample().cpu_percent.unwrap_or(-1.0);
        assert!((0.0..=100.0).contains(&second), "got {second}");
    }

    /// Absence is reported as absence. A machine with no temperature sensor
    /// must not read as 0°C, which would look like a machine that is freezing.
    #[test]
    fn nothing_absent_is_reported_as_zero() {
        let sample = Sample::default();

        assert_eq!(sample.hottest_celsius, None);
        assert_eq!(sample.gpu_celsius, None);
        assert_eq!(sample.battery_percent, None);
        assert_eq!(sample.charging, None);
        assert_eq!(sample.cpu_percent, None);
        assert_eq!(sample.power_source, PowerSource::Unknown);
    }

    /// A monitor that costs meaningful CPU is a monitor that has defeated its
    /// own purpose. Fifty samples — over four minutes of real ticking — must
    /// take a small fraction of a second.
    #[test]
    fn sampling_is_cheap_enough_not_to_be_the_load_it_measures() {
        let monitor = MachineMonitor::new();
        // One first, so the expensive slow path has already run once.
        monitor.sample();

        let started = Instant::now();
        for _ in 0..50 {
            monitor.sample();
        }
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(5),
            "fifty samples took {elapsed:?}, which at the real interval would be \
             a monitor that is itself a meaningful load"
        );
    }

    /// Reading the battery must not fail on a machine that has none.
    #[test]
    fn a_machine_without_a_battery_is_not_a_failure() {
        let (source, percent, charging) = read_battery();

        match source {
            PowerSource::Unknown => {
                assert_eq!(percent, None);
                assert_eq!(charging, None);
            }
            _ => {
                let percent = percent.unwrap_or(-1.0);
                assert!((0.0..=100.0).contains(&percent), "got {percent}");
            }
        }
    }

    #[test]
    fn the_memory_fraction_of_an_unmeasured_machine_is_zero_rather_than_a_division_by_zero() {
        assert_eq!(Sample::default().memory_used_fraction(), 0.0);
        assert!(!Sample::default().is_measured());
    }

    /// The idle figure is this application's, and it resets when something
    /// happens.
    #[test]
    fn noting_activity_resets_the_idle_figure() {
        let monitor = MachineMonitor::new();
        std::thread::sleep(Duration::from_millis(50));
        monitor.note_activity();
        assert_eq!(monitor.sample().app_idle_seconds, 0);
    }
}
