//! State shared across every command handler.

use std::sync::Arc;

use project_host_compatibility::{Assessment, ResourceDefaults};
use project_host_database::Database;
use project_host_docker_manager::{DockerProbe, DockerStatus};
use tokio::sync::RwLock;

use crate::config::AppConfig;

/// Everything a command handler needs. Cheap to clone: one `Arc`.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

/// `Debug` is written by hand rather than derived, because this struct ends up
/// inside error contexts and a derived one would print the whole configuration.
pub struct Inner {
    pub config: AppConfig,
    pub database: Database,
    pub docker: Arc<dyn DockerProbe>,
    /// Last observed daemon status, refreshed on a timer rather than probed per
    /// call — a status bar that pinged Docker on every render would turn a slow
    /// daemon into a slow interface.
    pub docker_status: RwLock<DockerStatus>,
    /// What this machine is, and the resource defaults that follow from it.
    ///
    /// Decided once at startup: the hardware does not change while the process
    /// runs, and re-scanning per project creation would spawn subprocesses on a
    /// path the user is waiting on. Existing projects are never touched — a
    /// user who set a limit deliberately does not have it overwritten.
    pub assessment: Assessment,
    pub instance_id: String,
    pub app_version: String,
    pub schema_version: u32,
    pub started_at: std::time::Instant,
    pub started_at_wall: String,
}

/// Who this process is: the facts fixed at startup that never change.
///
/// Grouped rather than passed as four loose strings, because three of them are
/// `String` and a caller that swapped two would compile.
#[derive(Debug, Clone)]
pub struct Identity {
    /// Fresh on every start, so log lines from two runs can be told apart.
    pub instance_id: String,
    pub app_version: String,
    pub schema_version: u32,
    /// Wall-clock start time, for display. Uptime uses the monotonic clock.
    pub started_at_wall: String,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        database: Database,
        docker: Arc<dyn DockerProbe>,
        docker_status: DockerStatus,
        assessment: Assessment,
        identity: Identity,
    ) -> Self {
        Self(Arc::new(Inner {
            config,
            database,
            docker,
            docker_status: RwLock::new(docker_status),
            assessment,
            instance_id: identity.instance_id,
            app_version: identity.app_version,
            schema_version: identity.schema_version,
            started_at: std::time::Instant::now(),
            started_at_wall: identity.started_at_wall,
        }))
    }

    pub fn inner(&self) -> &Inner {
        &self.0
    }

    pub fn config(&self) -> &AppConfig {
        &self.0.config
    }

    pub fn database(&self) -> &Database {
        &self.0.database
    }

    /// The resource limits a newly created project should start with.
    pub fn resource_defaults(&self) -> ResourceDefaults {
        self.0.assessment.defaults
    }

    pub fn assessment(&self) -> Assessment {
        self.0.assessment
    }

    /// Monotonic, so a system clock change cannot produce a negative uptime.
    pub fn uptime_seconds(&self) -> u64 {
        self.0.started_at.elapsed().as_secs()
    }

    pub async fn docker_status(&self) -> DockerStatus {
        self.0.docker_status.read().await.clone()
    }

    /// Re-probe and store. Called by the background refresher.
    pub async fn refresh_docker_status(&self) -> DockerStatus {
        let status = self.0.docker.probe().await;
        *self.0.docker_status.write().await = status.clone();
        status
    }
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("instance_id", &self.instance_id)
            .field("app_version", &self.app_version)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("instance_id", &self.0.instance_id)
            .field("app_version", &self.0.app_version)
            .finish_non_exhaustive()
    }
}
