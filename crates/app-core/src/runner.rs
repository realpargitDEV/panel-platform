//! The seam between deciding to run a project and actually running one.
//!
//! [`lifecycle`](crate::lifecycle) owns the half of starting a project that is
//! the same whatever runs it: set the desired state, write `STARTING`, do the
//! work, then write the status that was *observed*. That last rule is why the
//! database has both `status` and `desired_state`, and it must hold identically
//! for a container and for a process.
//!
//! A [`ProjectRunner`] owns the other half. There are two: `DockerRunner`, which
//! is the behaviour this application has always had, and `HostRunner`, which
//! spawns an ordinary process on the user's machine.
//!
//! The trait speaks in project terms and nothing else. It deliberately does not
//! borrow `ensure_network`, `ensure_volume`, `has_image` or `build_image` from
//! `ContainerRunner`: four of that type's methods are meaningless for a process,
//! and an implementation that stubbed them `Ok(())` would be evidence of a
//! boundary drawn in the wrong place. They stay inside the Docker
//! implementation.

pub mod docker;

use std::path::Path;

use project_host_api_types::{HealthState, ProjectStatus};
use project_host_database::{projects::ProjectRecord, Database};

use crate::lifecycle::LifecycleError;

/// What is true about a project right now, in this application's vocabulary.
///
/// Deliberately not `ContainerState`, which carries Docker's words and Docker's
/// assumptions. A process has no health string and no image; it has a status, an
/// exit code and, when it failed, a reason worth showing someone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub status: ProjectStatus,
    /// `None` means "nothing to say", which leaves the column as it was. It is
    /// not the same as `Some(HealthState::None)`, which means the project has no
    /// health check.
    pub health: Option<HealthState>,
    pub exit_code: Option<i64>,
    pub failure_reason: Option<String>,
}

impl Observed {
    /// The common case: up, healthy or not, nothing to explain.
    pub fn running(health: Option<HealthState>) -> Self {
        Self {
            status: ProjectStatus::Running,
            health,
            exit_code: None,
            failure_reason: None,
        }
    }
}

/// Everything a runner needs to start one project.
///
/// The database handle is here because both implementations read rows that
/// belong to the project — its runtime, its ports — and write back what they
/// created. Passing the handle is honest about that; passing the rows instead
/// would mean the lifecycle layer knowing which rows each substrate happens to
/// need, which is the coupling this trait exists to remove.
pub struct StartContext<'a> {
    pub db: &'a Database,
    pub project: &'a ProjectRecord,
    pub directory: &'a Path,
    pub app_version: &'a str,
}

impl std::fmt::Debug for StartContext<'_> {
    /// Written by hand: the derived version would print the whole project row,
    /// environment included, into any error that captured a context.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartContext")
            .field("project", &self.project.id)
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

/// One way of running a project.
#[async_trait::async_trait]
pub trait ProjectRunner: Send + Sync + std::fmt::Debug {
    /// Start the project, and answer with what is true once it has started.
    async fn start(&self, ctx: StartContext<'_>) -> Result<Observed, LifecycleError>;

    /// Stop it gracefully. Stopping something already stopped is success: it is
    /// the state the caller asked for.
    async fn stop(&self, project: &ProjectRecord) -> Result<(), LifecycleError>;

    /// Stop it immediately.
    async fn kill(&self, project: &ProjectRecord) -> Result<(), LifecycleError>;

    /// What is true right now, or `None` if this substrate has never heard of
    /// the project.
    async fn observe(&self, project: &ProjectRecord) -> Result<Option<Observed>, LifecycleError>;

    /// Restart in place.
    ///
    /// Defaulted rather than required, because for a process there is no such
    /// thing: stop then start *is* the restart. Docker has a native restart that
    /// keeps the container, so `DockerRunner` overrides this. Having the default
    /// here is what lets `HostRunner` say nothing about restarting at all.
    async fn restart(&self, ctx: StartContext<'_>) -> Result<Observed, LifecycleError> {
        self.stop(ctx.project).await?;
        self.start(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Fake;

    #[async_trait::async_trait]
    impl ProjectRunner for Fake {
        async fn start(&self, _: StartContext<'_>) -> Result<Observed, LifecycleError> {
            Ok(Observed::running(None))
        }
        async fn stop(&self, _: &ProjectRecord) -> Result<(), LifecycleError> {
            Ok(())
        }
        async fn kill(&self, _: &ProjectRecord) -> Result<(), LifecycleError> {
            Ok(())
        }
        async fn observe(&self, _: &ProjectRecord) -> Result<Option<Observed>, LifecycleError> {
            Ok(None)
        }
    }

    /// Dispatching on `run_mode` means holding a runner behind a trait object,
    /// so the trait has to stay object-safe. A method that took `self` by value
    /// or returned `impl Trait` would break this and nothing else.
    #[tokio::test]
    async fn a_runner_can_be_held_behind_a_trait_object() {
        let runner: std::sync::Arc<dyn ProjectRunner> = std::sync::Arc::new(Fake);
        let project = sample_project();
        assert!(runner.observe(&project).await.expect("observe").is_none());
        assert_eq!(
            runner
                .stop(&project)
                .await
                .map_err(|error| error.to_string()),
            Ok(())
        );
    }

    fn sample_project() -> ProjectRecord {
        ProjectRecord {
            id: "prj_test".to_string(),
            slug: "quiet-harbor-4f2a".to_string(),
            display_name: "Test".to_string(),
            description: String::new(),
            project_type: "GENERIC".to_string(),
            icon: None,
            color: None,
            status: ProjectStatus::Stopped.as_str().to_string(),
            desired_state: "STOPPED".to_string(),
            health: "UNKNOWN".to_string(),
            container_id: None,
            container_name: None,
            image_tag: None,
            network_name: None,
            volume_name: None,
            source_type: "LOCAL".to_string(),
            directory: "projects/quiet-harbor-4f2a".to_string(),
            source_url: None,
            source_ref: None,
            source_commit: None,
            autostart: false,
            restart_policy: "UNLESS_STOPPED".to_string(),
            network_mode: "INTERNET".to_string(),
            run_mode: "DOCKER".to_string(),
            memory_limit_mb: 512,
            cpu_limit_cores: 1.0,
            storage_limit_mb: 1024,
            process_limit: 128,
            started_at: None,
            stopped_at: None,
            last_exit_code: None,
            last_failure_at: None,
            last_failure_reason: None,
            restart_count: 0,
            archived_at: None,
            created_at: "2026-08-07T00:00:00Z".to_string(),
            updated_at: "2026-08-07T00:00:00Z".to_string(),
        }
    }
}
