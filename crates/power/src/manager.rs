//! One object that reads the machine, decides, acts, and remembers.
//!
//! The four other modules are deliberately unaware of each other:
//! [`monitor`](crate::monitor) reads, [`policy`](crate::policy) decides,
//! [`power`](crate::power) acts, [`journal`](crate::journal) remembers. This is
//! the only place that knows all four, which keeps the interesting logic in
//! modules that can be tested without a machine in a particular state.
//!
//! # What this crate is not told
//!
//! It is not told what a project *is*. A caller hands over a list of
//! [`RunningProject`] — an id, a pid, a priority, whether it asked to hold
//! sleep off — and nothing else. The power crate has no opinion about
//! databases, Discord or containers, and a change to any of them cannot reach
//! it.
//!
//! # Why priority is re-applied per pid rather than per project
//!
//! A project that crashes and is restarted is a new process at the same
//! priority setting, and the operating system knows nothing about the old one.
//! Keyed by `(id, pid)`, a restart is naturally a change and gets the setting
//! applied again; keyed by id alone, a restarted project would silently run at
//! whatever the system gave it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::journal::{Entry, Journal};
use crate::monitor::{Sample, SystemMonitor};
use crate::policy::{self, Conditions, Mode, Profile, Warning};
use crate::power::{self, Priority, SleepHold};

/// One project, as this crate needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProject {
    pub id: String,
    /// `None` for a project the supervisor has not got a pid for yet. Counted
    /// as running — it is — but nothing is applied to it until there is a
    /// process to apply it to.
    pub pid: Option<u32>,
    pub priority: Priority,
    /// Whether this project running is a reason to hold sleep off.
    pub keep_awake: bool,
}

/// What one tick found and did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub sample: Sample,
    pub mode: Mode,
    pub profile: Profile,
    /// Why the profile is what it is. Shown verbatim.
    pub reason: String,
    pub prevent_sleep: bool,
    /// Whether sleep is *actually* being held. Not the same as
    /// `prevent_sleep`: a platform can refuse, and reporting the request as
    /// though it were the outcome is how an interface comes to lie.
    pub sleep_held: bool,
    pub warnings: Vec<Warning>,
    pub active_projects: usize,
}

/// Reads the machine, decides, and applies.
pub struct PowerManager {
    monitor: Arc<dyn SystemMonitor>,
    engine: policy::PolicyEngine,
    journal: Journal,
    hold: SleepHold,
    mode: Mode,
    /// What each project was last actually set to, keyed by id and pid. See the
    /// module note on why the pid is part of the key.
    applied: BTreeMap<(String, u32), Priority>,
    latest: Option<Snapshot>,
}

// `SleepHold` prints as its held flag and `dyn SystemMonitor` requires `Debug`,
// so the derive would work — except that `PolicyEngine` and `Journal` would
// print several hundred readings. What is useful here is the state a person
// would ask about.
impl std::fmt::Debug for PowerManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PowerManager")
            .field("mode", &self.mode)
            .field("profile", &self.engine.current())
            .field("sleep_held", &self.hold.held())
            .field("journal_entries", &self.journal.len())
            .finish()
    }
}

impl PowerManager {
    pub fn new(monitor: Arc<dyn SystemMonitor>) -> Self {
        Self {
            monitor,
            engine: policy::PolicyEngine::new(),
            journal: Journal::new(),
            hold: SleepHold::new(),
            mode: Mode::default(),
            applied: BTreeMap::new(),
            latest: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Change what the user asked for.
    ///
    /// Takes effect on the next tick rather than immediately, which is at most
    /// a few seconds and keeps every change to the machine on one path.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// The last tick's answer, for a window that wants to render without
    /// waiting for the next one.
    pub fn latest(&self) -> Option<&Snapshot> {
        self.latest.as_ref()
    }

    /// Journal entries after `since`, with the cursor to poll from next.
    pub fn journal_since(&self, since: u64) -> (Vec<Entry>, u64) {
        self.journal.since(since)
    }

    pub fn recent_journal(&self, limit: usize) -> Vec<Entry> {
        self.journal.recent(limit)
    }

    /// Read, decide, act, record.
    ///
    /// `now` is monotonic and drives every window and cooldown; `wall` is
    /// seconds since the epoch and is only ever written into the journal. They
    /// are separate arguments because conflating them is how a machine
    /// resuming from sleep gets treated as having satisfied a five-minute
    /// cooldown it never waited through.
    pub async fn tick(
        &mut self,
        projects: &[RunningProject],
        remote_session: bool,
        now: Instant,
        wall: u64,
    ) -> Snapshot {
        let sample = self.monitor.sample();

        let conditions = Conditions {
            mode: self.mode,
            cpu_percent: sample.cpu_percent,
            memory_used_fraction: sample.memory_used_fraction(),
            hottest_celsius: sample.hottest_celsius,
            power_source: sample.power_source,
            battery_percent: sample.battery_percent,
            active_projects: projects.len(),
            keep_awake_projects: projects.iter().filter(|item| item.keep_awake).count(),
            remote_session,
        };

        let decision = self.engine.decide(&conditions, now);
        let warnings = policy::warnings(&conditions);

        if self.hold.set(decision.prevent_sleep) {
            tracing::info!(held = decision.prevent_sleep, "sleep hold changed");
        }

        self.apply_priorities(projects, decision.profile).await;
        self.journal.observe(wall, &decision, &warnings);

        let snapshot = Snapshot {
            sample,
            mode: self.mode,
            profile: decision.profile,
            reason: decision.reason,
            prevent_sleep: decision.prevent_sleep,
            sleep_held: self.hold.held(),
            warnings,
            active_projects: projects.len(),
        };
        self.latest = Some(snapshot.clone());
        snapshot
    }

    /// Set every project that is not already where it should be.
    ///
    /// Only the difference is applied. Re-running the command every tick for a
    /// project already at the right priority would be a process spawn every few
    /// seconds per project, which is a heavier load than anything it is trying
    /// to manage.
    async fn apply_priorities(&mut self, projects: &[RunningProject], profile: Profile) {
        let mut live = BTreeMap::new();

        for project in projects {
            let Some(pid) = project.pid else {
                continue;
            };
            let wanted = power::effective(project.priority, profile);
            let key = (project.id.clone(), pid);

            if self.applied.get(&key) == Some(&wanted) {
                live.insert(key, wanted);
                continue;
            }

            match power::apply_priority(pid, wanted).await {
                Ok(()) => {
                    tracing::debug!(
                        project = %project.id,
                        pid,
                        priority = wanted.as_str(),
                        "scheduling priority set"
                    );
                    live.insert(key, wanted);
                }
                Err(error) => {
                    // Not recorded as applied, so the next tick tries again —
                    // and not an error that stops anything, because a project
                    // at the wrong priority is still a project that is running.
                    tracing::debug!(
                        project = %project.id,
                        pid,
                        %error,
                        "could not set scheduling priority"
                    );
                }
            }
        }

        // Anything not in this tick's list has stopped. Dropping it keeps the
        // map the size of what is running rather than of everything that ever
        // ran in this session.
        self.applied = live;
    }

    /// Release the sleep hold and forget what was applied.
    ///
    /// Called on the way out. The priorities themselves are not put back: the
    /// processes they were set on are being stopped by the same shutdown, and
    /// a command to re-prioritise a process that is exiting is a race with no
    /// winner.
    pub fn shutdown(&mut self) {
        if self.hold.set(false) {
            tracing::info!("sleep hold released");
        }
        self.applied.clear();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use std::time::Duration;

    use super::*;
    use crate::monitor::Sample;
    use crate::policy::PowerSource;

    /// A machine that is whatever the test says it is.
    #[derive(Debug)]
    struct FakeMachine(std::sync::Mutex<Sample>);

    impl FakeMachine {
        fn new(sample: Sample) -> Arc<Self> {
            Arc::new(Self(std::sync::Mutex::new(sample)))
        }
    }

    impl SystemMonitor for FakeMachine {
        fn sample(&self) -> Sample {
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    fn idle_sample() -> Sample {
        Sample {
            cpu_percent: Some(2.0),
            memory_used_bytes: 4_000_000_000,
            memory_total_bytes: 16_000_000_000,
            memory_available_bytes: 12_000_000_000,
            power_source: PowerSource::Ac,
            ..Sample::default()
        }
    }

    fn project(id: &str, keep_awake: bool) -> RunningProject {
        RunningProject {
            id: id.to_string(),
            // No pid: these tests are about the decision, and a real pid would
            // mean the suite changing the priority of whatever process happened
            // to own that number.
            pid: None,
            priority: Priority::Normal,
            keep_awake,
        }
    }

    #[tokio::test]
    async fn a_machine_with_nothing_running_does_not_hold_sleep_off() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        let snapshot = manager.tick(&[], false, Instant::now(), 0).await;

        assert!(!snapshot.prevent_sleep);
        assert!(!snapshot.sleep_held);
        assert_eq!(snapshot.active_projects, 0);
    }

    /// The setting doing what it says is the whole feature.
    #[tokio::test]
    async fn a_project_that_asked_to_stay_awake_is_a_reason_to_hold_sleep() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        let snapshot = manager
            .tick(&[project("bot", true)], false, Instant::now(), 0)
            .await;

        assert!(
            snapshot.prevent_sleep,
            "a keep-awake project did not hold sleep: {}",
            snapshot.reason
        );
    }

    #[tokio::test]
    async fn a_project_that_did_not_ask_is_not_a_reason() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        let snapshot = manager
            .tick(&[project("bot", false)], false, Instant::now(), 0)
            .await;

        assert!(!snapshot.prevent_sleep);
    }

    /// Every decision carries a sentence, at every tick, in every mode.
    #[tokio::test]
    async fn no_snapshot_is_ever_unexplained() {
        for mode in [
            Mode::Automatic,
            Mode::Performance,
            Mode::Balanced,
            Mode::Efficiency,
            Mode::Manual,
        ] {
            let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
            manager.set_mode(mode);
            let snapshot = manager
                .tick(&[project("bot", false)], false, Instant::now(), 0)
                .await;

            assert!(
                !snapshot.reason.trim().is_empty(),
                "{mode:?} produced a snapshot with no reason"
            );
            assert_eq!(snapshot.mode, mode);
        }
    }

    /// Manual means manual.
    #[tokio::test]
    async fn manual_mode_changes_no_profile_however_the_machine_looks() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        manager.set_mode(Mode::Manual);

        let mut now = Instant::now();
        let first = manager.tick(&[], false, now, 0).await;
        for tick in 1..50 {
            now += Duration::from_secs(10);
            let snapshot = manager.tick(&[], false, now, tick).await;
            assert_eq!(snapshot.profile, first.profile);
        }
    }

    /// Sleep prevention is not a power *policy*, so it still applies in manual.
    #[tokio::test]
    async fn manual_mode_still_holds_sleep_for_a_project_that_asked() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        manager.set_mode(Mode::Manual);
        let snapshot = manager
            .tick(&[project("bot", true)], false, Instant::now(), 0)
            .await;

        assert!(snapshot.prevent_sleep);
    }

    #[tokio::test]
    async fn a_quiet_machine_fills_no_journal() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        let mut now = Instant::now();

        for tick in 0..200 {
            now += Duration::from_secs(2);
            manager.tick(&[], false, now, tick).await;
        }

        let (entries, _) = manager.journal_since(0);
        assert!(
            entries.len() <= 1,
            "200 quiet ticks produced {} entries",
            entries.len()
        );
    }

    #[tokio::test]
    async fn the_latest_snapshot_is_available_without_ticking_again() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        assert!(manager.latest().is_none());

        let snapshot = manager.tick(&[], false, Instant::now(), 0).await;
        assert_eq!(manager.latest(), Some(&snapshot));
    }

    /// A hot machine must say so, and must not respond by stopping anything.
    #[tokio::test]
    async fn a_hot_machine_warns_and_keeps_every_project() {
        let hot = Sample {
            hottest_celsius: Some(95.0),
            ..idle_sample()
        };
        let mut manager = PowerManager::new(FakeMachine::new(hot));
        let running = vec![project("a", false), project("b", false)];

        let snapshot = manager.tick(&running, false, Instant::now(), 0).await;

        assert!(
            snapshot
                .warnings
                .iter()
                .any(|warning| matches!(warning, Warning::Thermal { .. })),
            "a machine at 95C did not warn"
        );
        assert_eq!(
            snapshot.active_projects, 2,
            "a warning changed how many projects were running"
        );
    }

    #[tokio::test]
    async fn shutting_down_releases_the_hold() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        manager
            .tick(&[project("bot", true)], false, Instant::now(), 0)
            .await;

        manager.shutdown();
        assert!(!manager.hold.held());
    }

    /// A project with no pid yet is counted, but nothing is applied to it.
    #[tokio::test]
    async fn a_project_without_a_pid_is_counted_and_left_alone() {
        let mut manager = PowerManager::new(FakeMachine::new(idle_sample()));
        let snapshot = manager
            .tick(&[project("starting", false)], false, Instant::now(), 0)
            .await;

        assert_eq!(snapshot.active_projects, 1);
        assert!(manager.applied.is_empty(), "a pidless project was acted on");
    }
}
