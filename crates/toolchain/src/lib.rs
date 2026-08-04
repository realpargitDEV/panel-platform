//! What a project's language needs on this machine, and how to get it there.
//!
//! `host-runner`'s [`probe`] answers whether a toolchain is present, against a
//! resolver the tests supply. It cannot answer what to do when the answer is
//! no: `Toolchain::Missing { looked_for: ["python3", "python"] }` names what
//! was looked for and nothing about how to fix it. This crate is the other
//! half — a catalogue of what to install, and a plan for installing it.
//!
//! Everything except [`execute`] is a pure function of its arguments. The
//! platform, the package manager and the probe results are all passed in, so
//! every platform's plan is checked on every host. That rule is not stylistic:
//! this project once shipped an application that had never started for a
//! non-root Linux user, because the only test that would have caught it could
//! not run on the machine the project is developed on.
//!
//! It depends on `project-host-platform` and nothing else — not
//! `docker-manager`, not `api-types` — for the reason `compatibility` and
//! `host-runner` both give: it should be possible to reason about whether a
//! machine can run a language without also holding the container model, or the
//! wire format, in mind.
//!
//! [`probe`]: https://docs.rs/project-host-host-runner

// Tests are allowed to unwrap and slice; production paths in this workspace are
// not. A panic in a test is a failed test — in the application it is a stopped
// project.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod blocker;
pub mod catalog;
pub mod execute;
pub mod plan;
pub mod refresh;

pub use blocker::Blocker;
pub use catalog::{catalog, prerequisite, prerequisites, spec_for, Prerequisite, ToolchainSpec};
pub use plan::{elevate, plan, Host, Plan, ProjectInstall, Step};
pub use refresh::{find_executable, merged_path, suffixes_for};
