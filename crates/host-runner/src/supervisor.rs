//! One running project, and everything that has to be true while it runs.
//!
//! A container is supervised by a daemon that outlives this application. A host
//! process is not: it is a child of this process, and if nothing here watches it
//! then nothing does. So each running project gets one task that owns its child,
//! pumps its output, notices when it exits, and can be told to stop.
//!
//! The status this reports is always **observed**. Nothing here writes "running"
//! because it asked for a start; it writes what the process actually did. That
//! rule belongs to the lifecycle layer, and a supervisor that guessed would make
//! it unenforceable.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::command::ProcessCommand;
use crate::output::{pump, Tail};

/// How long a freshly spawned process is watched before it is called started.
///
/// A process that is going to fail on a missing module, a syntax error or a
/// bound port does it immediately. Waiting this long turns "started, then FAILED
/// a moment later with nothing to read" into a start that fails with the child's
/// own words attached. It is a real delay on every host start, and it buys the
/// single most common failure being reported properly.
const SETTLE: Duration = Duration::from_millis(600);

/// How long a stop waits before it stops asking.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStatus {
    Running,
    Stopped,
    Failed,
}

/// What is true about a supervised project right now.
///
/// This crate has its own status words rather than `api-types`' `ProjectStatus`
/// for the reason given in the crate documentation: it should be possible to
/// reason about running a process without also holding the wire format in mind.
/// The translation happens one layer up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostObserved {
    pub status: HostStatus,
    pub pid: Option<u32>,
    pub exit_code: Option<i64>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("could not start `{program}`: {source}")]
    SpawnFailed {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("`{program}` exited immediately with code {code}{}", tail.as_ref().map(|text| format!("\n{text}")).unwrap_or_default())]
    ExitedImmediately {
        program: String,
        code: i64,
        tail: Option<String>,
    },

    #[error("the {phase} step failed with code {code}{}", tail.as_ref().map(|text| format!("\n{text}")).unwrap_or_default())]
    StepFailed {
        phase: &'static str,
        code: i64,
        tail: Option<String>,
    },

    #[error("could not end the process: {0}")]
    CouldNotStop(String),
}

/// The mutable half of a supervisor, shared with the task that waits on the
/// child.
#[derive(Debug)]
struct Shared {
    status: HostStatus,
    pid: Option<u32>,
    exit_code: Option<i64>,
    failure_reason: Option<String>,
    /// Set when a stop was asked for, so the waiter can tell a requested exit
    /// from a crash. Without it, every clean stop would be recorded as a
    /// failure the moment the exit code was non-zero — which on Windows it is,
    /// because the process was force-killed.
    stopping: bool,
}

/// A handle to one running project.
#[derive(Debug, Clone)]
pub struct SupervisorHandle {
    shared: Arc<Mutex<Shared>>,
    tail: Tail,
}

impl SupervisorHandle {
    fn state(&self) -> MutexGuard<'_, Shared> {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What is true right now.
    pub fn observe(&self) -> HostObserved {
        let state = self.state();
        HostObserved {
            status: state.status,
            pid: state.pid,
            exit_code: state.exit_code,
            failure_reason: state.failure_reason.clone(),
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.state().pid
    }

    /// The last lines the project printed.
    pub fn tail(&self) -> Option<String> {
        self.tail.text()
    }

    /// Ask the project to stop, and end it if it will not.
    pub async fn stop(&self, grace: Duration) -> Result<(), HostError> {
        let Some(pid) = self.mark_stopping() else {
            return Ok(());
        };
        project_host_platform::terminate_tree(pid, grace)
            .await
            .map_err(|error| HostError::CouldNotStop(error.to_string()))
    }

    /// End the project now.
    pub async fn kill(&self) -> Result<(), HostError> {
        let Some(pid) = self.mark_stopping() else {
            return Ok(());
        };
        project_host_platform::kill_tree(pid)
            .await
            .map_err(|error| HostError::CouldNotStop(error.to_string()))
    }

    /// Record that a stop was requested, and answer with the pid to act on.
    ///
    /// `None` means there is nothing to stop, which is success.
    fn mark_stopping(&self) -> Option<u32> {
        let mut state = self.state();
        state.stopping = true;
        state.pid
    }
}

/// Start a project and supervise it.
///
/// Returns once the process has been watched for [`SETTLE`] without dying. A
/// process that exits within that window with a non-zero code is a failed start
/// reported with its own output, not a project that briefly ran.
pub async fn start(
    command: ProcessCommand,
    log_path: PathBuf,
) -> Result<SupervisorHandle, HostError> {
    let program = command.program.clone();
    let mut child = spawn(&command)?;

    let pid = child.id();
    let tail = pump(child.stdout.take(), child.stderr.take(), log_path);

    let shared = Arc::new(Mutex::new(Shared {
        status: HostStatus::Running,
        pid,
        exit_code: None,
        failure_reason: None,
        stopping: false,
    }));

    let handle = SupervisorHandle {
        shared: Arc::clone(&shared),
        tail: tail.clone(),
    };

    // The waiter owns the child from here. Nothing else may wait on it: two
    // waiters means one of them gets "no such child" and reports a healthy
    // project as vanished.
    let waiter_tail = tail.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        let mut state = shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let code = status.as_ref().ok().and_then(|status| status.code());
        state.exit_code = code.map(i64::from);
        state.pid = None;

        if state.stopping {
            // Asked to stop, and it stopped. On Windows this arrives as a
            // non-zero code because the tree was force-killed, which is not a
            // failure and must not be recorded as one.
            state.status = HostStatus::Stopped;
        } else if code == Some(0) {
            state.status = HostStatus::Stopped;
        } else {
            state.status = HostStatus::Failed;
            state.failure_reason = waiter_tail.text();
        }
    });

    // Watch the settle window, checking often enough that a healthy start is
    // not delayed by the full period for no reason.
    let deadline = std::time::Instant::now() + SETTLE;
    while std::time::Instant::now() < deadline {
        let observed = handle.observe();
        if observed.status != HostStatus::Running {
            if observed.status == HostStatus::Failed {
                return Err(HostError::ExitedImmediately {
                    program,
                    code: observed.exit_code.unwrap_or(-1),
                    tail: handle.tail(),
                });
            }
            // Exited cleanly inside the window. A one-shot command run as a
            // project is unusual but not wrong, and it is not a failure.
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    Ok(handle)
}

/// Run a command to completion — an install or a build step.
///
/// Separate from [`start`] because these are expected to end, and their failure
/// has to fail the start with their output attached rather than leaving a
/// project that will not run and no reason why.
pub async fn run_step(
    phase: &'static str,
    command: ProcessCommand,
    log_path: PathBuf,
) -> Result<(), HostError> {
    let mut child = spawn(&command)?;
    let tail = pump(child.stdout.take(), child.stderr.take(), log_path);

    let status = child
        .wait()
        .await
        .map_err(|source| HostError::SpawnFailed {
            program: command.program.clone(),
            source,
        })?;

    if status.success() {
        return Ok(());
    }

    // Let the pumps drain before the tail is read, or the reason the step
    // failed is missing from the message reporting that it failed.
    settle_output(&tail).await;

    Err(HostError::StepFailed {
        phase,
        code: status.code().map_or(-1, i64::from),
        tail: tail.text(),
    })
}

/// Build the child process, as a group leader, with its output piped.
fn spawn(command: &ProcessCommand) -> Result<tokio::process::Child, HostError> {
    let mut process = tokio::process::Command::new(&command.program);
    process
        .args(&command.args)
        .current_dir(&command.cwd)
        .envs(&command.env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Without this, stopping the project leaves whatever it spawned running and
    // holding the port.
    project_host_platform::as_group_leader(&mut process);

    process.spawn().map_err(|source| HostError::SpawnFailed {
        program: command.program.clone(),
        source,
    })
}

/// Give the output pumps a moment to catch up.
///
/// The pumps are separate tasks, so a child can exit before its last line has
/// been read. Reading the tail immediately would drop exactly the line that
/// explains what went wrong.
async fn settle_output(tail: &Tail) {
    for _ in 0..40 {
        if tail.text().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn command(program: &str, args: &[&str], cwd: &std::path::Path) -> ProcessCommand {
        ProcessCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            cwd: cwd.to_path_buf(),
            env: BTreeMap::new(),
        }
    }

    /// A command that keeps running until it is stopped.
    fn long_running(cwd: &std::path::Path) -> ProcessCommand {
        #[cfg(windows)]
        return command("cmd", &["/C", "ping -n 60 127.0.0.1 >NUL"], cwd);
        #[cfg(unix)]
        return command("sh", &["-c", "sleep 60"], cwd);
    }

    #[tokio::test]
    async fn a_project_that_starts_is_observed_running() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(
            long_running(directory.path()),
            directory.path().join("run.log"),
        )
        .await
        .expect("start");

        let observed = handle.observe();
        assert_eq!(observed.status, HostStatus::Running);
        assert!(observed.pid.is_some(), "a running project has a pid");

        handle.kill().await.expect("kill");
    }

    /// The case log capture exists for. A project that dies on startup must
    /// fail the start *with its own words*, not leave FAILED and nothing to
    /// read.
    #[tokio::test]
    async fn a_project_that_dies_at_once_fails_the_start_with_its_output() {
        let directory = tempfile::tempdir().expect("temp dir");

        #[cfg(windows)]
        let dying = command(
            "cmd",
            &["/C", "echo Error: listen EADDRINUSE 1>&2 && exit 1"],
            directory.path(),
        );
        #[cfg(unix)]
        let dying = command(
            "sh",
            &["-c", "echo 'Error: listen EADDRINUSE' 1>&2; exit 1"],
            directory.path(),
        );

        let error = start(dying, directory.path().join("run.log"))
            .await
            .expect_err("a project that exits 1 immediately has not started");

        match error {
            HostError::ExitedImmediately { code, tail, .. } => {
                assert_eq!(code, 1);
                assert!(
                    tail.unwrap_or_default().contains("EADDRINUSE"),
                    "the failure has to carry the reason"
                );
            }
            other => panic!("expected ExitedImmediately, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_program_that_does_not_exist_names_itself() {
        let directory = tempfile::tempdir().expect("temp dir");
        let error = start(
            command("definitely-not-a-real-program", &[], directory.path()),
            directory.path().join("run.log"),
        )
        .await
        .expect_err("nothing to spawn");

        match error {
            HostError::SpawnFailed { program, .. } => {
                assert_eq!(program, "definitely-not-a-real-program");
            }
            other => panic!("expected SpawnFailed, got {other:?}"),
        }
    }

    /// A requested stop is not a failure, even though on Windows the process
    /// is force-killed and exits non-zero.
    #[tokio::test]
    async fn stopping_a_project_records_it_stopped_rather_than_failed() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(
            long_running(directory.path()),
            directory.path().join("run.log"),
        )
        .await
        .expect("start");

        handle.stop(Duration::from_secs(5)).await.expect("stop");

        for _ in 0..100 {
            if handle.observe().status != HostStatus::Running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let observed = handle.observe();
        assert_eq!(
            observed.status,
            HostStatus::Stopped,
            "a stop the user asked for is not a failure"
        );
        assert!(observed.failure_reason.is_none());
    }

    #[tokio::test]
    async fn a_failing_build_step_carries_its_output() {
        let directory = tempfile::tempdir().expect("temp dir");

        #[cfg(windows)]
        let failing = command(
            "cmd",
            &["/C", "echo missing dependency 1>&2 && exit 3"],
            directory.path(),
        );
        #[cfg(unix)]
        let failing = command(
            "sh",
            &["-c", "echo 'missing dependency' 1>&2; exit 3"],
            directory.path(),
        );

        let error = run_step("install", failing, directory.path().join("run.log"))
            .await
            .expect_err("exit 3 is a failure");

        match error {
            HostError::StepFailed { phase, code, tail } => {
                assert_eq!(phase, "install");
                assert_eq!(code, 3);
                assert!(tail.unwrap_or_default().contains("missing dependency"));
            }
            other => panic!("expected StepFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_step_that_succeeds_says_nothing() {
        let directory = tempfile::tempdir().expect("temp dir");
        #[cfg(windows)]
        let ok = command("cmd", &["/C", "exit 0"], directory.path());
        #[cfg(unix)]
        let ok = command("sh", &["-c", "exit 0"], directory.path());

        run_step("build", ok, directory.path().join("run.log"))
            .await
            .expect("exit 0 is success");
    }

    /// Stopping something that has already gone is the state the caller asked
    /// for, not an error to report.
    #[tokio::test]
    async fn stopping_a_project_twice_is_success() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(
            long_running(directory.path()),
            directory.path().join("run.log"),
        )
        .await
        .expect("start");

        handle.kill().await.expect("first");
        handle.kill().await.expect("second");
    }
}
