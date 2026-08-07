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
    pub recovery: RecoveryReport,
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
        );

        Ok(Self {
            state,
            paths,
            refresher: None,
            recovery,
        })
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
