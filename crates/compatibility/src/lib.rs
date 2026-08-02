//! Whether this machine can run Docker, how well, and which one.
//!
//! Everything here is a pure function of a [`SystemSnapshot`]. This crate does
//! no I/O, spawns no process and reads no file, which is what lets every
//! decision be tested against a machine that was constructed rather than one
//! that happens to be running the test.
//!
//! It depends on `project-host-platform` for the snapshot type and on nothing
//! else — not `docker-manager`, not `api-types` — for the same reason
//! `detection` and `host-runner` do: it should be possible to reason about
//! whether a machine can run Docker without also holding the container model,
//! or the wire format, in mind.
//!
//! [`SystemSnapshot`]: project_host_platform::SystemSnapshot

// Tests are allowed to unwrap and slice; production paths in this workspace are
// not. A panic in a test is a failed test — in the agent it is a stopped service.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod machines;
