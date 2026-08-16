//! Application lifecycle: start, run, stop.
//!
//! The order in [`Runtime::start`] matters and is not arbitrary:
//!
//! 1. Directories, so everything below has somewhere to write.
//! 2. Database and migrations, before anything reads state.
//! 3. Recovery, before the interface opens — the user must not be shown a
//!    half-repaired world.
//! 4. Docker probe, which may fail without stopping anything.
//!
//! Docker deliberately comes last and cannot abort startup. An application that
//! refuses to start without Docker cannot tell the user why Docker is missing.

use std::path::PathBuf;
use std::sync::Arc;

use project_host_database::{queries, recover, time, Database, RecoveryReport};
use project_host_docker_manager::{system_probe, DockerProbe};
use project_host_platform::{PathProvider, StandardPaths};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::AppConfig;
use crate::state::{AppState, Identity};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often the Docker daemon is re-probed in the background.
const DOCKER_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// How often the machine's memory and CPU are sampled.
///
/// Two seconds is a compromise: often enough that the number on screen is not
/// visibly stale, and rare enough that walking the process table is not itself
/// a load worth measuring.
const USAGE_SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// How often stored project statuses are reconciled against what the
/// supervisors are actually observing.
///
/// Two seconds, matching the usage sampler. This is what makes a project that
/// dies on its own appear as crashed without anybody pressing a button, so it
/// is the interval a user experiences as "how long until the interface tells me
/// the truth". The sweep itself is a map lookup per project and a write only
/// when something changed, so the cost of doing it often is close to nothing.
const RECONCILE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// How often the machine's power behaviour is reconsidered.
///
/// Five seconds rather than two, and the difference is deliberate. The usage
/// sampler feeds a number on screen, so it has to keep up with what a person
/// can see changing; this drives a moving average measured in minutes, and
/// sampling it more often would only add readings to an average that is
/// already smooth. It also bounds how often the priority commands can run.
const POWER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("could not prepare directories: {0}")]
    Directories(#[from] project_host_platform::PlatformError),
    #[error("database error: {0}")]
    Database(#[from] project_host_database::DatabaseError),
}

/// A started application core, ready to serve commands.
///
/// Holding one of these means the database is open and migrated, recovery has
/// run, and the Docker status has been observed at least once.
pub struct Runtime {
    state: AppState,
    paths: StandardPaths,
    refresher: Option<JoinHandle<()>>,
    sampler: Option<JoinHandle<()>>,
    reconciler: Option<JoinHandle<()>>,
    power: Option<JoinHandle<()>>,
    pub recovery: RecoveryReport,
    /// What the startup reconciliation found: projects whose stored status
    /// outlived the process it described, and which of those the user still
    /// wants running.
    ///
    /// Held rather than logged and dropped, because the window asks for it —
    /// "three projects were running when the application last closed, start
    /// them again?" is a question only this answer can pose.
    pub startup: crate::reconcile::StartupReport,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// Prepare everything the user interface will need.
    pub async fn start(config: AppConfig, paths: StandardPaths) -> Result<Self, RuntimeError> {
        paths.ensure_all()?;

        let database = Database::open(&paths.database_path()).await?;
        database.assert_schema_supported().await?;
        let schema_version = database.schema_version().await?;

        let instance_id = uuid::Uuid::new_v4().simple().to_string();

        // Whether the previous stop was clean decides how aggressive recovery
        // is, so it must be read before anything else touches app state.
        //
        // The recorded "address" is now a description of where this instance
        // runs rather than a socket, because there is no socket. It stays in
        // the session row because a support log that says which machine and
        // which process wrote a row is still worth having.
        let was_clean = queries::begin_agent_session(
            &database,
            APP_VERSION,
            schema_version,
            &instance_id,
            "in-process",
        )
        .await?;

        let recovery = recover(&database, was_clean).await?;
        if !recovery.integrity_ok {
            tracing::error!("database integrity check failed after an unclean shutdown");
        }
        if !recovery.is_uneventful() {
            tracing::warn!(
                locks_cleared = recovery.locks_cleared,
                deployments_interrupted = recovery.deployments_interrupted,
                backups_interrupted = recovery.backup_operations_interrupted,
                projects_reset = recovery.projects_reset_from_transient,
                "recovered state from an unclean shutdown"
            );
        }

        // Probed once now so the first render has an answer; refreshed on a
        // timer thereafter. A failure here is a reported state, never fatal.
        let probe: Arc<dyn DockerProbe> = Arc::new(system_probe());
        let docker_status = probe.probe().await;
        tracing::info!(docker = %docker_status.summary(), "docker probe complete");
        queries::record_heartbeat(&database, docker_status.available).await?;

        // Scanned once, here: the hardware does not change while the process
        // runs. The scan has no failure case, so this cannot make startup fail.
        let assessment = {
            use project_host_platform::SystemProbe;
            project_host_compatibility::assess(&project_host_platform::SystemScanner.snapshot())
        };
        tracing::info!(
            tier = assessment.tier.as_str(),
            memory_limit_mb = assessment.defaults.memory_limit_mb,
            cpu_limit_cores = assessment.defaults.cpu_limit_cores,
            process_limit = assessment.defaults.process_limit,
            "assessed this machine"
        );

        // Opened before the state so a failure is one clear log line rather
        // than a surprise the first time a secret is needed. A machine whose
        // keychain cannot be reached still runs; it just cannot hold tokens.
        let master_key = match crate::keys::load_or_create_master_key(paths.config_dir()) {
            Ok(loaded) => {
                tracing::info!(
                    backend = loaded.backend.as_str(),
                    created = loaded.created,
                    "master encryption key ready"
                );
                Some(loaded)
            }
            Err(error) => {
                tracing::error!(%error, "no master key; features that store secrets are disabled");
                None
            }
        };

        let state = AppState::new(
            config,
            database,
            probe,
            docker_status,
            assessment,
            Identity {
                instance_id,
                app_version: APP_VERSION.to_string(),
                schema_version,
                started_at_wall: time::now(),
            },
            master_key,
        );

        // Before the window opens, and after the database's own recovery has
        // cleared the transient statuses. Anything still claiming to be up is
        // describing a process from a previous run: the supervisor registry is
        // empty at this point by construction, so there is nothing to check
        // against and nothing to adopt.
        let startup = crate::reconcile::at_startup(&state).await;

        Ok(Self {
            state,
            paths,
            refresher: None,
            sampler: None,
            reconciler: None,
            power: None,
            recovery,
            startup,
        })
    }

    /// Start everything that should come up on its own.
    ///
    /// Separate from [`Runtime::start`] and called after the window exists, for
    /// two reasons: a project's install step can take minutes, and a bot that
    /// cannot connect should be visible in an open window rather than delaying
    /// one. Nothing here can fail the launch — every failure is logged against
    /// the project or bot it belongs to and the rest carry on.
    pub async fn start_automatic_workloads(&self) {
        crate::reconcile::start_autostart_projects(&self.state).await;

        crate::bots::start_autostart_bots(
            self.state.database(),
            self.state.master_key(),
            self.state.discord(),
        )
        .await;
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn paths(&self) -> &StandardPaths {
        &self.paths
    }

    /// Start the background Docker refresher.
    ///
    /// Kept separate from [`Runtime::start`] so tests can construct a runtime
    /// without a timer running underneath them. Calling it twice replaces the
    /// previous task rather than leaking a second one.
    pub fn spawn_docker_refresher(&mut self, mut shutdown: watch::Receiver<bool>) {
        if let Some(previous) = self.refresher.take() {
            previous.abort();
        }

        let state = self.state.clone();
        self.refresher = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(DOCKER_REFRESH_INTERVAL);
            // The first tick completes immediately; the status was already
            // probed during startup, so skip it.
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let status = state.refresh_docker_status().await;
                        if let Err(error) =
                            queries::record_heartbeat(state.database(), status.available).await
                        {
                            tracing::warn!(%error, "could not record a heartbeat");
                        }
                    }
                    _ = shutdown.changed() => break,
                }
            }
        }));
    }

    /// Sample what the machine is using, on a timer.
    ///
    /// On a timer rather than per call for the same reason the Docker status is:
    /// walking the process table on every render would turn a busy machine —
    /// exactly the machine this exists to detect — into an unusable interface.
    ///
    /// The first tick is not skipped, unlike the Docker refresher's. Nothing has
    /// sampled yet at this point, and until something does, admission has no
    /// basis for a refusal and allows everything.
    pub fn spawn_usage_sampler(&mut self, mut shutdown: watch::Receiver<bool>) {
        if let Some(previous) = self.sampler.take() {
            previous.abort();
        }

        let state = self.state.clone();
        self.sampler = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(USAGE_SAMPLE_INTERVAL);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        state.refresh_machine_usage().await;
                    }
                    _ = shutdown.changed() => break,
                }
            }
        }));
    }

    /// Keep every project's stored status equal to what its supervisor sees.
    ///
    /// The task that makes "Running" mean a process is running. Without it the
    /// database is only written when a lifecycle call returns, so a project
    /// that dies an hour after it started reads as running until somebody
    /// presses a button and finds out.
    pub fn spawn_reconciler(&mut self, mut shutdown: watch::Receiver<bool>) {
        if let Some(previous) = self.reconciler.take() {
            previous.abort();
        }

        let state = self.state.clone();
        self.reconciler = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(RECONCILE_INTERVAL);
            // Skipped, like the Docker refresher's: `at_startup` has just run.
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        crate::reconcile::sweep(&state).await;
                    }
                    _ = shutdown.changed() => break,
                }
            }
        }));
    }

    /// Start the background power manager.
    ///
    /// The first tick is not skipped. A machine with a keep-awake project
    /// already running — which is the state after the startup restart prompt —
    /// should be holding sleep off from the first few seconds, not from the
    /// first interval.
    pub fn spawn_power_manager(&mut self, mut shutdown: watch::Receiver<bool>) {
        if let Some(previous) = self.power.take() {
            previous.abort();
        }

        let state = self.state.clone();
        self.power = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(POWER_INTERVAL);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        crate::power_flow::tick(&state).await;
                    }
                    _ = shutdown.changed() => break,
                }
            }
        }));
    }

    /// Stop cleanly: stop host projects, flag the shutdown, checkpoint the WAL,
    /// close the pool.
    ///
    /// This does **not** stop project *containers*. Docker keeps them running
    /// under its own restart policy, which is what lets a bot stay online after
    /// the window is closed.
    ///
    /// Host projects are the opposite case and are stopped here. They are
    /// children of this process, so they would die with it regardless; stopping
    /// them deliberately is the difference between a clean stop with `STOPPED`
    /// recorded and a process that vanishes leaving the database claiming it
    /// runs. It happens before the pool closes, because it writes.
    pub async fn shutdown(mut self) {
        if let Some(refresher) = self.refresher.take() {
            refresher.abort();
        }
        if let Some(sampler) = self.sampler.take() {
            sampler.abort();
        }
        // Before the projects are stopped, so it cannot overwrite the statuses
        // the stop is about to write with the ones it read a moment earlier.
        if let Some(reconciler) = self.reconciler.take() {
            reconciler.abort();
        }
        // Aborted before the hold is released, so a tick already in flight
        // cannot re-take the assertion a moment after it is given up.
        if let Some(power) = self.power.take() {
            power.abort();
        }
        // Released explicitly rather than left to the drop. The process is
        // about to exit and the operating system would clear the assertion
        // anyway, but "anyway" is doing real work in that sentence on a
        // platform that outlives the handle, and a machine that will not sleep
        // after the application has closed is the worst bug this crate could
        // have.
        self.state.power().write().await.shutdown();

        // Discord first, and politely. A close frame tells Discord the session
        // is over; an abandoned socket leaves it resumable, and the bot shows
        // as online to everyone in the server for a minute or so after the
        // application has gone. Dropping the process without this is exactly
        // the "bot showing online when disconnected" complaint.
        self.state.discord().stop_all().await;

        let stopped = crate::lifecycle::stop_host_projects(&self.state).await;
        if !stopped.is_empty() {
            tracing::info!(count = stopped.len(), "stopped host projects on shutdown");
        }

        if let Err(error) = queries::record_clean_shutdown(self.state.database()).await {
            tracing::warn!(%error, "could not record a clean shutdown");
        }
        if let Err(error) = self.state.database().checkpoint().await {
            tracing::warn!(%error, "WAL checkpoint failed");
        }
        self.state.database().close().await;

        tracing::info!("application stopped");
    }
}

/// Resolve the directory layout, honouring a `data_dir` override for
/// development.
pub fn resolve_paths(config: &AppConfig) -> Result<StandardPaths, RuntimeError> {
    match &config.data_dir {
        Some(root) => Ok(StandardPaths::rooted(&PathBuf::from(root))),
        None => Ok(project_host_platform::platform_paths()?),
    }
}
