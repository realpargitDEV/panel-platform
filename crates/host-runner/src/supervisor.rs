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
use crate::health::{Check, Health};
use crate::output::{pump, pump_into, Pumps, Tail};

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

/// How many times a crashed project is restarted before it is left alone.
///
/// A project that cannot start does not start no matter how often it is asked.
/// Past this point the restarts are noise in the log and load on the machine,
/// and the status the user needs to see is `FAILED` with the last output.
pub const MAX_RESTARTS: u32 = 5;

/// The first backoff, doubled per attempt: 1s, 2s, 4s, 8s, 16s.
const FIRST_BACKOFF: Duration = Duration::from_secs(1);

/// How long an attempt has to survive before it stops counting as part of a
/// crash loop.
///
/// Without this the restart counter only ever rises, so a project that runs
/// perfectly for three weeks and then crashes five times over those weeks is
/// declared a crash loop and abandoned. A crash loop is a *rate*, not a total:
/// five failures in a minute is one, five failures in a month is five ordinary
/// crashes. An attempt that stayed up this long has demonstrated it can, so the
/// count starts again from there.
const STABLE_RUN: Duration = Duration::from_secs(60);

/// How long an exit waits for the child's last output before reporting it.
///
/// Short, because it delays every crash report, and bounded because a child
/// that left a grandchild holding its pipes never closes them.
const DRAIN: Duration = Duration::from_secs(2);

/// What to run, where to log it, and what to do while it runs.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub command: ProcessCommand,
    pub log_path: PathBuf,
    /// `None` means no health check is configured, which is not the same as one
    /// that never passes.
    pub health: Option<HealthPolicy>,
    /// Whether a crash is followed by another attempt.
    pub restart_on_crash: bool,
    /// The first backoff, doubled per attempt.
    ///
    /// Configurable only so the restart cap can be tested. Exercising five
    /// attempts at the real backoff costs thirty-one seconds, which is the kind
    /// of test that gets deleted rather than run — and the cap is the behaviour
    /// most worth pinning, because without it a project that cannot start
    /// becomes a machine that cannot idle.
    pub backoff: Duration,
    /// How long an attempt must survive before the crash-loop count resets.
    ///
    /// Configurable for the same reason `backoff` is: pinning "a project that
    /// ran for a while is not in a crash loop" at the real sixty seconds would
    /// be a minute-long test.
    pub stable_run: Duration,
}

impl SupervisorConfig {
    /// The plain case: run it, log it, ask nothing, restart nothing.
    pub fn new(command: ProcessCommand, log_path: PathBuf) -> Self {
        Self {
            command,
            log_path,
            health: None,
            restart_on_crash: false,
            backoff: FIRST_BACKOFF,
            stable_run: STABLE_RUN,
        }
    }
}

/// How often to ask a project whether it is working, and how long to wait
/// before starting to ask.
#[derive(Debug, Clone)]
pub struct HealthPolicy {
    pub check: Check,
    pub interval: Duration,
    /// Grace at the beginning. A project that takes ten seconds to bind its
    /// port is not unhealthy for those ten seconds; it is starting.
    pub start_period: Duration,
}

/// What a supervised project is doing.
///
/// `Crashed` and `Failed` are separate for the reason `ProjectStatus` keeps
/// them separate one layer up: a project that ran and then died is a different
/// situation, with a different fix, from one that could not be run at all.
/// Here the distinction is precise rather than a judgement — `Crashed` is only
/// ever written for a child that was spawned, survived its settle window, and
/// then exited without being asked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStatus {
    Running,
    Stopped,
    /// Was up, then exited on its own with a non-zero code.
    Crashed,
    /// Could not be run: the program vanished between attempts, or the crash
    /// loop reached its cap and supervision gave up.
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
    pub health: Health,
    pub pid: Option<u32>,
    pub exit_code: Option<i64>,
    pub failure_reason: Option<String>,
    pub restarts: u32,
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
    health: Health,
    pid: Option<u32>,
    exit_code: Option<i64>,
    failure_reason: Option<String>,
    /// Set when a stop was asked for, so the supervisor can tell a requested
    /// exit from a crash. Without it, every clean stop would be recorded as a
    /// failure the moment the exit code was non-zero — which on Windows it is,
    /// because the process was force-killed.
    stopping: bool,
    /// How many times this project has been restarted after crashing, in
    /// total. Reported, and only ever rises — it is the history, not the
    /// crash-loop measure.
    restarts: u32,
    /// How many attempts have failed *in a row* without one of them surviving
    /// [`SupervisorConfig::stable_run`]. This is what the cap is measured
    /// against, because a crash loop is a rate and not a total.
    consecutive_failures: u32,
    /// Set once an exit has been fully written down, output included.
    ///
    /// The status flips as soon as the child is reaped, because a start waiting
    /// on the settle window must not be told a dead project is alive. The
    /// child's last words arrive a moment later. This distinguishes "failed,
    /// reason still being collected" from "failed, and it said nothing".
    exit_recorded: bool,
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
            health: state.health.clone(),
            pid: state.pid,
            exit_code: state.exit_code,
            failure_reason: state.failure_reason.clone(),
            restarts: state.restarts,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.state().pid
    }

    /// The last lines the project printed.
    pub fn tail(&self) -> Option<String> {
        self.tail.text()
    }

    /// This project's console: the lines after `seq`, and the cursor to ask
    /// with next time.
    ///
    /// Per handle, which is per project, which is the whole point — five
    /// projects running at once have five of these and nothing joins them.
    pub fn logs_since(&self, seq: u64) -> (Vec<crate::output::LogLine>, u64) {
        self.tail.since(seq)
    }

    /// Everything currently retained for this project.
    pub fn logs(&self) -> Vec<crate::output::LogLine> {
        self.tail.all()
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

    /// Turn a start that died in its settle window into the error to report,
    /// and take the supervision down with it.
    ///
    /// Both halves matter. A failed start returns an error and no handle, so
    /// anything the supervision task did afterwards — restarting on the
    /// project's restart policy, in particular — would be work nobody could
    /// observe and nobody could stop. Ending it here is what keeps a project
    /// that cannot start from becoming a process the registry has never heard
    /// of, respawning against a port the user thinks is free.
    async fn failed_start(&self, program: String) -> HostError {
        // Wait for the child's own words before ending anything, or the report
        // says only that it failed.
        self.await_exit_recorded().await;
        let observed = self.observe();
        let _ = self.kill().await;

        HostError::ExitedImmediately {
            program,
            code: observed.exit_code.unwrap_or(-1),
            tail: self.tail(),
        }
    }

    /// Wait, briefly, for an exit to be written down in full.
    ///
    /// The status flips the moment the child is reaped; its output lands once
    /// the pumps have drained. Bounded by the same limit the drain itself uses,
    /// so a pipe held open by a grandchild delays the report rather than
    /// hanging the start.
    async fn await_exit_recorded(&self) {
        let deadline = std::time::Instant::now() + DRAIN + Duration::from_secs(1);
        while std::time::Instant::now() < deadline {
            if self.state().exit_recorded {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Start a project and supervise it.
///
/// Returns once the process has been watched for [`SETTLE`] without dying. A
/// process that exits within that window with a non-zero code is a failed start
/// reported with its own output, not a project that briefly ran.
pub async fn start(config: SupervisorConfig) -> Result<SupervisorHandle, HostError> {
    let program = config.command.program.clone();
    let mut child = spawn(&config.command)?;

    let pid = child.id();
    let (tail, pumps) = pump(
        child.stdout.take(),
        child.stderr.take(),
        config.log_path.clone(),
    );

    let shared = Arc::new(Mutex::new(Shared {
        status: HostStatus::Running,
        health: Health::None,
        pid,
        exit_code: None,
        failure_reason: None,
        stopping: false,
        restarts: 0,
        consecutive_failures: 0,
        exit_recorded: false,
    }));

    let handle = SupervisorHandle {
        shared: Arc::clone(&shared),
        tail: tail.clone(),
    };

    // The console reads as one story or it reads as nothing. Without these
    // notes a restart looks like output that stops and then resumes for no
    // reason, and a crash looks like output that simply ends.
    tail.note(&format!(
        "started {} (pid {})",
        config.command.program,
        pid.map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));

    // The supervision task owns the child from here. Nothing else may wait on
    // it: two waiters means one of them gets "no such child" and reports a
    // healthy project as vanished.
    tokio::spawn(supervise(shared, config, tail.clone(), child, pumps));

    // Watch the settle window, checking often enough that a healthy start is
    // not delayed by the full period for no reason.
    let deadline = std::time::Instant::now() + SETTLE;
    while std::time::Instant::now() < deadline {
        let observed = handle.observe();
        if observed.status != HostStatus::Running {
            // Anything but a clean exit inside the settle window is a start
            // that did not take. The supervision task will have written
            // CRASHED, because from inside the loop every unasked-for exit
            // looks the same — but a child that never survived its first
            // second did not crash, it failed to start, and that is what the
            // caller is told.
            if observed.status != HostStatus::Stopped {
                return Err(handle.failed_start(program).await);
            }
            // Exited cleanly inside the window. A one-shot command run as a
            // project is unusual but not wrong, and it is not a failure.
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    Ok(handle)
}

/// Own one project for as long as it runs.
///
/// Three things happen here and nowhere else: the child is waited on, its health
/// is polled, and a crash is followed by another attempt. They share a task
/// because they share the child — a health poller in a separate task would have
/// to be told, race-free, each time the child was replaced by a restart.
async fn supervise(
    shared: Arc<Mutex<Shared>>,
    config: SupervisorConfig,
    tail: Tail,
    mut child: tokio::process::Child,
    mut pumps: Pumps,
) {
    // When the attempt currently being waited on was spawned. Compared against
    // `stable_run` on exit, which is what turns the crash-loop count from a
    // total into a rate.
    let mut attempt_started = std::time::Instant::now();

    loop {
        let exit = wait_while_polling_health(&shared, &config, &mut child).await;
        let code = exit.ok().and_then(|status| status.code()).map(i64::from);
        let ran_for = attempt_started.elapsed();

        // The exit itself is recorded at once, before the output is collected.
        // A start watching the settle window must not be told a project that
        // has already died is still running just because its last line has not
        // arrived yet.
        {
            let mut state = lock(&shared);
            state.exit_code = code;
            state.pid = None;
            state.health = Health::None;

            if state.stopping {
                // Asked to stop, and it stopped. On Windows this arrives as a
                // non-zero code because the tree was force-killed, which is not
                // a failure and must not be recorded as one.
                state.status = HostStatus::Stopped;
                state.exit_recorded = true;
                drop(state);
                tail.note("stopped as requested");
                return;
            }
            if code == Some(0) {
                state.status = HostStatus::Stopped;
                state.exit_recorded = true;
                drop(state);
                tail.note("exited with code 0");
                return;
            }

            // It was up and it is not any more, without being asked. That is a
            // crash, whatever happens next.
            state.status = HostStatus::Crashed;

            // An attempt that stayed up is evidence the project *can* run, so
            // the loop counter starts again from here. Without this a project
            // that crashes once a week is abandoned on its sixth week.
            if ran_for >= config.stable_run {
                state.consecutive_failures = 0;
            }
        }

        // Now the child's last words, which are the whole reason a failure is
        // readable. The pumps are separate tasks, so this waits for them rather
        // than reading a tail they have not finished filling.
        pumps.drained(DRAIN).await;

        tail.note(&format!(
            "exited unexpectedly with code {} after {}s",
            code.map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            ran_for.as_secs()
        ));

        let backoff = {
            let mut state = lock(&shared);
            state.failure_reason = tail.text();
            state.exit_recorded = true;

            if !config.restart_on_crash {
                drop(state);
                tail.note("not restarting: this project's restart policy is off");
                return;
            }
            if state.consecutive_failures >= MAX_RESTARTS {
                // Give up, and say so in the reason rather than leaving the
                // user to infer it from a restart count that stopped moving.
                // The status becomes FAILED rather than staying CRASHED: this
                // project is not coming back on its own, and that is a
                // different thing to report.
                state.status = HostStatus::Failed;
                state.failure_reason = Some(format!(
                    "gave up after {MAX_RESTARTS} restarts in quick succession\n{}",
                    tail.text().unwrap_or_default()
                ));
                drop(state);
                tail.note(&format!(
                    "crash loop: {MAX_RESTARTS} restarts in quick succession, all of them \
                     ending the same way. Not restarting again — fix the project and start it."
                ));
                return;
            }

            // Backoff, doubling with the *consecutive* failures. Computed under
            // the same lock that read the count, so the two cannot disagree.
            // `saturating_mul` rather than a shift, so a future larger cap
            // cannot overflow the duration.
            config
                .backoff
                .saturating_mul(1u32 << state.consecutive_failures.min(16))
        };

        tail.note(&format!("restarting in {}ms", backoff.as_millis()));

        if !sleep_unless_stopping(&shared, backoff).await {
            lock(&shared).status = HostStatus::Stopped;
            tail.note("stopped while waiting to restart");
            return;
        }

        match spawn(&config.command) {
            Ok(mut replacement) => {
                let pid = replacement.id();
                // Fresh pumps per attempt, because the previous child's pipes
                // are closed — but into the same tail, so the excerpt shown for
                // the next failure is written by the child that failed rather
                // than by the first one.
                pumps = pump_into(
                    replacement.stdout.take(),
                    replacement.stderr.take(),
                    config.log_path.clone(),
                    tail.clone(),
                );
                attempt_started = std::time::Instant::now();

                let mut state = lock(&shared);
                state.status = HostStatus::Running;
                state.pid = pid;
                state.exit_code = None;
                state.failure_reason = None;
                state.restarts = state.restarts.saturating_add(1);
                state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                state.exit_recorded = false;
                let attempt = state.restarts;
                drop(state);

                tail.note(&format!(
                    "restarted (attempt {attempt}, pid {})",
                    pid.map(|pid| pid.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ));
                child = replacement;
            }
            Err(error) => {
                // The program vanished between attempts — uninstalled, or the
                // directory was removed. Another attempt would fail the same
                // way. This is a failure to *run*, not a crash of something
                // that ran.
                let mut state = lock(&shared);
                state.status = HostStatus::Failed;
                state.failure_reason = Some(error.to_string());
                state.exit_recorded = true;
                drop(state);
                tail.note(&format!("could not be restarted: {error}"));
                return;
            }
        }
    }
}

/// Wait for the child, asking after its health while waiting.
///
/// `Child::wait` is cancel-safe, which is what makes it usable as a `select!`
/// branch: a health tick that fires first does not lose the wait.
async fn wait_while_polling_health(
    shared: &Arc<Mutex<Shared>>,
    config: &SupervisorConfig,
    child: &mut tokio::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    let Some(policy) = config.health.as_ref() else {
        return child.wait().await;
    };

    let started = tokio::time::Instant::now();
    let mut ticker = tokio::time::interval_at(
        started + policy.start_period.max(Duration::from_millis(1)),
        policy.interval.max(Duration::from_millis(100)),
    );

    loop {
        tokio::select! {
            status = child.wait() => return status,
            _ = ticker.tick() => {
                let health = crate::health::check(&policy.check).await;
                let mut state = lock(shared);
                // Only while it is up. A check that completed just as the
                // process exited must not overwrite the exit with a verdict
                // about a process that is no longer there.
                if state.status == HostStatus::Running {
                    state.health = health;
                }
            }
        }
    }
}

/// Sleep, unless a stop is requested first. `false` means a stop was requested.
async fn sleep_unless_stopping(shared: &Arc<Mutex<Shared>>, total: Duration) -> bool {
    let deadline = std::time::Instant::now() + total;
    while std::time::Instant::now() < deadline {
        if lock(shared).stopping {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    !lock(shared).stopping
}

/// Lock, recovering from poisoning. A panicked supervision task must not make
/// every subsequent read of a project's state panic too.
fn lock(shared: &Arc<Mutex<Shared>>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    let (tail, mut pumps) = pump(child.stdout.take(), child.stderr.take(), log_path);

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
    pumps.drained(DRAIN).await;

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
        let handle = start(SupervisorConfig::new(
            long_running(directory.path()),
            directory.path().join("run.log"),
        ))
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

        let error = start(SupervisorConfig::new(
            dying,
            directory.path().join("run.log"),
        ))
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
        let error = start(SupervisorConfig::new(
            command("definitely-not-a-real-program", &[], directory.path()),
            directory.path().join("run.log"),
        ))
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
        let handle = start(SupervisorConfig::new(
            long_running(directory.path()),
            directory.path().join("run.log"),
        ))
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

    /// A command that runs briefly and then crashes, so the supervisor has
    /// something to keep restarting. It survives the settle window first, so
    /// the start succeeds and the crash is a crash rather than a failed start.
    fn crashes_after_a_moment(cwd: &std::path::Path) -> ProcessCommand {
        #[cfg(windows)]
        return command("cmd", &["/C", "ping -n 2 127.0.0.1 >NUL && exit 9"], cwd);
        #[cfg(unix)]
        return command("sh", &["-c", "sleep 1; exit 9"], cwd);
    }

    /// Without a restart policy, a crash is simply a crash.
    #[tokio::test]
    async fn a_crash_is_not_restarted_unless_asked() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(SupervisorConfig::new(
            crashes_after_a_moment(directory.path()),
            directory.path().join("run.log"),
        ))
        .await
        .expect("start");

        wait_until(&handle, |observed| observed.status != HostStatus::Running).await;

        let observed = handle.observe();
        assert_eq!(
            observed.status,
            HostStatus::Crashed,
            "a project that ran and then died crashed; it did not fail to run"
        );
        assert_eq!(observed.exit_code, Some(9));
        assert_eq!(observed.restarts, 0, "nothing asked for a restart");
    }

    /// A program that dies instantly never reaches the restart loop at all: it
    /// fails inside the settle window, so the *start* fails. Enabling restarts
    /// must not change that — a project that was never up has nothing to
    /// restart.
    #[tokio::test]
    async fn a_program_that_exits_at_once_fails_the_start_even_with_restarts_on() {
        let directory = tempfile::tempdir().expect("temp dir");

        #[cfg(windows)]
        let always_fails = command("cmd", &["/C", "exit 4"], directory.path());
        #[cfg(unix)]
        let always_fails = command("sh", &["-c", "exit 4"], directory.path());

        let error = start(SupervisorConfig {
            restart_on_crash: true,
            ..SupervisorConfig::new(always_fails, directory.path().join("run.log"))
        })
        .await
        .expect_err("a program that exits at once has not started");

        assert!(matches!(
            error,
            HostError::ExitedImmediately { code: 4, .. }
        ));
    }

    /// The cap is the behaviour most worth pinning: without it, a project that
    /// crashes on a loop keeps a core busy respawning it forever.
    ///
    /// Run at a 10ms backoff rather than the real 1s, so five attempts take a
    /// moment instead of thirty-one seconds.
    #[tokio::test]
    async fn a_project_that_keeps_crashing_is_given_up_on_after_the_cap() {
        let directory = tempfile::tempdir().expect("temp dir");

        let handle = start(SupervisorConfig {
            restart_on_crash: true,
            backoff: Duration::from_millis(10),
            ..SupervisorConfig::new(
                crashes_after_a_moment(directory.path()),
                directory.path().join("run.log"),
            )
        })
        .await
        .expect("start");

        let gave_up = wait_until(&handle, |observed| {
            observed.status == HostStatus::Failed && observed.restarts >= MAX_RESTARTS
        })
        .await;
        assert!(gave_up, "the supervisor never reached the cap");

        // Nothing further happens: the count stops moving.
        let at_cap = handle.observe().restarts;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let observed = handle.observe();
        assert_eq!(
            observed.restarts, at_cap,
            "it kept restarting after giving up"
        );
        assert_eq!(observed.status, HostStatus::Failed);
        assert!(
            observed
                .failure_reason
                .unwrap_or_default()
                .contains("gave up"),
            "giving up has to say so, not just stop"
        );
    }

    /// A crash loop is a rate, not a total.
    ///
    /// Every attempt here survives longer than `stable_run`, so however many
    /// times it crashes it is never a loop and is never given up on. Without
    /// the reset, a project that crashes once a week would be abandoned on its
    /// sixth week — with a message blaming a crash loop that never happened.
    ///
    /// `stable_run` is 50ms here against a command that lives for about a
    /// second, so the margin is twenty-fold rather than a race.
    #[tokio::test]
    async fn attempts_that_stay_up_are_not_a_crash_loop_however_many_there_are() {
        let directory = tempfile::tempdir().expect("temp dir");

        let handle = start(SupervisorConfig {
            restart_on_crash: true,
            backoff: Duration::from_millis(10),
            stable_run: Duration::from_millis(50),
            ..SupervisorConfig::new(
                crashes_after_a_moment(directory.path()),
                directory.path().join("run.log"),
            )
        })
        .await
        .expect("start");

        // More restarts than the cap allows, every one of them after a run that
        // counts as stable.
        let kept_going = wait_until(&handle, |observed| observed.restarts > MAX_RESTARTS).await;
        assert!(
            kept_going,
            "supervision stopped restarting a project that was staying up between crashes"
        );

        assert_ne!(
            handle.observe().status,
            HostStatus::Failed,
            "a project that keeps coming back up was declared a crash loop"
        );

        handle.kill().await.expect("kill");
    }

    /// A project that survives its start and then crashes is restarted, and the
    /// restart is counted.
    #[tokio::test]
    async fn a_crash_after_a_good_start_is_restarted_and_counted() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(SupervisorConfig {
            restart_on_crash: true,
            ..SupervisorConfig::new(
                crashes_after_a_moment(directory.path()),
                directory.path().join("run.log"),
            )
        })
        .await
        .expect("start");

        // First crash, then the 1s backoff, then a fresh attempt.
        wait_until(&handle, |observed| observed.restarts >= 1).await;

        assert!(
            handle.observe().restarts >= 1,
            "the crash should have been followed by another attempt"
        );

        handle.kill().await.expect("kill");
    }

    /// A stop during the backoff must not be followed by another attempt: the
    /// user asked for it to be off.
    #[tokio::test]
    async fn stopping_during_the_backoff_ends_the_restart_loop() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(SupervisorConfig {
            restart_on_crash: true,
            ..SupervisorConfig::new(
                crashes_after_a_moment(directory.path()),
                directory.path().join("run.log"),
            )
        })
        .await
        .expect("start");

        // Wait for the crash, which puts the supervisor into its backoff.
        wait_until(&handle, |observed| observed.status == HostStatus::Crashed).await;

        handle.kill().await.expect("kill");
        tokio::time::sleep(Duration::from_secs(3)).await;

        let observed = handle.observe();
        assert_eq!(
            observed.restarts, 0,
            "a stop during the backoff must not be followed by a restart"
        );
        assert_eq!(observed.status, HostStatus::Stopped);
    }

    /// Health is polled while the project runs, and reaches `observe`.
    #[tokio::test]
    async fn a_running_project_has_its_health_polled() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(SupervisorConfig {
            health: Some(HealthPolicy {
                check: Check::resolved("TCP", Some(&port.to_string()), 2, None),
                interval: Duration::from_millis(200),
                start_period: Duration::from_millis(100),
            }),
            ..SupervisorConfig::new(
                long_running(directory.path()),
                directory.path().join("run.log"),
            )
        })
        .await
        .expect("start");

        wait_until(&handle, |observed| observed.health == Health::Passing).await;
        assert_eq!(handle.observe().health, Health::Passing);

        // Take the listener away and the next poll should say so.
        drop(listener);
        wait_until(&handle, |observed| {
            matches!(observed.health, Health::Failing(_))
        })
        .await;
        assert!(matches!(handle.observe().health, Health::Failing(_)));

        handle.kill().await.expect("kill");
    }

    /// A crash loop only explains itself if the output shown is the output of
    /// the attempt that just failed. A tail belonging to the first child stops
    /// filling the moment that child's pipes close, so every later failure would
    /// be reported with words from a run the user has already seen.
    #[tokio::test]
    async fn each_restarted_attempt_adds_its_own_output_to_the_tail() {
        let directory = tempfile::tempdir().expect("temp dir");

        #[cfg(windows)]
        let talks_then_crashes = command(
            "cmd",
            &["/C", "echo tick&& ping -n 2 127.0.0.1 >NUL && exit 9"],
            directory.path(),
        );
        #[cfg(unix)]
        let talks_then_crashes = command(
            "sh",
            &["-c", "echo tick; sleep 1; exit 9"],
            directory.path(),
        );

        let handle = start(SupervisorConfig {
            restart_on_crash: true,
            backoff: Duration::from_millis(10),
            ..SupervisorConfig::new(talks_then_crashes, directory.path().join("run.log"))
        })
        .await
        .expect("start");

        assert!(
            wait_until(&handle, |observed| observed.restarts >= 2).await,
            "the supervisor never reached a second restart"
        );

        // Two restarts means three attempts have run, and each said `tick`.
        let ticks = handle
            .tail()
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("tick"))
            .count();
        assert!(
            ticks >= 3,
            "the tail held only {ticks} attempts' output, so a later failure \
             would be reported with an earlier run's words"
        );

        handle.kill().await.expect("kill");
    }

    /// A failed start hands the caller an error and no handle. If the
    /// supervision task outlived that, it would go on respawning a project
    /// nothing holds a reference to — unstoppable, and invisible to the
    /// registry that is supposed to know what is running.
    #[tokio::test]
    async fn a_failed_start_does_not_leave_a_restart_loop_running() {
        let directory = tempfile::tempdir().expect("temp dir");
        let marker = directory.path().join("attempts.txt");

        #[cfg(windows)]
        let always_fails = command(
            "cmd",
            &["/C", "echo attempt>>attempts.txt&& exit 4"],
            directory.path(),
        );
        #[cfg(unix)]
        let always_fails = command(
            "sh",
            &["-c", "echo attempt >> attempts.txt; exit 4"],
            directory.path(),
        );

        start(SupervisorConfig {
            restart_on_crash: true,
            backoff: Duration::from_millis(10),
            ..SupervisorConfig::new(always_fails, directory.path().join("run.log"))
        })
        .await
        .expect_err("a program that exits at once has not started");

        // Long enough for every one of the five restarts to have happened at a
        // 10ms backoff, had anything still been driving them.
        tokio::time::sleep(Duration::from_secs(2)).await;

        let attempts = std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("attempt"))
            .count();
        assert_eq!(
            attempts, 1,
            "the process ran {attempts} times, so a start that failed left a \
             restart loop nobody can stop"
        );
    }

    /// Poll until the predicate holds, or give up after a bounded wait.
    async fn wait_until(
        handle: &SupervisorHandle,
        predicate: impl Fn(&HostObserved) -> bool,
    ) -> bool {
        for _ in 0..300 {
            if predicate(&handle.observe()) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// Stopping something that has already gone is the state the caller asked
    /// for, not an error to report.
    #[tokio::test]
    async fn stopping_a_project_twice_is_success() {
        let directory = tempfile::tempdir().expect("temp dir");
        let handle = start(SupervisorConfig::new(
            long_running(directory.path()),
            directory.path().join("run.log"),
        ))
        .await
        .expect("start");

        handle.kill().await.expect("first");
        handle.kill().await.expect("second");
    }
}
