//! Running a project as an ordinary process on this machine.
//!
//! The alternative to a container. A project in host mode is started from the
//! same [`RuntimeSpec`] a container project is — `install_command`,
//! `build_command`, `start_command` — because none of those fields ever
//! mentioned Docker. What differs is the substrate underneath them.
//!
//! What a container gives and this does not: filesystem isolation, network
//! isolation, a non-root user, and a daemon that outlives the application. Host
//! mode is therefore never selected without the user saying so, and the code
//! that offers it is responsible for saying what is given up.
//!
//! This crate depends on neither `docker-manager` nor `api-types`, for the same
//! reason `detection` depends on neither: it should be possible to reason about
//! how a process is started without also holding the container model, or the
//! wire format, in mind.
//!
//! **Verified on Windows only.** The machine this was written on has no Linux
//! and no macOS. Anything below that differs per platform is marked where it
//! appears.
//!
//! [`RuntimeSpec`]: https://docs.rs/project-host-database

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod command;
pub mod health;
pub mod output;
pub mod probe;
pub mod supervisor;

pub use command::{split_command, start_command, CommandError, CommandInputs, ProcessCommand};
pub use health::{check, Check, Health};
pub use output::{log_path, Tail};
pub use probe::{candidates_for, probe, ExecutableResolver, Toolchain};
pub use supervisor::{
    run_step, start, HealthPolicy, HostError, HostObserved, HostStatus, SupervisorConfig,
    SupervisorHandle, DEFAULT_GRACE, MAX_RESTARTS,
};
