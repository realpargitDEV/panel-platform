//! Deciding what the machine's power behaviour should be, and why.
//!
//! Pure. Nothing here reads a sensor, spawns a process or changes a setting: it
//! takes a description of the machine and answers with a decision and a
//! sentence explaining it. That is what makes "on battery, below twenty
//! percent, three projects running" a test rather than something only
//! observable by unplugging a laptop and waiting.
//!
//! # The rule everything else is subordinate to
//!
//! **Never sacrifice a running project to save power.** Nothing this module can
//! output stops, suspends or slows a project into failure. The two levers are
//! the operating system's own sleep behaviour and process scheduling priority,
//! and neither can terminate a workload. Where saving power and keeping a
//! project available conflict, the project wins and the user is told.
//!
//! # Why a decision is not a threshold
//!
//! A single CPU percentage would flip the machine between profiles several
//! times a minute, and each flip is a real change to how the operating system
//! behaves. Four things stop that, and all four are needed:
//!
//! * a **moving average**, so one busy second is not a trend;
//! * a **minimum observation window**, so a candidate has to persist before it
//!   is acted on;
//! * a **cooldown**, so changes cannot come faster than a set rate however
//!   convincing the evidence;
//! * **hysteresis**, so the boundary at which a profile is entered is not the
//!   boundary at which it is left.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// What the user asked the application to do about power.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Decide from what the machine is doing. The recommended setting, and so
    /// the default a fresh install starts on.
    #[default]
    Automatic,
    /// Always prefer the projects being responsive.
    Performance,
    Balanced,
    /// Always prefer using less.
    Efficiency,
    /// Watch, report, and change nothing automatically.
    ///
    /// Not the same as Efficiency or as being switched off: the monitoring
    /// still runs and the numbers are still shown. What stops is this module's
    /// output being applied.
    Manual,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Automatic => "automatic",
            Mode::Performance => "performance",
            Mode::Balanced => "balanced",
            Mode::Efficiency => "efficiency",
            Mode::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "automatic" => Mode::Automatic,
            "performance" => Mode::Performance,
            "balanced" => Mode::Balanced,
            "efficiency" => Mode::Efficiency,
            "manual" => Mode::Manual,
            _ => return None,
        })
    }
}

/// The behaviour a decision asks for.
///
/// Deliberately three words and not a number: this is what the interface shows
/// and what the journal records, and a percentage would imply a precision the
/// underlying levers do not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Performance,
    Balanced,
    Efficiency,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Performance => "performance",
            Profile::Balanced => "balanced",
            Profile::Efficiency => "efficiency",
        }
    }
}

/// Where the machine's power is coming from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    /// Plugged in.
    Ac,
    Battery,
    /// A desktop with no battery, or a machine that would not say. Treated as
    /// AC for policy purposes, because a machine that cannot run out of power
    /// has no reason to be conserved.
    ///
    /// The default, because a sampler that has not read a battery yet knows
    /// nothing — and guessing `Ac` before the first sample would put a laptop
    /// on the wrong profile for its first few seconds on battery.
    #[default]
    Unknown,
}

/// What the policy engine is being asked about.
///
/// One flat struct rather than a tree, because every field is read by the same
/// short decision and a caller assembling a tree would have more code than the
/// decision does.
#[derive(Debug, Clone, PartialEq)]
pub struct Conditions {
    pub mode: Mode,
    /// Percentage across all cores, 0–100, or `None` before anything has been
    /// measured.
    pub cpu_percent: Option<f32>,
    /// Fraction of total memory in use, 0.0–1.0.
    pub memory_used_fraction: f32,
    /// The hottest reading any sensor gave, in Celsius. `None` on a machine
    /// with no readable sensors, which is most Windows desktops.
    pub hottest_celsius: Option<f32>,
    pub power_source: PowerSource,
    /// 0–100, or `None` on a machine with no battery.
    pub battery_percent: Option<f32>,
    /// Projects running right now.
    pub active_projects: usize,
    /// Projects running right now that were marked as needing the machine to
    /// stay available.
    pub keep_awake_projects: usize,
    /// Whether somebody is connected to this machine remotely.
    pub remote_session: bool,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            mode: Mode::Automatic,
            cpu_percent: None,
            memory_used_fraction: 0.0,
            hottest_celsius: None,
            power_source: PowerSource::Unknown,
            battery_percent: None,
            active_projects: 0,
            keep_awake_projects: 0,
            remote_session: false,
        }
    }
}

/// What the application should do, and why it thinks so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub profile: Profile,
    /// Whether automatic sleep should be held off.
    pub prevent_sleep: bool,
    /// The sentence shown when the user asks why. Never empty: a change with
    /// no explanation is the thing this whole design is against.
    pub reason: String,
    /// Whether this differs from what is currently in force.
    pub changed: bool,
}

/// Something the user should be told about, produced alongside a decision.
// No `Eq`: two of these carry a percentage or a temperature, and `f32` has no
// total equality. `PartialEq` is what the tests compare with anyway.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    /// Temperatures are high enough to be worth saying out loud.
    ///
    /// A warning and nothing more. This application manages operating-system
    /// behaviour; it does not touch voltages, clocks or power limits, and a
    /// product that started doing so because a sensor read high would be a
    /// hardware utility wearing a hosting manager's clothes.
    Thermal { celsius: f32, message: String },
    /// The battery is low and projects are running.
    ///
    /// Explicitly *not* an instruction to stop them. Terminating somebody's
    /// hosted workload to save a laptop battery is the exact behaviour the
    /// design forbids; the user is told and decides.
    LowBattery {
        percent: f32,
        active_projects: usize,
        message: String,
    },
    /// Memory is nearly gone.
    Memory { used_fraction: f32, message: String },
}

// ------------------------------------------------------------------ constants

/// How much history the moving average covers.
///
/// Two minutes at the sampler's five-second tick. Long enough that a build or a
/// page load is not a trend, short enough that a genuinely busy machine is
/// recognised while it is still busy.
pub const AVERAGE_WINDOW: Duration = Duration::from_secs(120);

/// How long a candidate profile must be the answer before it is acted on.
pub const MIN_OBSERVATION: Duration = Duration::from_secs(120);

/// The shortest time between two profile changes.
///
/// Independent of the observation window and deliberately longer: the window
/// stops a change being made on thin evidence, this stops changes being made
/// often even when each one is justified.
pub const COOLDOWN: Duration = Duration::from_secs(300);

/// Sustained CPU at or above this, with work running, asks for performance.
const BUSY_PERCENT: f32 = 40.0;

/// Sustained CPU below this counts as idle.
///
/// Well below `BUSY_PERCENT` rather than adjacent to it: the gap *is* the
/// hysteresis. A machine hovering at 30% is in neither band and stays where it
/// is, instead of alternating across a single boundary.
const IDLE_PERCENT: f32 = 15.0;

/// Battery percentage below which conserving comes first.
const LOW_BATTERY_PERCENT: f32 = 20.0;

/// Battery percentage that produces a warning while projects are running.
const WARN_BATTERY_PERCENT: f32 = 25.0;

/// Temperature worth telling the user about.
///
/// High rather than cautious. Modern parts run hot under load by design, and a
/// warning that fires during every build is a warning nobody reads.
const HOT_CELSIUS: f32 = 90.0;

/// Memory pressure worth telling the user about.
const MEMORY_PRESSURE: f32 = 0.92;

// -------------------------------------------------------------------- engine

/// One CPU reading and when it was taken.
#[derive(Debug, Clone, Copy)]
struct Reading {
    at: Instant,
    percent: f32,
}

/// Holds the history a decision needs and produces decisions from it.
///
/// Stateful only in the ways the design requires: the samples behind the moving
/// average, when the candidate profile first became the answer, and when the
/// last change was applied.
#[derive(Debug)]
pub struct PolicyEngine {
    readings: VecDeque<Reading>,
    current: Profile,
    /// The profile the conditions have been asking for, and since when.
    candidate: Option<(Profile, Instant)>,
    last_change: Option<Instant>,
    window: Duration,
    observation: Duration,
    cooldown: Duration,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self::with_timings(AVERAGE_WINDOW, MIN_OBSERVATION, COOLDOWN)
    }

    /// An engine with its own timings.
    ///
    /// Only so the tests can exercise five minutes of behaviour in a
    /// millisecond. Nothing in the application constructs one of these: a
    /// cooldown that could be configured is a cooldown a user could set to
    /// zero, and the oscillation this exists to prevent would be back.
    pub fn with_timings(window: Duration, observation: Duration, cooldown: Duration) -> Self {
        Self {
            readings: VecDeque::new(),
            current: Profile::Balanced,
            candidate: None,
            last_change: None,
            window,
            observation,
            cooldown,
        }
    }

    /// The profile currently in force.
    pub fn current(&self) -> Profile {
        self.current
    }

    /// The moving average of the readings still inside the window.
    ///
    /// `None` until something has been recorded, which admission and policy
    /// both treat as "cannot say" rather than as zero.
    pub fn average_cpu(&self) -> Option<f32> {
        if self.readings.is_empty() {
            return None;
        }
        let total: f32 = self.readings.iter().map(|reading| reading.percent).sum();
        Some(total / self.readings.len() as f32)
    }

    /// Record a CPU reading, dropping anything older than the window.
    pub fn record(&mut self, percent: f32, now: Instant) {
        self.readings.push_back(Reading { at: now, percent });
        while self
            .readings
            .front()
            .is_some_and(|reading| now.duration_since(reading.at) > self.window)
        {
            self.readings.pop_front();
        }
    }

    /// Decide, given what is true now.
    ///
    /// Records the reading, works out what the conditions ask for, and then
    /// applies the observation window and the cooldown to whether that becomes
    /// the answer.
    pub fn decide(&mut self, conditions: &Conditions, now: Instant) -> Decision {
        if let Some(percent) = conditions.cpu_percent {
            self.record(percent, now);
        }

        // A mode the user chose is the answer, immediately and without a
        // window: they did not ask the application to think about it, and
        // making them wait two minutes for a switch they just flicked would
        // look like a broken control.
        if let Some(fixed) = fixed_profile(conditions.mode) {
            let changed = fixed != self.current;
            if changed {
                self.current = fixed;
                self.last_change = Some(now);
                self.candidate = None;
            }
            return Decision {
                profile: fixed,
                prevent_sleep: wants_sleep_held(conditions),
                reason: format!(
                    "{} mode is selected, so the application is not choosing.",
                    title(conditions.mode)
                ),
                changed,
            };
        }

        // Manual: watch and report, change nothing. The profile stays whatever
        // it was, and sleep prevention still applies — that is not a power
        // *policy*, it is the application refusing to let the machine sleep out
        // from under a workload somebody is relying on.
        if conditions.mode == Mode::Manual {
            return Decision {
                profile: self.current,
                prevent_sleep: wants_sleep_held(conditions),
                reason: "Manual mode is selected. The machine is being watched \
                         and nothing is being changed automatically."
                    .to_string(),
                changed: false,
            };
        }

        let (wanted, reason) = self.automatic(conditions);
        let prevent_sleep = wants_sleep_held(conditions);

        if wanted == self.current {
            // Whatever was building towards a change is no longer building.
            self.candidate = None;
            return Decision {
                profile: self.current,
                prevent_sleep,
                reason,
                changed: false,
            };
        }

        // The candidate has to be the same answer for a while before it counts.
        let since = match self.candidate {
            Some((profile, since)) if profile == wanted => since,
            _ => {
                self.candidate = Some((wanted, now));
                now
            }
        };

        let observed_long_enough = now.duration_since(since) >= self.observation;
        let cooled_down = self
            .last_change
            .is_none_or(|last| now.duration_since(last) >= self.cooldown);

        if !observed_long_enough || !cooled_down {
            // The reason `automatic` gave explains `wanted`, and `wanted` is
            // not what is in force yet. Returning it unchanged would put a
            // sentence about being busy under the word "Balanced" — an
            // interface contradicting itself in two adjacent fields. What is
            // actually true is that a change is being considered and has not
            // been made, so that is what is said.
            let holding = if cooled_down {
                "waiting to see whether it lasts"
            } else {
                "waiting before changing again"
            };
            return Decision {
                profile: self.current,
                prevent_sleep,
                reason: format!(
                    "Staying on {}. {reason} The application is {holding}.",
                    title_of(self.current)
                ),
                changed: false,
            };
        }

        self.current = wanted;
        self.candidate = None;
        self.last_change = Some(now);

        Decision {
            profile: wanted,
            prevent_sleep,
            reason,
            changed: true,
        }
    }

    /// What the conditions ask for, before any smoothing is applied.
    ///
    /// The order of the arms is the priority order, and it is deliberate:
    /// battery before load, because a laptop about to die has a more pressing
    /// problem than being fast; temperature before load, for the same reason.
    fn automatic(&self, conditions: &Conditions) -> (Profile, String) {
        let average = self.average_cpu();
        let on_battery = conditions.power_source == PowerSource::Battery;

        if on_battery
            && conditions
                .battery_percent
                .is_some_and(|p| p < LOW_BATTERY_PERCENT)
        {
            let percent = conditions.battery_percent.unwrap_or_default();
            return (
                Profile::Efficiency,
                format!(
                    "The battery is at {percent:.0}% and the computer is not plugged in, \
                     so power is being conserved. Running projects are not affected."
                ),
            );
        }

        if conditions
            .hottest_celsius
            .is_some_and(|celsius| celsius >= HOT_CELSIUS)
        {
            let celsius = conditions.hottest_celsius.unwrap_or_default();
            return (
                Profile::Efficiency,
                format!(
                    "A sensor is reading {celsius:.0}°C, so the machine is being asked \
                     to do less. No project has been stopped."
                ),
            );
        }

        // A remote session is somebody waiting on this machine. Responsiveness
        // is what they experience, and it is worth AC power to give it to them.
        if conditions.remote_session && !on_battery {
            return (
                Profile::Performance,
                "A remote session is active, so responsiveness is being kept up.".to_string(),
            );
        }

        match average {
            Some(cpu) if cpu >= BUSY_PERCENT && conditions.active_projects > 0 && !on_battery => (
                Profile::Performance,
                format!(
                    "{} {} running and processor use has averaged {cpu:.0}% \
                     while the computer is plugged in.",
                    conditions.active_projects,
                    plural(conditions.active_projects, "project is", "projects are"),
                ),
            ),

            Some(cpu) if cpu < IDLE_PERCENT && on_battery => (
                Profile::Efficiency,
                format!(
                    "Processor use has averaged {cpu:.0}% and the computer is running \
                     on battery."
                ),
            ),

            Some(cpu) if cpu < IDLE_PERCENT && conditions.active_projects == 0 => (
                Profile::Efficiency,
                format!("Nothing is running and processor use has averaged {cpu:.0}%."),
            ),

            Some(cpu) => (
                Profile::Balanced,
                format!(
                    "Processor use has averaged {cpu:.0}% with {} {} running.",
                    conditions.active_projects,
                    plural(conditions.active_projects, "project", "projects"),
                ),
            ),

            // Nothing measured yet. Balanced is the answer that assumes least.
            None => (
                Profile::Balanced,
                "The machine has not been measured yet.".to_string(),
            ),
        }
    }
}

/// The profile a fixed mode names, or `None` for the two that do not name one.
fn fixed_profile(mode: Mode) -> Option<Profile> {
    match mode {
        Mode::Performance => Some(Profile::Performance),
        Mode::Balanced => Some(Profile::Balanced),
        Mode::Efficiency => Some(Profile::Efficiency),
        Mode::Automatic | Mode::Manual => None,
    }
}

/// Whether anything running is a reason to hold sleep off.
///
/// Independent of the profile and of the mode, including Manual. Letting a
/// machine sleep while it is hosting a bot somebody is talking to, or while
/// somebody is connected to it remotely, is not a power saving — it is the
/// workload failing. This is the one thing the application does regardless of
/// which power mode is selected.
fn wants_sleep_held(conditions: &Conditions) -> bool {
    conditions.remote_session || conditions.keep_awake_projects > 0
}

/// What the user should be told about, given the conditions.
///
/// Separate from the decision because a warning is not a change: these are
/// produced whether or not anything was altered, and in Manual mode they are
/// the only output at all.
pub fn warnings(conditions: &Conditions) -> Vec<Warning> {
    let mut warnings = Vec::new();

    if let Some(celsius) = conditions.hottest_celsius {
        if celsius >= HOT_CELSIUS {
            warnings.push(Warning::Thermal {
                celsius,
                message: format!(
                    "A temperature sensor is reading {celsius:.0}°C. Check that the \
                     computer's vents are clear. Nothing has been stopped."
                ),
            });
        }
    }

    if conditions.power_source == PowerSource::Battery && conditions.active_projects > 0 {
        if let Some(percent) = conditions.battery_percent {
            if percent <= WARN_BATTERY_PERCENT {
                warnings.push(Warning::LowBattery {
                    percent,
                    active_projects: conditions.active_projects,
                    message: format!(
                        "The battery is at {percent:.0}% while {} {} running. \
                         Connect the computer to power to keep them available.",
                        conditions.active_projects,
                        plural(conditions.active_projects, "project is", "projects are"),
                    ),
                });
            }
        }
    }

    if conditions.memory_used_fraction >= MEMORY_PRESSURE {
        warnings.push(Warning::Memory {
            used_fraction: conditions.memory_used_fraction,
            message: format!(
                "Memory is {:.0}% used. Starting another project may be refused.",
                conditions.memory_used_fraction * 100.0
            ),
        });
    }

    warnings
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 {
        one
    } else {
        many
    }
}

/// A profile's name as a sentence uses it.
fn title_of(profile: Profile) -> &'static str {
    match profile {
        Profile::Performance => "performance",
        Profile::Balanced => "balanced",
        Profile::Efficiency => "efficiency",
    }
}

fn title(mode: Mode) -> &'static str {
    match mode {
        Mode::Automatic => "Automatic",
        Mode::Performance => "Performance",
        Mode::Balanced => "Balanced",
        Mode::Efficiency => "Efficiency",
        Mode::Manual => "Manual",
    }
}

#[cfg(test)]
mod tests {
    // A failing assertion is how a test reports, and the workspace's ban on
    // panicking is about the paths that run for a user. Same allowance the
    // other crates' test modules take.
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    /// An engine whose windows are short enough to test against.
    fn engine() -> PolicyEngine {
        PolicyEngine::with_timings(
            Duration::from_secs(10),
            Duration::from_millis(20),
            Duration::from_millis(20),
        )
    }

    /// An engine that changes nothing until a long time has passed, for the
    /// tests about *not* changing.
    fn patient_engine() -> PolicyEngine {
        PolicyEngine::with_timings(
            Duration::from_secs(10),
            Duration::from_secs(120),
            Duration::from_secs(300),
        )
    }

    fn busy_on_ac() -> Conditions {
        Conditions {
            cpu_percent: Some(70.0),
            power_source: PowerSource::Ac,
            active_projects: 5,
            ..Default::default()
        }
    }

    fn idle_on_battery() -> Conditions {
        Conditions {
            cpu_percent: Some(4.0),
            power_source: PowerSource::Battery,
            battery_percent: Some(80.0),
            active_projects: 1,
            ..Default::default()
        }
    }

    /// Drive the engine for `ticks` at `every`, and answer with the last
    /// decision. This is how every scenario reaches the point of deciding.
    fn run(
        engine: &mut PolicyEngine,
        conditions: &Conditions,
        ticks: u32,
        every: Duration,
    ) -> Decision {
        let mut now = Instant::now();
        let mut decision = engine.decide(conditions, now);
        for _ in 1..ticks {
            now += every;
            decision = engine.decide(conditions, now);
        }
        decision
    }

    /// The first example from the request: several projects, sustained load,
    /// plugged in.
    #[test]
    fn sustained_load_on_mains_power_asks_for_performance() {
        let mut engine = engine();
        let decision = run(&mut engine, &busy_on_ac(), 10, Duration::from_millis(10));

        assert_eq!(decision.profile, Profile::Performance);
        assert!(decision.reason.contains("70%"), "{}", decision.reason);
        assert!(
            decision.reason.contains("plugged in"),
            "{}",
            decision.reason
        );
    }

    /// The second: mostly idle, on battery.
    #[test]
    fn an_idle_machine_on_battery_asks_for_efficiency() {
        let mut engine = engine();
        let decision = run(
            &mut engine,
            &idle_on_battery(),
            10,
            Duration::from_millis(10),
        );

        assert_eq!(decision.profile, Profile::Efficiency);
        assert!(decision.reason.contains("battery"), "{}", decision.reason);
    }

    /// The behaviour the whole design exists for: a burst of load must not
    /// change anything.
    #[test]
    fn two_seconds_of_load_does_not_change_the_profile() {
        let mut engine = patient_engine();
        let mut now = Instant::now();

        // Settled, idle, nothing running.
        let quiet = Conditions {
            cpu_percent: Some(5.0),
            power_source: PowerSource::Ac,
            ..Default::default()
        };
        for _ in 0..10 {
            engine.decide(&quiet, now);
            now += Duration::from_secs(5);
        }
        let settled = engine.current();

        // Two ticks of heavy load, then quiet again.
        for _ in 0..2 {
            let decision = engine.decide(&busy_on_ac(), now);
            assert!(
                !decision.changed,
                "a burst of load changed the profile: {}",
                decision.reason
            );
            now += Duration::from_secs(5);
        }

        assert_eq!(engine.current(), settled);
    }

    /// The cooldown is separate from the observation window, and longer. A
    /// second justified change immediately after a first must still wait.
    #[test]
    fn a_second_change_waits_for_the_cooldown_however_good_the_evidence() {
        let mut engine = PolicyEngine::with_timings(
            Duration::from_secs(10),
            Duration::from_millis(1),
            Duration::from_secs(300),
        );
        let mut now = Instant::now();

        // First change: busy on AC. The change lands on whichever call first
        // satisfies the observation window, and every call after it reports
        // `changed: false` because the profile is now steady — so this asks
        // whether a change happened at all, not whether the last call made one.
        let mut changed_once = engine.decide(&busy_on_ac(), now).changed;
        for _ in 0..5 {
            now += Duration::from_millis(10);
            changed_once |= engine.decide(&busy_on_ac(), now).changed;
        }
        assert!(changed_once, "the first change never happened");
        assert_eq!(engine.current(), Profile::Performance);

        // Conditions reverse completely, and the observation window is
        // satisfied instantly — but the cooldown is not.
        for _ in 0..20 {
            now += Duration::from_millis(10);
            let decision = engine.decide(&idle_on_battery(), now);
            assert!(
                !decision.changed,
                "a change landed inside the cooldown: {}",
                decision.reason
            );
        }
        assert_eq!(engine.current(), Profile::Performance);

        // Past the cooldown, it changes.
        //
        // Two ticks, not one. The busy readings age out of the average window
        // during the jump, which makes `Efficiency` a candidate that was not
        // one a moment ago — and a fresh candidate starts its own observation
        // window. The first tick past the cooldown establishes it; the second
        // is the earliest one that can act on it. Expecting a single tick to
        // change the profile would be asserting that the observation window
        // stops applying once a cooldown has expired, which is not the design.
        now += Duration::from_secs(301);
        let established = engine.decide(&idle_on_battery(), now);
        assert!(!established.changed, "a fresh candidate skipped its window");

        now += Duration::from_millis(10);
        let decision = engine.decide(&idle_on_battery(), now);
        assert!(
            decision.changed,
            "the cooldown never expired: profile={:?} reason={}",
            decision.profile, decision.reason
        );
        assert_eq!(decision.profile, Profile::Efficiency);
    }

    /// Found by running the thing: the reason described the profile the
    /// machine *wanted*, while the profile field showed the one still in
    /// force — so a panel read "Balanced" over a sentence explaining why the
    /// machine was busy enough for Performance.
    #[test]
    fn a_held_back_change_does_not_explain_a_profile_that_is_not_in_force() {
        let mut engine = PolicyEngine::with_timings(
            Duration::from_secs(10),
            Duration::from_secs(120),
            Duration::from_secs(300),
        );
        let mut now = Instant::now();

        let settled = engine.current();
        for _ in 0..5 {
            let decision = engine.decide(&busy_on_ac(), now);
            assert!(!decision.changed);
            assert_eq!(decision.profile, settled, "the profile changed too early");
            assert!(
                decision.reason.contains(title_of(settled)),
                "the reason does not name the profile actually in force: {}",
                decision.reason
            );
            assert!(
                decision.reason.contains("waiting"),
                "the reason does not say a change is being considered: {}",
                decision.reason
            );
            now += Duration::from_secs(5);
        }
    }

    /// The gap between the two thresholds is the hysteresis. A machine sitting
    /// between them stays where it is rather than alternating.
    #[test]
    fn load_between_the_two_thresholds_is_balanced_and_stable() {
        let mut engine = engine();
        let middling = Conditions {
            cpu_percent: Some(28.0),
            power_source: PowerSource::Ac,
            active_projects: 2,
            ..Default::default()
        };

        let decision = run(&mut engine, &middling, 20, Duration::from_millis(10));
        assert_eq!(decision.profile, Profile::Balanced);

        // Twenty more ticks at the same load change nothing further.
        let again = run(&mut engine, &middling, 20, Duration::from_millis(10));
        assert!(!again.changed);
    }

    /// Battery before load. A laptop about to die has a more pressing problem
    /// than being fast, even with five projects pinning the processor.
    #[test]
    fn a_nearly_flat_battery_outranks_a_busy_processor() {
        let mut engine = engine();
        let decision = run(
            &mut engine,
            &Conditions {
                cpu_percent: Some(95.0),
                power_source: PowerSource::Battery,
                battery_percent: Some(11.0),
                active_projects: 5,
                ..Default::default()
            },
            10,
            Duration::from_millis(10),
        );

        assert_eq!(decision.profile, Profile::Efficiency);
        assert!(decision.reason.contains("11%"), "{}", decision.reason);
        // …and it says the projects are safe, because that is the question the
        // user actually has when they read this.
        assert!(
            decision.reason.contains("not affected"),
            "{}",
            decision.reason
        );
    }

    /// A hot machine is asked to do less, and told about it — but nothing is
    /// stopped and nothing is undervolted.
    #[test]
    fn a_hot_machine_is_eased_off_and_the_user_is_told() {
        let conditions = Conditions {
            cpu_percent: Some(88.0),
            power_source: PowerSource::Ac,
            hottest_celsius: Some(97.0),
            active_projects: 3,
            ..Default::default()
        };

        let mut engine = engine();
        let decision = run(&mut engine, &conditions, 10, Duration::from_millis(10));
        assert_eq!(decision.profile, Profile::Efficiency);
        assert!(
            decision.reason.contains("No project has been stopped"),
            "{}",
            decision.reason
        );

        let warnings = warnings(&conditions);
        assert!(matches!(warnings.first(), Some(Warning::Thermal { .. })));
    }

    /// A remote session is somebody waiting on this machine.
    #[test]
    fn a_remote_session_keeps_the_machine_responsive_and_awake() {
        let conditions = Conditions {
            cpu_percent: Some(3.0),
            power_source: PowerSource::Ac,
            remote_session: true,
            ..Default::default()
        };

        let mut engine = engine();
        let decision = run(&mut engine, &conditions, 10, Duration::from_millis(10));

        assert_eq!(decision.profile, Profile::Performance);
        assert!(decision.prevent_sleep, "a remote session was left to sleep");
        assert!(decision.reason.contains("remote"), "{}", decision.reason);
    }

    /// Sleep is held for a project that asked for it, in every mode — Manual
    /// included. Letting the machine sleep under a workload somebody relies on
    /// is the workload failing, not a power saving.
    #[test]
    fn sleep_is_held_for_a_project_that_asked_for_it_in_every_mode() {
        for mode in [
            Mode::Automatic,
            Mode::Performance,
            Mode::Balanced,
            Mode::Efficiency,
            Mode::Manual,
        ] {
            let mut engine = engine();
            let decision = engine.decide(
                &Conditions {
                    mode,
                    cpu_percent: Some(2.0),
                    active_projects: 2,
                    keep_awake_projects: 1,
                    ..Default::default()
                },
                Instant::now(),
            );
            assert!(
                decision.prevent_sleep,
                "{} mode let the machine sleep under a hosted project",
                mode.as_str()
            );
        }
    }

    /// Nothing that needs availability means the machine's own settings apply
    /// again. The application must not hold an override it no longer needs.
    #[test]
    fn sleep_is_released_when_nothing_needs_the_machine_available() {
        let mut engine = engine();
        let decision = engine.decide(
            &Conditions {
                cpu_percent: Some(2.0),
                active_projects: 3,
                keep_awake_projects: 0,
                remote_session: false,
                ..Default::default()
            },
            Instant::now(),
        );
        assert!(!decision.prevent_sleep);
    }

    /// Manual watches and reports. Its output must never be a change.
    #[test]
    fn manual_mode_never_changes_the_profile() {
        let mut engine = engine();
        let mut now = Instant::now();

        for _ in 0..30 {
            let decision = engine.decide(
                &Conditions {
                    mode: Mode::Manual,
                    ..busy_on_ac()
                },
                now,
            );
            assert!(!decision.changed, "manual mode changed something");
            assert!(decision.reason.contains("Manual"), "{}", decision.reason);
            now += Duration::from_millis(50);
        }
        assert_eq!(engine.current(), Profile::Balanced);
    }

    /// A mode the user picked applies at once. Making them wait two minutes for
    /// a switch they just flicked would look like a broken control.
    #[test]
    fn a_mode_the_user_chose_applies_immediately() {
        let mut engine = patient_engine();
        let decision = engine.decide(
            &Conditions {
                mode: Mode::Efficiency,
                ..busy_on_ac()
            },
            Instant::now(),
        );

        assert!(decision.changed);
        assert_eq!(decision.profile, Profile::Efficiency);
        assert!(
            decision.reason.contains("Efficiency"),
            "{}",
            decision.reason
        );
    }

    /// Before anything is measured there is no basis for a decision, and
    /// Balanced is the answer that assumes least.
    #[test]
    fn an_unmeasured_machine_is_balanced_and_says_so() {
        let mut engine = engine();
        let decision = engine.decide(&Conditions::default(), Instant::now());

        assert_eq!(decision.profile, Profile::Balanced);
        assert!(
            decision.reason.contains("not been measured"),
            "{}",
            decision.reason
        );
        assert_eq!(engine.average_cpu(), None);
    }

    /// The average has to be an average, and readings have to fall out of it.
    #[test]
    fn readings_older_than_the_window_stop_counting() {
        let mut engine =
            PolicyEngine::with_timings(Duration::from_secs(10), MIN_OBSERVATION, COOLDOWN);
        let start = Instant::now();

        engine.record(100.0, start);
        engine.record(0.0, start + Duration::from_secs(1));
        assert_eq!(engine.average_cpu(), Some(50.0));

        // Far enough ahead that both fall out and only the new one remains.
        engine.record(20.0, start + Duration::from_secs(60));
        assert_eq!(engine.average_cpu(), Some(20.0));
    }

    /// The low-battery warning names the number and the count, and says what
    /// to do — never that anything will be stopped.
    #[test]
    fn a_low_battery_with_projects_running_warns_without_threatening_them() {
        let warnings = warnings(&Conditions {
            power_source: PowerSource::Battery,
            battery_percent: Some(19.0),
            active_projects: 3,
            ..Default::default()
        });

        let Some(Warning::LowBattery { message, .. }) = warnings.first() else {
            panic!("expected a low-battery warning, got {warnings:?}");
        };
        assert!(message.contains("19%"), "{message}");
        assert!(message.contains("3 projects are running"), "{message}");
        assert!(
            message.contains("Connect the computer to power"),
            "{message}"
        );
        assert!(
            !message.to_lowercase().contains("stop"),
            "the warning threatened the user's projects: {message}"
        );
    }

    /// A laptop on mains power with a low battery is charging. Warning about it
    /// would be warning about nothing.
    #[test]
    fn a_low_battery_that_is_charging_is_not_warned_about() {
        assert!(warnings(&Conditions {
            power_source: PowerSource::Ac,
            battery_percent: Some(9.0),
            active_projects: 3,
            ..Default::default()
        })
        .is_empty());
    }

    /// A desktop has no battery and no reason to be conserved.
    #[test]
    fn a_machine_with_no_battery_is_never_treated_as_running_out() {
        let mut engine = engine();
        let decision = run(
            &mut engine,
            &Conditions {
                cpu_percent: Some(70.0),
                power_source: PowerSource::Unknown,
                battery_percent: None,
                active_projects: 4,
                ..Default::default()
            },
            10,
            Duration::from_millis(10),
        );
        assert_eq!(decision.profile, Profile::Performance);
        assert!(warnings(&Conditions {
            power_source: PowerSource::Unknown,
            active_projects: 4,
            ..Default::default()
        })
        .is_empty());
    }

    #[test]
    fn every_mode_and_profile_word_survives_a_round_trip() {
        for mode in [
            Mode::Automatic,
            Mode::Performance,
            Mode::Balanced,
            Mode::Efficiency,
            Mode::Manual,
        ] {
            assert_eq!(Mode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(Mode::parse("turbo"), None);
        assert_eq!(Mode::default(), Mode::Automatic);

        for profile in [Profile::Performance, Profile::Balanced, Profile::Efficiency] {
            assert!(!profile.as_str().is_empty());
        }
    }

    /// Every decision carries a reason. A change with no explanation is the
    /// thing this whole design is against, so it is asserted for every path
    /// rather than for the ones that happened to be written with one.
    #[test]
    fn no_decision_is_ever_unexplained() {
        let cases = [
            Conditions::default(),
            busy_on_ac(),
            idle_on_battery(),
            Conditions {
                mode: Mode::Manual,
                ..busy_on_ac()
            },
            Conditions {
                mode: Mode::Performance,
                ..idle_on_battery()
            },
            Conditions {
                hottest_celsius: Some(99.0),
                ..busy_on_ac()
            },
            Conditions {
                remote_session: true,
                ..busy_on_ac()
            },
        ];

        for conditions in cases {
            let mut engine = engine();
            let decision = run(&mut engine, &conditions, 10, Duration::from_millis(10));
            assert!(
                !decision.reason.trim().is_empty(),
                "an unexplained decision for {conditions:?}"
            );
            assert!(
                decision.reason.ends_with('.'),
                "a reason that is not a sentence: {}",
                decision.reason
            );
        }
    }
}
