//! Read this actual machine and print what the power manager decides.
//!
//! Not a test: the numbers depend on the machine it runs on, so there is
//! nothing to assert. It exists to answer "does any of this work on real
//! hardware" — which the test suite cannot, because every test in the crate
//! describes a machine rather than reading one.
//!
//! Run with `cargo run -p project-host-power --example smoke`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use project_host_power::manager::{PowerManager, RunningProject};
use project_host_power::monitor::{MachineMonitor, SystemMonitor};
use project_host_power::power::{self, Priority, SleepHold};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("== what this machine reports ==");
    let monitor = Arc::new(MachineMonitor::new());

    // Twice, a moment apart: the first CPU reading from `sysinfo` is not a
    // measurement, and printing it would be reporting a number that means
    // nothing.
    let _ = monitor.sample();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let sample = monitor.sample();

    println!("  measured:      {}", sample.is_measured());
    println!("  cpu:           {:?}", sample.cpu_percent);
    println!(
        "  memory:        {:.1} GB of {:.1} GB ({:.0}%)",
        sample.memory_used_bytes as f64 / 1e9,
        sample.memory_total_bytes as f64 / 1e9,
        sample.memory_used_fraction() * 100.0
    );
    println!("  hottest:       {:?} {:?}", sample.hottest_celsius, sample.hottest_sensor);
    println!("  power source:  {:?}", sample.power_source);
    println!("  battery:       {:?} charging={:?}", sample.battery_percent, sample.charging);

    println!("\n== sleep hold ==");
    let mut hold = SleepHold::new();
    let taken = hold.set(true);
    println!("  taken:         {taken} (held={})", hold.held());
    let again = hold.set(true);
    println!("  asked again:   {again} (must be false)");
    let released = hold.set(false);
    println!("  released:      {released} (held={})", hold.held());

    println!("\n== priority command on this platform ==");
    for priority in [Priority::Low, Priority::Normal, Priority::High] {
        let command = power::priority_command(std::process::id(), priority);
        println!("  {:<7} {} {:?}", priority.as_str(), command.program, command.args);
    }

    // Against this process, which is the only one it is safe to reprioritise:
    // it is about to exit, and nothing else depends on it.
    let applied = power::apply_priority(std::process::id(), Priority::Low).await;
    println!("  applying Low to self: {applied:?}");
    let _ = power::apply_priority(std::process::id(), Priority::Normal).await;

    println!("\n== the manager, ticked ==");
    let mut manager = PowerManager::new(monitor);
    let projects = vec![RunningProject {
        id: "smoke".to_string(),
        pid: None,
        priority: Priority::Normal,
        keep_awake: true,
    }];

    let now = Instant::now();
    let first = manager.tick(&projects, false, now, 0).await;
    println!("  profile:       {}", first.profile.as_str());
    println!("  reason:        {}", first.reason);
    println!("  prevent_sleep: {}", first.prevent_sleep);
    println!("  sleep_held:    {}", first.sleep_held);
    println!("  warnings:      {:?}", first.warnings);

    // A second tick with nothing running: the hold must be released.
    let second = manager.tick(&[], false, now + Duration::from_secs(5), 5).await;
    println!("\n  with nothing running:");
    println!("  prevent_sleep: {}", second.prevent_sleep);
    println!("  sleep_held:    {}", second.sleep_held);

    let (entries, cursor) = manager.journal_since(0);
    println!("\n== journal ({} entries, cursor {cursor}) ==", entries.len());
    for entry in entries {
        println!("  [{}] {:?} — {}", entry.seq, entry.event, entry.reason);
    }

    manager.shutdown();
    println!("\nshut down cleanly");
}
