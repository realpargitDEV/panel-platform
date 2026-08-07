//! Whether this machine can take one more project, and what it is carrying now.
//!
//! # Why this exists
//!
//! A container carries limits the daemon enforces. Twenty containers on a
//! machine that cannot hold twenty get OOM-killed one at a time, which is bad
//! but bounded, and Docker does the bounding.
//!
//! A host process has no such bound. The workspace forbids `unsafe`, a Windows
//! memory cap needs a Job Object, and a Job Object needs FFI — so for a host
//! project this application **cannot enforce a limit at all**. What is left is
//! to decline to start what will not fit, and to report honestly once it has.
//!
//! That asymmetry is the whole design. Admission control is the mechanism
//! because enforcement is unavailable, and refusal is the behaviour at the limit
//! because with nothing to catch an overcommit, allowing one means the machine
//! swaps and the user loses work in every application, not only this one.
//!
//! The arithmetic is pure and lives in [`admission`], which is what makes it
//! testable: the interesting cases are all "1 GB free, wants 2 GB", and none of
//! them are reachable by arranging for the development machine to actually run
//! out of memory.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod admission;
pub mod usage;

pub use admission::{admit, Admission, Reserve, Shortfall};
pub use usage::{MachineUsage, RunningProject, SystemUsage, Usage, UsageSource};
