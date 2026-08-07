//! Starting and stopping a project.
//!
//! The sequence, and why it is this order:
//!
//! 1. **Scaffold** — write the project's `Dockerfile` and starter files if they
//!    are missing, so there is something to build.
//! 2. **Network and volume** — created before the container that references
//!    them, and safe to repeat after a partial failure.
//! 3. **Build the image** — skipped when one already exists, because rebuilding
//!    on every start would make starting a bot take minutes.
//! 4. **Create the container** — from a spec that `docker-manager` hardens and
//!    then re-audits before it reaches Docker.
//! 5. **Start it**, and record what Docker then says about it.
//!
//! Status is written from what Docker reports, never from what was intended.
//! That separation is the reason the database has both `status` and
//! `desired_state`.
//!
//! **None of this has been run against a Docker daemon.** It compiles, and its
//! translation into Docker's API is unit tested, but the machine it was written
//! on has no Docker. Treat every claim about runtime behaviour here as
//! unverified.

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use crate::images::{dockerfile_for, starter_files, ImageSpec};
use crate::runner::docker::DockerRunner;
use crate::runner::host::HostRunner;
use crate::runner::{Observed, ProjectRunner, StartContext};
use crate::state::AppState;
use project_host_api_types::{DesiredState, HealthState, ProjectStatus};
use project_host_database::{projects, Database};
use project_host_docker_manager::container_spec::{
    ContainerSpec, NetworkMode, PortBinding, ResourceLimits, RestartPolicy, SpecInputs,
};
use project_host_docker_manager::lifecycle::ContainerState;
use project_host_docker_manager::DockerError;

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("database error: {0}")]
    Database(#[from] project_host_database::DatabaseError),
    #[error("Docker is not available: {0}")]
    Docker(#[from] DockerError),
    #[error("no project with id {0}")]
    NoSuchProject(String),
    #[error("could not prepare the project directory: {0}")]
    Scaffold(String),
    #[error("the image build failed: {0}")]
    Build(String),
    /// Docker reported a container state this build has no status for. Only
    /// reachable if `docker-manager` gains a word `ProjectStatus` does not have.
    #[error("`{0}` is not a project status this build knows")]
    UnknownStatus(String),

    /// Host mode only. The failure is reported before anything is spawned, so
    /// the message can name the runtime and the executables tried rather than
    /// being an operating-system error about a file that does not exist.
    #[error("{runtime} is not installed on this machine; looked for {}", looked_for.join(", "))]
    ToolchainMissing {
        runtime: String,
        looked_for: Vec<String>,
    },

    /// A runtime that host mode cannot serve. `STATIC` needs an HTTP server
    /// this application does not have, and `POLYGLOT` needs several toolchains
    /// at once — neither is a missing executable, and reporting one would send
    /// the user to install something that would not help.
    #[error("host mode cannot run {0} projects yet; run this one in Docker")]
    UnsupportedInHostMode(String),

    #[error("{0}")]
    Host(#[from] project_host_host_runner::HostError),

    #[error("{0}")]
    Command(#[from] project_host_host_runner::CommandError),
}

/// The runner a project's `run_mode` column asks for.
///
/// An unrecognised value is `DOCKER`, not an error. The schema's `CHECK` already
/// refuses anything but the two words, so the fallback is unreachable through
/// the application; if a hand-edited database ever reached it, defaulting to the
/// substrate that isolates is the safer of the two wrong answers.
pub fn runner_for(app: &AppState, project: &projects::ProjectRecord) -> Arc<dyn ProjectRunner> {
    match project.run_mode.as_str() {
        "HOST" => Arc::new(HostRunner::new(
            app.host_projects().clone(),
            app.logs_root(),
        )),
        _ => Arc::new(DockerRunner::new()),
    }
}

/// Write a project's `Dockerfile` and starter files if they are absent.
///
/// Never overwrites: once a project exists, its files belong to the user. A
/// scaffold that clobbered an edited `Dockerfile` on every start would be a
/// data-loss bug wearing a convenience hat. That is also what makes this safe to
/// call for a fetched repository — its own files are already there, so only a
/// missing `Dockerfile` gets written.
///
/// The image is generated from the project's *planned* commands rather than from
/// a fixed template, so a repository whose start command is `npm run serve` gets
/// an image that runs `npm run serve`.
pub fn scaffold(directory: &Path, spec: &ImageSpec<'_>) -> Result<(), LifecycleError> {
    std::fs::create_dir_all(directory)
        .map_err(|error| LifecycleError::Scaffold(error.to_string()))?;

    write_if_absent(&directory.join("Dockerfile"), &dockerfile_for(spec))?;

    for (relative, contents) in starter_files(spec.runtime) {
        let path = directory.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| LifecycleError::Scaffold(error.to_string()))?;
        }
        write_if_absent(&path, contents)?;
    }

    Ok(())
}

fn write_if_absent(path: &Path, contents: &str) -> Result<(), LifecycleError> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents).map_err(|error| LifecycleError::Scaffold(error.to_string()))
}

/// Build the container specification for a stored project.
pub(crate) async fn spec_for(
    db: &Database,
    project: &projects::ProjectRecord,
    app_version: &str,
) -> Result<ContainerSpec, LifecycleError> {
    let runtime = projects::find_runtime(db, &project.id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project.id.clone()))?;
    let ports = projects::list_ports(db, &project.id).await?;

    let bindings: Vec<PortBinding> = ports
        .iter()
        .filter_map(|port| {
            let host_port = u16::try_from(port.host_port?).ok()?;
            Some(PortBinding {
                container_port: u16::try_from(port.container_port).ok()?,
                host_port,
                protocol: port.protocol.clone(),
                bind_address: port.bind_address.clone(),
            })
        })
        .collect();

    // The start command is stored as text but must reach Docker as a list.
    // Splitting on whitespace is adequate because the value is ours, not the
    // user's — it comes from the template, and nothing in it is quoted.
    let command: Vec<String> = runtime
        .start_command
        .split_whitespace()
        .map(str::to_string)
        .collect();

    Ok(ContainerSpec::build(SpecInputs {
        slug: project.slug.clone(),
        project_id: project.id.clone(),
        template_id: runtime.template_id.clone(),
        agent_version: app_version.to_string(),
        image_tag: image_tag(&project.slug),
        command,
        working_dir: runtime.working_dir.clone(),
        environment: Vec::new(),
        project_dir: std::path::PathBuf::from(&project.directory),
        data_volume: ContainerSpec::volume_name(&project.slug),
        network_mode: match project.network_mode.as_str() {
            "NONE" => NetworkMode::None,
            "INTERNAL" => NetworkMode::Internal,
            _ => NetworkMode::Internet,
        },
        ports: bindings,
        limits: ResourceLimits::from_user_values(
            u32::try_from(project.memory_limit_mb).unwrap_or(512),
            project.cpu_limit_cores as f32,
            u32::try_from(project.process_limit).unwrap_or(128),
        ),
        restart_policy: match project.restart_policy.as_str() {
            "NO" => RestartPolicy::No,
            "ON_FAILURE" => RestartPolicy::OnFailure,
            "ALWAYS" => RestartPolicy::Always,
            _ => RestartPolicy::UnlessStopped,
        },
        health_check: None,
    }))
}

pub fn image_tag(slug: &str) -> String {
    format!("projecthost/{slug}:latest")
}

/// Docker's health word, in this application's vocabulary.
///
/// `docker inspect` answers `healthy`, `unhealthy` or `starting`; the column
/// stores `HEALTHY`, `UNHEALTHY`, `STARTING`, `NONE` or `UNKNOWN`. The two lists
/// were passed straight through, which meant a container that had a health check
/// and passed it ended its start with "value rejected by a database constraint" —
/// after the image was built and the container was already running.
///
/// A word Docker has and we do not becomes `UNKNOWN` rather than an error: the
/// container is up either way, and refusing to record that would be a worse
/// answer than recording that its health is not known.
pub(crate) fn health_state(reported: Option<&str>) -> Option<HealthState> {
    let word = reported?;
    Some(match word {
        "healthy" => HealthState::Healthy,
        "unhealthy" => HealthState::Unhealthy,
        "starting" => HealthState::Starting,
        "none" => HealthState::None,
        _ => HealthState::Unknown,
    })
}

/// The status word `docker-manager` derived, as the enum the column takes.
///
/// `ContainerState::project_status` already answers in this application's
/// vocabulary, so this parse succeeds for every value it can return. It exists
/// so that a word added there without a matching variant here is a reported
/// error rather than a write the database refuses.
pub(crate) fn project_status(state: &ContainerState) -> Result<ProjectStatus, LifecycleError> {
    let word = state.project_status();
    ProjectStatus::from_str(word).map_err(|_| LifecycleError::UnknownStatus(word.to_string()))
}

/// Start a project, building its image if there is not one already.
pub async fn start(app: &AppState, project_id: &str) -> Result<String, LifecycleError> {
    let db = app.database();
    let app_version = app.inner().app_version.clone();
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    projects::set_desired_state(db, project_id, DesiredState::Running).await?;
    projects::set_status(db, project_id, ProjectStatus::Starting, None).await?;

    let directory = std::path::PathBuf::from(&project.directory);
    let outcome = runner_for(app, &project)
        .start(StartContext {
            db,
            project: &project,
            directory: &directory,
            app_version: &app_version,
        })
        .await;

    record(db, project_id, outcome).await
}

/// Write what a runner observed, and answer with the status word.
///
/// The single place a lifecycle outcome becomes a row. Both substrates go
/// through it, which is what makes "status is what was observed, never what was
/// intended" a property of the module rather than a habit of each caller.
async fn record(
    db: &Database,
    project_id: &str,
    outcome: Result<Observed, LifecycleError>,
) -> Result<String, LifecycleError> {
    match outcome {
        Ok(observed) => {
            projects::set_status(db, project_id, observed.status, observed.health).await?;
            Ok(observed.status.as_str().to_string())
        }
        Err(error) => {
            // The observed status is FAILED whatever the intent was. Recording
            // the intent as the status is how a panel ends up claiming a
            // project runs when it does not.
            projects::set_status(db, project_id, ProjectStatus::Failed, None).await?;
            Err(error)
        }
    }
}

/// Stop a project. Its data volume and files are untouched.
pub async fn stop(app: &AppState, project_id: &str) -> Result<(), LifecycleError> {
    let db = app.database();
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    projects::set_desired_state(db, project_id, DesiredState::Stopped).await?;
    projects::set_status(db, project_id, ProjectStatus::Stopping, None).await?;

    runner_for(app, &project).stop(&project).await?;

    // A user-requested stop is a clean one: exit 0, no failure reason.
    projects::record_stopped(db, project_id, Some(0), None).await?;
    Ok(())
}

/// Kill a project immediately. Its data volume and files are untouched.
pub async fn kill(app: &AppState, project_id: &str) -> Result<(), LifecycleError> {
    let db = app.database();
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    projects::set_desired_state(db, project_id, DesiredState::Stopped).await?;
    projects::set_status(db, project_id, ProjectStatus::Stopping, None).await?;

    runner_for(app, &project).kill(&project).await?;

    projects::record_stopped(db, project_id, None, None).await?;
    Ok(())
}

/// Restart a project in place, without rebuilding its image.
pub async fn restart(app: &AppState, project_id: &str) -> Result<String, LifecycleError> {
    let db = app.database();
    let app_version = app.inner().app_version.clone();
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    // `RESTARTING` is now written before the runner is asked, rather than only
    // once a container was known to exist. A restart of something not running is
    // still a start, and now passes through `RESTARTING` on its way to
    // `STARTING` instead of going straight there. Both end in the same place;
    // the intermediate word is the honest one, because a restart is what was
    // asked for.
    projects::set_desired_state(db, project_id, DesiredState::Running).await?;
    projects::set_status(db, project_id, ProjectStatus::Restarting, None).await?;

    let directory = std::path::PathBuf::from(&project.directory);
    let outcome = runner_for(app, &project)
        .restart(StartContext {
            db,
            project: &project,
            directory: &directory,
            app_version: &app_version,
        })
        .await;

    record(db, project_id, outcome).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec for a runtime, with the commands a plan would have supplied.
    fn spec(runtime: &str) -> ImageSpec<'_> {
        ImageSpec {
            runtime,
            install_command: None,
            build_command: None,
            start_command: "run-the-thing",
            publish_dir: None,
        }
    }

    #[test]
    fn the_scaffold_writes_something_buildable_for_every_runtime() {
        for runtime in project_host_project_manager::detection::Runtime::ALL {
            let directory = tempfile::tempdir().expect("temp dir");
            scaffold(directory.path(), &spec(runtime.as_str())).expect("scaffold");

            let dockerfile = directory.path().join("Dockerfile");
            assert!(
                dockerfile.exists(),
                "{} produced no Dockerfile",
                runtime.as_str()
            );

            let contents = std::fs::read_to_string(&dockerfile).expect("read");
            assert!(
                contents.contains("FROM "),
                "{}: no base image",
                runtime.as_str()
            );
        }
    }

    #[test]
    fn the_scaffold_never_overwrites_what_the_user_edited() {
        // The failure this prevents is silent data loss on every start.
        let directory = tempfile::tempdir().expect("temp dir");
        scaffold(directory.path(), &spec("NODEJS")).expect("first");

        let entry = directory.path().join("index.js");
        std::fs::write(&entry, "// my actual bot\n").expect("edit");
        std::fs::write(directory.path().join("Dockerfile"), "FROM scratch\n").expect("edit");

        scaffold(directory.path(), &spec("NODEJS")).expect("second");

        assert_eq!(
            std::fs::read_to_string(&entry).expect("read"),
            "// my actual bot\n"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("Dockerfile")).expect("read"),
            "FROM scratch\n"
        );
    }

    #[test]
    fn no_scaffolded_image_runs_as_root() {
        // The container is also started with an explicit non-root user, but an
        // image whose own USER is root would still be wrong.
        for runtime in ["NODEJS", "PYTHON", "GO", "RUST", "JAVA", "PHP", "RUBY"] {
            let dockerfile = dockerfile_for(&spec(runtime));
            assert!(
                dockerfile.contains("USER 10001:10001") || dockerfile.contains("USER nonroot"),
                "{runtime} is missing its unprivileged user"
            );
        }
    }

    /// Every word `docker inspect` can put in `State.Health.Status`, as Docker
    /// spells it. The database column spells them differently, and passing them
    /// through unchanged was a rejected write at the end of a successful start.
    #[test]
    fn dockers_health_words_become_values_the_column_allows() {
        for (reported, expected) in [
            ("healthy", HealthState::Healthy),
            ("unhealthy", HealthState::Unhealthy),
            ("starting", HealthState::Starting),
            ("none", HealthState::None),
        ] {
            assert_eq!(health_state(Some(reported)), Some(expected), "{reported}");
        }

        // A word from a future Docker is recorded as unknown, not refused: the
        // container is running either way.
        assert_eq!(health_state(Some("delirious")), Some(HealthState::Unknown));
        // No health check configured means nothing to write, so the column keeps
        // whatever it held.
        assert_eq!(health_state(None), None);
    }

    fn container_state(running: bool, exit_code: Option<i64>) -> ContainerState {
        ContainerState {
            id: "abc".to_string(),
            status: "exited".to_string(),
            running,
            exit_code,
            health: None,
            started_at: None,
            finished_at: None,
            out_of_memory: false,
        }
    }

    #[test]
    fn every_status_docker_manager_derives_parses_into_the_enum() {
        for (state, expected) in [
            (container_state(true, None), ProjectStatus::Running),
            (container_state(false, Some(0)), ProjectStatus::Stopped),
            (container_state(false, None), ProjectStatus::Stopped),
            (container_state(false, Some(137)), ProjectStatus::Failed),
        ] {
            assert_eq!(project_status(&state).expect("a known status"), expected);
        }
    }

    #[test]
    fn the_image_tag_is_derived_from_the_generated_slug() {
        // Never from the display name, which is user input.
        assert_eq!(
            image_tag("quiet-harbor-4f2a"),
            "projecthost/quiet-harbor-4f2a:latest"
        );
    }

    #[test]
    fn a_static_site_scaffold_puts_its_page_where_the_image_expects_it() {
        let directory = tempfile::tempdir().expect("temp dir");
        scaffold(directory.path(), &spec("STATIC")).expect("scaffold");
        assert!(directory.path().join("public/index.html").exists());
    }
}
