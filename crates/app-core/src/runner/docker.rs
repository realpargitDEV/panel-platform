//! Running a project as a container. The behaviour this application has always
//! had, moved behind [`ProjectRunner`] without changing it.
//!
//! **None of this has been run against a Docker daemon.** It compiles, and its
//! translation into Docker's API is unit tested, but the machine it was written
//! on has no Docker. Treat every claim about runtime behaviour here as
//! unverified — the same caveat `lifecycle` has always carried.

use project_host_database::projects::{self, ProjectRecord};
use project_host_docker_manager::container_spec::ContainerSpec;
use project_host_docker_manager::lifecycle::ContainerRunner;
use project_host_docker_manager::DockerError;

use crate::images::ImageSpec;
use crate::lifecycle::{health_state, project_status, scaffold, spec_for, LifecycleError};
use crate::runner::{Observed, ProjectRunner, StartContext};

/// Starts projects as containers.
///
/// Holds no connection. Each call connects, because a daemon that went away
/// between two calls must be reported as gone rather than answered from a stale
/// handle — and because the connection is cheap next to what these methods then
/// ask the daemon to do.
#[derive(Debug, Default)]
pub struct DockerRunner;

impl DockerRunner {
    pub fn new() -> Self {
        Self
    }

    /// Connect to Docker, or explain that it is not there.
    async fn connect(&self) -> Result<ContainerRunner, LifecycleError> {
        let probe = project_host_docker_manager::system_probe();
        let connection = probe.connect().await?;
        Ok(ContainerRunner::new(connection.client().clone()))
    }
}

#[async_trait::async_trait]
impl ProjectRunner for DockerRunner {
    async fn start(&self, ctx: StartContext<'_>) -> Result<Observed, LifecycleError> {
        let StartContext {
            db,
            project,
            directory,
            app_version,
        } = ctx;
        let runner = self.connect().await?;

        let runtime_record = projects::find_runtime(db, &project.id)
            .await?
            .ok_or_else(|| {
                LifecycleError::Scaffold("the project has no runtime row".to_string())
            })?;
        scaffold(
            directory,
            &ImageSpec {
                runtime: &runtime_record.runtime,
                install_command: runtime_record.install_command.as_deref(),
                build_command: runtime_record.build_command.as_deref(),
                start_command: &runtime_record.start_command,
                publish_dir: runtime_record.publish_dir.as_deref(),
            },
        )?;

        let spec = spec_for(db, project, app_version).await?;

        if let Some(network) = &spec.network_name {
            runner.ensure_network(network).await?;
        }
        runner
            .ensure_volume(&ContainerSpec::volume_name(&project.slug))
            .await?;

        if !runner.has_image(&spec.image).await {
            let mut log = String::new();
            runner
                .build_image(&spec.image, directory, |line| {
                    // Kept for the failure message; the logs view will stream
                    // this properly once Phase 6 exists.
                    log.push_str(line);
                    log.push('\n');
                })
                .await
                .map_err(|error| LifecycleError::Build(format!("{error}\n{log}")))?;
        }

        // A container from a previous run has the old configuration baked in, so
        // it is replaced rather than reused.
        if runner.inspect(&spec.name).await?.is_some() {
            runner.remove(&spec.name, true).await?;
        }

        let container_id = runner.create(&spec).await?;
        projects::record_container(
            db,
            &project.id,
            Some(container_id.as_str()),
            Some(spec.image.as_str()),
        )
        .await?;

        runner.start(&spec.name).await?;
        projects::record_started(db, &project.id).await?;

        let state = runner.inspect(&spec.name).await?.ok_or_else(|| {
            LifecycleError::Docker(DockerError::Daemon(
                "the container vanished immediately after starting".to_string(),
            ))
        })?;

        Ok(Observed {
            status: project_status(&state)?,
            health: health_state(state.health.as_deref()),
            exit_code: state.exit_code,
            failure_reason: None,
        })
    }

    async fn stop(&self, project: &ProjectRecord) -> Result<(), LifecycleError> {
        let runner = self.connect().await?;
        let name = ContainerSpec::container_name(&project.slug);

        // Already gone is success. Stopping something that is not there is the
        // state the caller asked for.
        if runner.inspect(&name).await?.is_some() {
            runner.stop(&name, None).await?;
        }
        Ok(())
    }

    async fn kill(&self, project: &ProjectRecord) -> Result<(), LifecycleError> {
        let runner = self.connect().await?;
        let name = ContainerSpec::container_name(&project.slug);

        if runner
            .inspect(&name)
            .await?
            .is_some_and(|state| state.running)
        {
            runner.kill(&name).await?;
        }
        Ok(())
    }

    async fn observe(&self, project: &ProjectRecord) -> Result<Option<Observed>, LifecycleError> {
        let runner = self.connect().await?;
        let name = ContainerSpec::container_name(&project.slug);

        let Some(state) = runner.inspect(&name).await? else {
            return Ok(None);
        };

        Ok(Some(Observed {
            status: project_status(&state)?,
            health: health_state(state.health.as_deref()),
            exit_code: state.exit_code,
            failure_reason: None,
        }))
    }

    /// Docker restarts a container in place, keeping it and its configuration.
    /// The default `stop` then `start` would instead remove and recreate it,
    /// which is a different operation wearing the same name.
    async fn restart(&self, ctx: StartContext<'_>) -> Result<Observed, LifecycleError> {
        let runner = self.connect().await?;
        let name = ContainerSpec::container_name(&ctx.project.slug);

        // Nothing to restart means start it, which is what the user meant.
        if runner.inspect(&name).await?.is_none() {
            return self.start(ctx).await;
        }

        runner.restart(&name, None).await?;
        projects::increment_restart_count(ctx.db, &ctx.project.id).await?;

        let state = runner.inspect(&name).await?.ok_or_else(|| {
            LifecycleError::Docker(DockerError::Daemon(
                "the container vanished during a restart".to_string(),
            ))
        })?;

        Ok(Observed {
            status: project_status(&state)?,
            health: health_state(state.health.as_deref()),
            exit_code: state.exit_code,
            failure_reason: None,
        })
    }
}
