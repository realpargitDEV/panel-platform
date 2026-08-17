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
use project_host_host_runner::supervisor::{HostStatus, DEFAULT_GRACE};
use project_host_resources::{admit, Admission, RunningProject, Shortfall, Usage};

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

    #[error("{0}")]
    Host(#[from] project_host_host_runner::HostError),

    #[error("{0}")]
    Command(#[from] project_host_host_runner::CommandError),

    /// The machine cannot take another project. Boxed because `Shortfall`
    /// carries the running set, and an enum whose largest variant is a vector
    /// of projects makes every `Result` in this module that size.
    #[error("{}", .0.message())]
    NotEnoughMemory(Box<Shortfall>),

    /// The port this project binds is already held. Reported before anything is
    /// spawned, so the message names the port and its holder rather than being
    /// an `EADDRINUSE` buried in the project's own output.
    ///
    /// Boxed for the same reason as above.
    #[error("{}", .0.message())]
    PortConflict(Box<crate::ports::Conflict>),
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
///
/// `force` skips the memory check. The estimate can be wrong and the user knows
/// things the governor does not, so overriding is offered on refusal — as a
/// button in a dialog that states the numbers, never as a setting that turns
/// the governor off.
pub async fn start_forcing(
    app: &AppState,
    project_id: &str,
    force: bool,
) -> Result<String, LifecycleError> {
    let db = app.database();
    let app_version = app.inner().app_version.clone();
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    // Before anything is written or spawned. A refusal must leave the project
    // exactly as it was: writing STARTING and then refusing would leave a row
    // claiming a start that never began.
    //
    // The *port* check is not here but inside the host runner, because it can
    // only be asked once the project's own previous process has been stopped —
    // on a restart the port it wants is the port it is still holding.
    if !force {
        admit_project(app, &project).await?;
    }

    projects::set_desired_state(db, project_id, DesiredState::Running).await?;
    projects::set_status(db, project_id, ProjectStatus::Starting, None).await?;

    let directory = std::path::PathBuf::from(&project.directory);
    let outcome = runner_for(app, &project)
        .start(StartContext {
            db,
            project: &project,
            directory: &directory,
            app_version: &app_version,
            master_key: app.master_key(),
        })
        .await;

    record(db, project_id, outcome).await
}

/// Start a project, refusing if this machine cannot take another.
pub async fn start(app: &AppState, project_id: &str) -> Result<String, LifecycleError> {
    start_forcing(app, project_id, false).await
}

/// Stop every host project this process is running, and record that it stopped.
///
/// Called on the way out. Docker projects are deliberately untouched: the daemon
/// outlives this application and keeps them running under their own restart
/// policy. A host project has no such daemon — it is a child of this process —
/// so quitting stops it.
///
/// **`desired_state` is left alone.** A project stopped because the application
/// is quitting still *wants* to be running, and the honest row for that is
/// `desired_state = RUNNING` beside `status = STOPPED`. That pair is also
/// exactly what a "start these again" prompt on the next launch needs to find,
/// which is why no separate list is written.
///
/// Answers with the ids that were running, for the quit dialog to report.
pub async fn stop_host_projects(app: &AppState) -> Vec<String> {
    let registry = app.host_projects();
    let mut stopped = Vec::new();

    for (project_id, handle) in registry.all().await {
        if handle.observe().status != HostStatus::Running {
            continue;
        }

        if let Err(error) = handle.stop(DEFAULT_GRACE).await {
            // Report and carry on. One project that will not die must not
            // prevent the other fourteen from being stopped, and must not
            // prevent the database from being closed cleanly.
            tracing::warn!(%project_id, %error, "could not stop a host project on shutdown");
        }

        // Written from what happened, like every other status write. The
        // process is gone whether or not the stop reported cleanly.
        if let Err(error) =
            projects::record_stopped(app.database(), &project_id, Some(0), None).await
        {
            tracing::warn!(%project_id, %error, "could not record a host project as stopped");
        }
        stopped.push(project_id);
    }

    stopped
}

/// What every project that is currently up is costing this machine.
///
/// Host projects are measured: their supervisor holds a pid, and the reading
/// covers the whole process tree because `npm start`'s memory lives in the
/// `node` it spawned.
///
/// Docker projects are **not** measured. The daemon's stats endpoint is not
/// wired up, and this machine has no daemon to wire it against. Their declared
/// `memory_limit_mb` is counted instead, which is an upper bound rather than a
/// reading: a container cannot exceed the limit the daemon enforces. Counting
/// the bound errs towards refusing a start, which is the safe direction — the
/// opposite error would let the machine overcommit on a guess.
pub async fn running_projects(app: &AppState) -> Vec<RunningProject> {
    let Ok(records) = projects::list_projects(app.database(), false, None, 500).await else {
        return Vec::new();
    };

    let handles: std::collections::BTreeMap<String, _> =
        app.host_projects().all().await.into_iter().collect();

    let mut running = Vec::new();
    for record in records {
        if !is_running(&record.status) {
            continue;
        }

        let usage = match handles.get(&record.id).and_then(|handle| handle.pid()) {
            Some(pid) => app.usage_source().process_tree(pid).unwrap_or_default(),
            None => Usage {
                memory_bytes: record.memory_limit_mb.max(0).unsigned_abs() * 1024 * 1024,
                cpu_percent: None,
            },
        };

        running.push(RunningProject {
            project_id: record.id,
            display_name: record.display_name,
            run_mode: record.run_mode,
            usage,
        });
    }

    running
}

/// What a project is expected to need, in bytes.
///
/// Its own `memory_limit_mb`. For a container that is exact, because the daemon
/// enforces it. For a host project it is an estimate and nothing enforces it —
/// which is precisely why the start is gated rather than the process capped.
fn wanted_bytes(project: &projects::ProjectRecord) -> u64 {
    project.memory_limit_mb.max(0).unsigned_abs() * 1024 * 1024
}

/// Decide whether this machine can take one more project.
///
/// Called before the runner is dispatched, so both substrates are gated without
/// either runner knowing the governor exists.
pub async fn admit_project(
    app: &AppState,
    project: &projects::ProjectRecord,
) -> Result<(), LifecycleError> {
    let running = running_projects(app).await;
    let decision = admit(
        wanted_bytes(project),
        app.machine_usage().await,
        app.reserve(),
        &running,
    );

    match decision {
        Admission::Allow => Ok(()),
        Admission::Refuse(shortfall) => Err(LifecycleError::NotEnoughMemory(Box::new(shortfall))),
    }
}

/// Whether a stored status word means the project is up or on its way there.
///
/// Used to refuse changes that only make sense on a stopped project. The
/// transitional words count as running: a project that is STARTING is not a
/// project it is safe to reconfigure.
pub fn is_running(status: &str) -> bool {
    matches!(
        status,
        "RUNNING" | "STARTING" | "RESTARTING" | "STOPPING" | "BUILDING" | "UNHEALTHY"
    )
}

/// How many host projects would stop if the application quit now.
///
/// What the quit dialog counts. Separate from [`stop_host_projects`] because it
/// is asked before the user has decided, and must change nothing.
pub async fn host_projects_running(app: &AppState) -> usize {
    app.host_projects().running().await
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
            master_key: app.master_key(),
        })
        .await;

    record(db, project_id, outcome).await
}

/// Tests that need a real `AppState`: the governor, and shutdown.
///
/// Separate from the module below because those are pure functions needing
/// nothing, and these need a database, a project row and a described machine.
#[cfg(test)]
mod tests_with_state {
    use super::*;
    use project_host_api_types::{ProjectType, RunMode};
    use project_host_database::projects::NewProject;

    /// An `AppState` backed by an in-memory database and a Docker daemon that
    /// is not there — which is also the state of the machine this is written on.
    async fn test_state() -> AppState {
        let database = project_host_database::Database::open_in_memory()
            .await
            .expect("in-memory database");

        AppState::new(
            crate::config::AppConfig::default(),
            database,
            std::sync::Arc::new(crate::runner::tests::AbsentDocker),
            project_host_docker_manager::DockerStatus::unavailable(
                project_host_platform::DockerInstallHint {
                    summary: "Docker is not installed.".to_string(),
                    detail: String::new(),
                    url: String::new(),
                },
            ),
            project_host_compatibility::Assessment {
                tier: project_host_compatibility::PerformanceTier::Standard,
                defaults: project_host_compatibility::ResourceDefaults {
                    memory_limit_mb: 512,
                    cpu_limit_cores: 1.0,
                    process_limit: 128,
                },
            },
            crate::state::Identity {
                instance_id: "test".to_string(),
                app_version: "0.0.0-test".to_string(),
                schema_version: 5,
                started_at_wall: project_host_database::time::now(),
            },
            // No key: these tests never store a secret, and a test that
            // silently acquired one from the real keychain would be reaching
            // outside its sandbox.
            None,
        )
    }

    async fn a_host_project(app: &AppState, slug: &str) -> String {
        let project = projects::create_project(
            app.database(),
            &NewProject {
                slug: slug.to_string(),
                display_name: slug.to_string(),
                description: String::new(),
                project_type: ProjectType::Service,
                icon: None,
                color: None,
                source_type: "EMPTY".to_string(),
                directory: format!("projects/{slug}"),
                source_url: None,
                source_ref: None,
                source_commit: None,
                container_name: format!("ph-{slug}"),
                network_name: format!("ph-{slug}-net"),
                volume_name: format!("ph-{slug}-data"),
                autostart: false,
                restart_policy: "NO".to_string(),
                network_mode: "INTERNET".to_string(),
                memory_limit_mb: 512,
                cpu_limit_cores: 1.0,
                storage_limit_mb: 1024,
                process_limit: 128,
                runtime: project_host_database::projects::RuntimeSpec {
                    runtime: "NODEJS".to_string(),
                    runtime_version: "latest".to_string(),
                    package_manager: "NPM".to_string(),
                    install_command: None,
                    build_command: None,
                    start_command: "node index.js".to_string(),
                    working_dir: "/app".to_string(),
                    entry_file: None,
                    publish_dir: None,
                    template_id: "node".to_string(),
                    health_check_type: "NONE".to_string(),
                    health_check_target: None,
                    health_interval_s: 30,
                    health_timeout_s: 5,
                    health_retries: 3,
                    health_start_period_s: 10,
                },
                ports: Vec::new(),
            },
        )
        .await
        .expect("create");

        projects::set_run_mode(app.database(), &project.id, RunMode::Host)
            .await
            .expect("host mode");
        project.id
    }

    const GB: u64 = 1024 * 1024 * 1024;

    /// A machine with plenty to spare.
    fn roomy() -> project_host_resources::MachineUsage {
        project_host_resources::MachineUsage {
            total_memory_bytes: 32 * GB,
            available_memory_bytes: 24 * GB,
            cpu_percent: Some(10.0),
            logical_cores: 8,
        }
    }

    /// A machine with nothing to spare. Constructed rather than arranged: the
    /// alternative is exhausting the memory of whatever runs the tests.
    fn full() -> project_host_resources::MachineUsage {
        project_host_resources::MachineUsage {
            total_memory_bytes: 32 * GB,
            available_memory_bytes: GB / 2,
            cpu_percent: Some(99.0),
            logical_cores: 8,
        }
    }

    /// The requirement, in one test: on a full machine, starting is refused —
    /// and the refusal says what the numbers are rather than just failing.
    #[tokio::test]
    async fn a_full_machine_refuses_another_project() {
        let app = test_state().await;
        let project_id = a_host_project(&app, "crowded-port-9a11").await;
        app.set_usage_for_test(full()).await;

        let project = projects::find_project(app.database(), &project_id)
            .await
            .expect("query")
            .expect("row");

        match admit_project(&app, &project).await {
            Err(LifecycleError::NotEnoughMemory(shortfall)) => {
                let message = shortfall.message();
                assert!(message.contains("GB"), "no numbers in: {message}");
                assert_eq!(shortfall.headroom_bytes, 0, "a full machine spares nothing");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_roomy_machine_admits() {
        let app = test_state().await;
        let project_id = a_host_project(&app, "quiet-port-9a12").await;
        app.set_usage_for_test(roomy()).await;

        let project = projects::find_project(app.database(), &project_id)
            .await
            .expect("query")
            .expect("row");
        admit_project(&app, &project).await.expect("512 MB fits");
    }

    /// Before the sampler has run there is no basis for a refusal. Refusing
    /// everything until it warms up would look exactly like a broken
    /// application.
    #[tokio::test]
    async fn an_unmeasured_machine_admits_rather_than_blocking_the_first_start() {
        let app = test_state().await;
        let project_id = a_host_project(&app, "unmeasured-9a13").await;

        let project = projects::find_project(app.database(), &project_id)
            .await
            .expect("query")
            .expect("row");
        admit_project(&app, &project)
            .await
            .expect("nothing measured yet means nothing to refuse on");
    }

    /// The override exists because the estimate can be wrong and the user knows
    /// things the governor does not. `start` refuses; `start_forcing(.., true)`
    /// gets past the gate — and then fails for its own reasons, which is a
    /// different failure and the point of the assertion.
    #[tokio::test]
    async fn forcing_gets_past_the_governor() {
        let app = test_state().await;
        let project_id = a_host_project(&app, "forced-port-9a14").await;
        app.set_usage_for_test(full()).await;

        let refused = start(&app, &project_id).await;
        assert!(
            matches!(refused, Err(LifecycleError::NotEnoughMemory(_))),
            "got {refused:?}"
        );

        // NODEJS on a machine without Node, or a start that fails for any other
        // reason — either way it is no longer the governor refusing.
        let forced = start_forcing(&app, &project_id, true).await;
        assert!(
            !matches!(forced, Err(LifecycleError::NotEnoughMemory(_))),
            "forcing must not still be refused by the governor: {forced:?}"
        );
    }

    /// Nothing running is not a failure, and must not stop the database from
    /// being closed cleanly.
    #[tokio::test]
    async fn shutting_down_with_nothing_running_does_nothing() {
        let app = test_state().await;
        assert!(stop_host_projects(&app).await.is_empty());
        assert_eq!(host_projects_running(&app).await, 0);
    }

    /// The property the whole path exists for: a host project that was running
    /// is recorded as stopped, so the next launch does not open onto an
    /// interface claiming it runs.
    #[tokio::test]
    async fn quitting_stops_host_projects_and_records_it() {
        let app = test_state().await;
        let project_id = a_host_project(&app, "quiet-harbor-4f2a").await;

        // Start something long-running directly through the supervisor, which
        // is what HostRunner would have registered.
        let directory = tempfile::tempdir().expect("temp dir");
        #[cfg(windows)]
        let command = project_host_host_runner::ProcessCommand {
            program: "cmd".to_string(),
            args: vec!["/C".to_string(), "ping -n 60 127.0.0.1 >NUL".to_string()],
            cwd: directory.path().to_path_buf(),
            env: Default::default(),
        };
        #[cfg(unix)]
        let command = project_host_host_runner::ProcessCommand {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "sleep 60".to_string()],
            cwd: directory.path().to_path_buf(),
            env: Default::default(),
        };

        let handle =
            project_host_host_runner::start(project_host_host_runner::SupervisorConfig::new(
                command,
                directory.path().join("run.log"),
            ))
            .await
            .expect("start");

        app.host_projects()
            .insert_for_test(&project_id, handle.clone())
            .await;

        assert_eq!(host_projects_running(&app).await, 1);

        // Mark it as running and wanted, the way a real start would have.
        projects::set_desired_state(app.database(), &project_id, DesiredState::Running)
            .await
            .expect("desired");
        projects::set_status(app.database(), &project_id, ProjectStatus::Running, None)
            .await
            .expect("status");

        let stopped = stop_host_projects(&app).await;
        assert_eq!(stopped, vec![project_id.clone()]);

        let project = projects::find_project(app.database(), &project_id)
            .await
            .expect("query")
            .expect("row");
        assert_eq!(project.status, ProjectStatus::Stopped.as_str());

        // Still *wanted* running. That pair — wanted running, observed stopped
        // — is what a "start these again" prompt on the next launch reads.
        assert_eq!(project.desired_state, "RUNNING");

        assert!(!project_host_platform::is_alive(
            handle.pid().unwrap_or(u32::MAX)
        ));
    }
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

    /// Every status the schema allows is classified deliberately. A word added
    /// to the column later falls through to "not running" by default, and this
    /// is where that decision is made visible rather than implied.
    #[test]
    fn every_stored_status_is_classified_as_running_or_not() {
        for status in [
            "RUNNING",
            "STARTING",
            "RESTARTING",
            "STOPPING",
            "BUILDING",
            "UNHEALTHY",
        ] {
            assert!(is_running(status), "{status} should count as running");
        }

        // A project in any of these can be reconfigured safely.
        for status in ["CREATING", "STOPPED", "FAILED", "ARCHIVED", "DELETING"] {
            assert!(!is_running(status), "{status} should not count as running");
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
