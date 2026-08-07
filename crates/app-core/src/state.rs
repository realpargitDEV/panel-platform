//! State shared across every command handler.

use std::sync::Arc;

use project_host_compatibility::{Assessment, ResourceDefaults};
use project_host_database::Database;
use project_host_docker_manager::{DockerProbe, DockerStatus};
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::runner::host::HostRegistry;
use project_host_resources::{MachineUsage, Reserve, SystemUsage, UsageSource};

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
    /// Every host project this process is running.
    ///
    /// Lives here because it is process-scoped by nature: a supervisor handle
    /// owns a child of *this* process, so nothing outside the process can hold
    /// a meaningful one.
    pub host_projects: HostRegistry,
    /// Where usage figures come from. Behind a trait so a test can describe a
    /// machine that is out of memory, which is the case that matters and the
    /// one that cannot be arranged for real.
    pub usage: Arc<dyn UsageSource>,
    /// The last machine sample, refreshed on a timer rather than read per call.
    ///
    /// The same reason `docker_status` is: walking the process table on every
    /// render would turn a busy machine into an unusable interface.
    pub machine_usage: RwLock<MachineUsage>,
    /// How much memory is held back from the budget.
    pub reserve: Reserve,
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
            host_projects: HostRegistry::new(),
            usage: Arc::new(SystemUsage::new()),
            // Unknown until the sampler has run once. Admission treats that as
            // "cannot say" and allows, which is the honest answer before
            // anything has been measured.
            machine_usage: RwLock::new(MachineUsage::unknown()),
            reserve: Reserve::default(),
        }))
    }

    /// The last machine sample.
    pub async fn machine_usage(&self) -> MachineUsage {
        *self.0.machine_usage.read().await
    }

    /// Re-sample and store. Called by the background sampler.
    pub async fn refresh_machine_usage(&self) -> MachineUsage {
        // Sampled outside the lock: reading the process table is the slow part,
        // and holding the write lock across it would block every render.
        let sample = self.0.usage.machine();
        *self.0.machine_usage.write().await = sample;
        sample
    }

    pub fn usage_source(&self) -> &Arc<dyn UsageSource> {
        &self.0.usage
    }

    pub fn reserve(&self) -> Reserve {
        self.0.reserve
    }

    /// Replace the usage source and the last sample.
    ///
    /// For tests that need a machine which is out of memory — the case the
    /// governor exists for, and the one that cannot be arranged for real
    /// without making the machine running the tests unusable.
    #[cfg(test)]
    pub async fn set_usage_for_test(&self, sample: MachineUsage) {
        *self.0.machine_usage.write().await = sample;
    }

    /// Every host project this process is running.
    pub fn host_projects(&self) -> &HostRegistry {
        &self.0.host_projects
    }

    /// Where a project's run log is written.
    ///
    /// Falls back to a relative `logs` directory for the same reason
    /// `projects_dir` does: a configuration that named neither should still
    /// produce a working application rather than a panic at the first start.
    pub fn logs_root(&self) -> std::path::PathBuf {
        self.0
            .config
            .log_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("logs"))
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
