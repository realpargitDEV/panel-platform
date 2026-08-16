//! The record of what this crate decided, and when.
//!
//! A power manager that changes the machine's behaviour without leaving a trace
//! is indistinguishable from a machine behaving strangely. This is the answer
//! to "why did my project slow down at four in the morning" — which is the only
//! reason any of it is worth recording.
//!
//! # Why not every tick
//!
//! The manager decides every few seconds. Writing all of those down would be
//! thousands of entries a day saying the same thing, and a log nobody can read
//! is the same as no log while costing memory to keep. Only two things go in:
//!
//! * a decision that **changed** the profile in force, and
//! * a warning **appearing or clearing**, which is a change in what the user
//!   should know rather than a repeat of what they already do.
//!
//! A thermal warning that stays true for an hour is one entry, not eighteen
//! hundred.
//!
//! # Why seconds rather than a formatted time
//!
//! This crate does not depend on the database or on the wire format, for the
//! same reason `host-runner` does not: reading a machine's temperature should
//! not require holding either in mind. Entries carry seconds since the epoch
//! and the layer that presents them formats them, using the one formatter the
//! rest of the product already shares.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::policy::{Decision, Profile, Warning};

/// How many entries are kept.
///
/// At the rate above — changes only — this is weeks of ordinary use and still a
/// bounded amount of memory on a machine that is thrashing between profiles.
pub const CAPACITY: usize = 500;

/// What happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// The profile in force changed.
    ProfileChanged { from: Profile, to: Profile },
    /// A warning started applying.
    WarningRaised { warning: Warning },
    /// A warning stopped applying.
    WarningCleared { warning: Warning },
    /// Sleep started or stopped being held off.
    SleepHoldChanged { held: bool },
}

/// One line of the record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Monotonic, starting at 1, never reused. The same idea as a console's
    /// line sequence, and for the same reason: a window can ask for everything
    /// after what it has without the two sides agreeing on a clock.
    pub seq: u64,
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub event: Event,
    /// The sentence the policy engine gave. Never empty.
    pub reason: String,
}

/// A bounded record of power decisions.
#[derive(Debug, Clone)]
pub struct Journal {
    entries: VecDeque<Entry>,
    capacity: usize,
    next_seq: u64,
    /// What was true at the last observation, so a change can be recognised.
    /// `None` before anything has been observed — which is why the first
    /// observation records nothing: there is no change from nothing.
    last_profile: Option<Profile>,
    last_warnings: Vec<Warning>,
    last_sleep_hold: Option<bool>,
}

impl Default for Journal {
    fn default() -> Self {
        Self::new()
    }
}

impl Journal {
    pub fn new() -> Self {
        Self::with_capacity(CAPACITY)
    }

    /// A journal that keeps `capacity` entries. At least one, so that a caller
    /// passing zero gets a journal that works rather than one that silently
    /// discards everything.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            next_seq: 1,
            last_profile: None,
            last_warnings: Vec::new(),
            last_sleep_hold: None,
        }
    }

    /// Note what is true now, recording only what changed.
    ///
    /// Returns how many entries were added, which is usually zero — that being
    /// the point.
    pub fn observe(&mut self, at: u64, decision: &Decision, warnings: &[Warning]) -> usize {
        let mut added = 0;

        // The first observation establishes a baseline rather than reporting a
        // change. "The profile changed to Balanced" as the first line after
        // every launch would be describing the application starting, not the
        // machine doing anything.
        match self.last_profile {
            Some(previous) if previous != decision.profile => {
                self.push(
                    at,
                    Event::ProfileChanged {
                        from: previous,
                        to: decision.profile,
                    },
                    decision.reason.clone(),
                );
                added += 1;
            }
            _ => {}
        }
        self.last_profile = Some(decision.profile);

        for warning in warnings {
            if !self.last_warnings.contains(warning) {
                self.push(
                    at,
                    Event::WarningRaised {
                        warning: warning.clone(),
                    },
                    message_of(warning),
                );
                added += 1;
            }
        }
        for warning in &self.last_warnings.clone() {
            if !warnings.contains(warning) {
                self.push(
                    at,
                    Event::WarningCleared {
                        warning: warning.clone(),
                    },
                    "The condition no longer applies.".to_string(),
                );
                added += 1;
            }
        }
        self.last_warnings = warnings.to_vec();

        if self.last_sleep_hold != Some(decision.prevent_sleep) {
            // Same first-observation rule: only a genuine change is an event.
            if self.last_sleep_hold.is_some() {
                self.push(
                    at,
                    Event::SleepHoldChanged {
                        held: decision.prevent_sleep,
                    },
                    decision.reason.clone(),
                );
                added += 1;
            }
            self.last_sleep_hold = Some(decision.prevent_sleep);
        }

        added
    }

    fn push(&mut self, at: u64, event: Event, reason: String) {
        self.entries.push_back(Entry {
            seq: self.next_seq,
            at,
            event,
            reason,
        });
        self.next_seq += 1;
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    /// Everything after `since`, and the cursor to pass next time.
    ///
    /// The cursor comes back even when nothing is new, so a window that polls
    /// on a timer does not have to special-case an empty answer.
    pub fn since(&self, since: u64) -> (Vec<Entry>, u64) {
        let entries: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.seq > since)
            .cloned()
            .collect();

        (entries, self.next_seq.saturating_sub(1))
    }

    /// The most recent entries, newest last. What a panel shows on open.
    pub fn recent(&self, limit: usize) -> Vec<Entry> {
        let skip = self.entries.len().saturating_sub(limit);
        self.entries.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The sentence a warning carries.
fn message_of(warning: &Warning) -> String {
    match warning {
        Warning::Thermal { message, .. } => message.clone(),
        Warning::LowBattery { message, .. } => message.clone(),
        Warning::Memory { message, .. } => message.clone(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    fn decision(profile: Profile, prevent_sleep: bool) -> Decision {
        Decision {
            profile,
            prevent_sleep,
            reason: "because".to_string(),
            changed: false,
        }
    }

    fn thermal(celsius: f32) -> Warning {
        Warning::Thermal {
            celsius,
            message: format!("{celsius} degrees"),
        }
    }

    /// The whole point. A manager ticking every two seconds must not produce
    /// two entries every two seconds.
    #[test]
    fn a_machine_doing_nothing_new_writes_nothing() {
        let mut journal = Journal::new();
        for tick in 0..100 {
            journal.observe(tick, &decision(Profile::Balanced, false), &[]);
        }
        assert!(journal.is_empty(), "{} entries for no change", journal.len());
    }

    /// Launching is not an event on the machine.
    #[test]
    fn the_first_observation_is_a_baseline_rather_than_a_change() {
        let mut journal = Journal::new();
        assert_eq!(journal.observe(0, &decision(Profile::Efficiency, true), &[]), 0);
        assert!(journal.is_empty());
    }

    #[test]
    fn a_profile_change_is_recorded_once_with_its_reason() {
        let mut journal = Journal::new();
        journal.observe(0, &decision(Profile::Balanced, false), &[]);
        assert_eq!(journal.observe(10, &decision(Profile::Performance, false), &[]), 1);

        // And not again while it stays there.
        for tick in 11..30 {
            journal.observe(tick, &decision(Profile::Performance, false), &[]);
        }

        assert_eq!(journal.len(), 1);
        let recent = journal.recent(1);
        let entry = recent.first().expect("the change was recorded");
        assert_eq!(
            entry.event,
            Event::ProfileChanged {
                from: Profile::Balanced,
                to: Profile::Performance
            }
        );
        assert_eq!(entry.reason, "because");
        assert_eq!(entry.at, 10);
    }

    /// A warning that stays true for an hour is one entry.
    #[test]
    fn a_standing_warning_is_recorded_once_and_cleared_once() {
        let mut journal = Journal::new();
        let hot = vec![thermal(95.0)];

        journal.observe(0, &decision(Profile::Balanced, false), &[]);
        assert_eq!(journal.observe(1, &decision(Profile::Balanced, false), &hot), 1);

        for tick in 2..1_000 {
            journal.observe(tick, &decision(Profile::Balanced, false), &hot);
        }
        assert_eq!(journal.len(), 1, "a standing warning was recorded again");

        assert_eq!(
            journal.observe(1_000, &decision(Profile::Balanced, false), &[]),
            1
        );
        assert_eq!(journal.len(), 2);
        let recent = journal.recent(1);
        assert!(matches!(
            recent.first().expect("the clear was recorded").event,
            Event::WarningCleared { .. }
        ));
    }

    /// A warning whose numbers move is still the same warning.
    #[test]
    fn a_warning_that_changes_degree_is_a_new_entry_rather_than_a_silent_update() {
        let mut journal = Journal::new();
        journal.observe(0, &decision(Profile::Balanced, false), &[thermal(95.0)]);
        journal.observe(1, &decision(Profile::Balanced, false), &[thermal(95.0)]);
        let before = journal.len();

        // 97° is not 95°, so it is raised and the old one clears: two entries,
        // and the record says what both readings were.
        journal.observe(2, &decision(Profile::Balanced, false), &[thermal(97.0)]);
        assert_eq!(journal.len(), before + 2);
    }

    #[test]
    fn sleep_being_held_and_released_is_recorded() {
        let mut journal = Journal::new();
        journal.observe(0, &decision(Profile::Balanced, false), &[]);
        assert_eq!(journal.observe(1, &decision(Profile::Balanced, true), &[]), 1);
        assert_eq!(journal.observe(2, &decision(Profile::Balanced, true), &[]), 0);
        assert_eq!(journal.observe(3, &decision(Profile::Balanced, false), &[]), 1);

        let recent = journal.recent(1);
        assert!(matches!(
            recent.first().expect("the release was recorded").event,
            Event::SleepHoldChanged { held: false }
        ));
    }

    #[test]
    fn a_cursor_returns_only_what_has_not_been_seen() {
        let mut journal = Journal::new();
        journal.observe(0, &decision(Profile::Balanced, false), &[]);
        journal.observe(1, &decision(Profile::Performance, false), &[]);

        let (first, cursor) = journal.since(0);
        assert_eq!(first.len(), 1);

        let (nothing, same) = journal.since(cursor);
        assert!(nothing.is_empty(), "a cursor returned an entry twice");
        assert_eq!(same, cursor, "the cursor moved with nothing to report");

        journal.observe(2, &decision(Profile::Efficiency, false), &[]);
        let (next, _) = journal.since(cursor);
        assert_eq!(next.len(), 1);
    }

    /// Sequences must not be reused once entries are dropped, or a polling
    /// window would be sent old entries as new ones.
    #[test]
    fn the_oldest_entries_are_dropped_without_reusing_their_sequences() {
        let mut journal = Journal::with_capacity(3);
        let profiles = [Profile::Balanced, Profile::Performance, Profile::Efficiency];

        journal.observe(0, &decision(Profile::Balanced, false), &[]);
        // Cycled rather than indexed, so every tick is a change and the count
        // below is the number of ticks rather than something to work out.
        for (tick, profile) in profiles.into_iter().cycle().skip(1).enumerate().take(19) {
            journal.observe(tick as u64 + 1, &decision(profile, false), &[]);
        }

        assert_eq!(journal.len(), 3);
        let sequences: Vec<u64> = journal.recent(3).iter().map(|entry| entry.seq).collect();
        assert_eq!(sequences, vec![17, 18, 19]);
        assert!(sequences.is_sorted_by(|left, right| left < right));
    }

    #[test]
    fn a_zero_capacity_journal_still_keeps_something() {
        let mut journal = Journal::with_capacity(0);
        journal.observe(0, &decision(Profile::Balanced, false), &[]);
        journal.observe(1, &decision(Profile::Performance, false), &[]);
        assert_eq!(journal.len(), 1);
    }
}
