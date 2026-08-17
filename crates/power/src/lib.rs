//! Power behaviour for a machine that is also somebody's computer.
//!
//! Two halves. [`monitor`] reports what the machine is doing, sampling once on
//! a timer so that every reader shares one cost. [`policy`] decides what should
//! follow from that, as a pure function of a described machine, so the
//! interesting cases are tests rather than something only observable by
//! unplugging a laptop.
//!
//! Neither half can stop a project. The levers are the operating system's sleep
//! behaviour and process scheduling priority, and where saving power conflicts
//! with keeping a project available, the project wins.

pub mod journal;
pub mod manager;
pub mod monitor;
pub mod policy;
pub mod power;

pub use manager::{PowerManager, RunningProject, Snapshot};
pub use policy::{Mode, Profile};
pub use power::Priority;
