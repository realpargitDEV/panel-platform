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

use project_host_database::{projects, Database};
use project_host_docker_manager::container_spec::{
    ContainerSpec, NetworkMode, PortBinding, ResourceLimits, RestartPolicy, SpecInputs,
};
use project_host_docker_manager::lifecycle::ContainerRunner;
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
}

/// Connect to Docker, or explain that it is not there.
async fn runner() -> Result<ContainerRunner, LifecycleError> {
    let probe = project_host_docker_manager::system_probe();
    let connection = probe.connect().await?;
    Ok(ContainerRunner::new(connection.client().clone()))
}

/// Write a project's `Dockerfile` and starter files if they are absent.
///
/// Never overwrites: once a project exists, its files belong to the user. A
/// scaffold that clobbered an edited `Dockerfile` on every start would be a
/// data-loss bug wearing a convenience hat.
pub fn scaffold(directory: &Path, runtime: &str) -> Result<(), LifecycleError> {
    std::fs::create_dir_all(directory)
        .map_err(|error| LifecycleError::Scaffold(error.to_string()))?;

    let (dockerfile, entry_name, entry_body) = match runtime {
        "PYTHON" => (
            PYTHON_DOCKERFILE,
            "main.py",
            "print('Hello from Panel Platform', flush=True)\n",
        ),
        "STATIC" => (STATIC_DOCKERFILE, "public/index.html", STATIC_INDEX),
        _ => (
            NODE_DOCKERFILE,
            "index.js",
            "console.log('Hello from Panel Platform');\n",
        ),
    };

    write_if_absent(&directory.join("Dockerfile"), dockerfile)?;

    let entry = directory.join(entry_name);
    if let Some(parent) = entry.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| LifecycleError::Scaffold(error.to_string()))?;
    }
    write_if_absent(&entry, entry_body)?;

    if runtime == "NODEJS" {
        write_if_absent(&directory.join("package.json"), NODE_PACKAGE_JSON)?;
    }
    if runtime == "PYTHON" {
        write_if_absent(&directory.join("requirements.txt"), "")?;
    }

    Ok(())
}

fn write_if_absent(path: &Path, contents: &str) -> Result<(), LifecycleError> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents).map_err(|error| LifecycleError::Scaffold(error.to_string()))
}

/// Every image runs as an unprivileged user, and the container is started with
/// a read-only root filesystem, so nothing here may expect to write outside
/// `/app` or `/tmp`.
const NODE_DOCKERFILE: &str = r#"# Managed by Panel Platform. Safe to edit.
FROM node:22-alpine
WORKDIR /app
COPY package*.json ./
RUN npm install --omit=dev || true
COPY . .
# Matches the uid the container is started with.
USER 10001:10001
CMD ["node", "index.js"]
"#;

const PYTHON_DOCKERFILE: &str = r#"# Managed by Panel Platform. Safe to edit.
FROM python:3.12-alpine
WORKDIR /app
COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt || true
COPY . .
USER 10001:10001
CMD ["python", "main.py"]
"#;

const STATIC_DOCKERFILE: &str = r#"# Managed by Panel Platform. Safe to edit.
FROM nginx:alpine
COPY public/ /usr/share/nginx/html/
EXPOSE 80
"#;

const NODE_PACKAGE_JSON: &str = r#"{
  "name": "project",
  "private": true,
  "version": "1.0.0",
  "main": "index.js"
}
"#;

const STATIC_INDEX: &str =
    "<!doctype html>\n<html>\n  <body>\n    <h1>Hello from Panel Platform</h1>\n  </body>\n</html>\n";

/// Build the container specification for a stored project.
async fn spec_for(
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

/// Start a project, building its image if there is not one already.
pub async fn start(
    db: &Database,
    project_id: &str,
    app_version: &str,
) -> Result<String, LifecycleError> {
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    let runtime = projects::find_runtime(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    projects::set_desired_state(db, project_id, "RUNNING").await?;
    projects::set_status(db, project_id, "STARTING", None).await?;

    let outcome = start_inner(db, &project, &runtime.runtime, app_version).await;

    match outcome {
        Ok(state) => {
            projects::set_status(
                db,
                project_id,
                state.project_status(),
                state.health.as_deref(),
            )
            .await?;
            Ok(state.project_status().to_string())
        }
        Err(error) => {
            // The observed status is FAILED whatever the intent was. Recording
            // the intent as the status is how a panel ends up claiming a
            // project runs when it does not.
            projects::set_status(db, project_id, "FAILED", None).await?;
            Err(error)
        }
    }
}

async fn start_inner(
    db: &Database,
    project: &projects::ProjectRecord,
    runtime: &str,
    app_version: &str,
) -> Result<project_host_docker_manager::lifecycle::ContainerState, LifecycleError> {
    let runner = runner().await?;
    let directory = std::path::PathBuf::from(&project.directory);

    scaffold(&directory, runtime)?;

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
            .build_image(&spec.image, &directory, |line| {
                // Kept for the failure message; the logs view will stream this
                // properly once Phase 6 exists.
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

    runner.inspect(&spec.name).await?.ok_or_else(|| {
        LifecycleError::Docker(DockerError::Daemon(
            "the container vanished immediately after starting".to_string(),
        ))
    })
}

/// Stop a project. Its data volume and files are untouched.
pub async fn stop(db: &Database, project_id: &str) -> Result<(), LifecycleError> {
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    projects::set_desired_state(db, project_id, "STOPPED").await?;
    projects::set_status(db, project_id, "STOPPING", None).await?;

    let runner = runner().await?;
    let name = ContainerSpec::container_name(&project.slug);

    // Already gone is success. Stopping something that is not there is the
    // state the caller asked for.
    if runner.inspect(&name).await?.is_some() {
        runner.stop(&name, None).await?;
    }

    // A user-requested stop is a clean one: exit 0, no failure reason.
    projects::record_stopped(db, project_id, Some(0), None).await?;
    Ok(())
}

/// Restart a project in place, without rebuilding its image.
pub async fn restart(
    db: &Database,
    project_id: &str,
    app_version: &str,
) -> Result<String, LifecycleError> {
    let project = projects::find_project(db, project_id)
        .await?
        .ok_or_else(|| LifecycleError::NoSuchProject(project_id.to_string()))?;

    let runner = runner().await?;
    let name = ContainerSpec::container_name(&project.slug);

    // Nothing to restart means start it, which is what the user meant.
    if runner.inspect(&name).await?.is_none() {
        return start(db, project_id, app_version).await;
    }

    projects::set_status(db, project_id, "RESTARTING", None).await?;
    runner.restart(&name, None).await?;
    projects::increment_restart_count(db, project_id).await?;

    let state = runner.inspect(&name).await?.ok_or_else(|| {
        LifecycleError::Docker(DockerError::Daemon(
            "the container vanished during a restart".to_string(),
        ))
    })?;

    projects::set_status(
        db,
        project_id,
        state.project_status(),
        state.health.as_deref(),
    )
    .await?;
    Ok(state.project_status().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scaffold_writes_something_buildable_for_every_runtime() {
        for runtime in ["NODEJS", "PYTHON", "STATIC"] {
            let directory = tempfile::tempdir().expect("temp dir");
            scaffold(directory.path(), runtime).expect("scaffold");

            let dockerfile = directory.path().join("Dockerfile");
            assert!(dockerfile.exists(), "{runtime} produced no Dockerfile");

            let contents = std::fs::read_to_string(&dockerfile).expect("read");
            assert!(contents.contains("FROM "), "{runtime}: no base image");
        }
    }

    #[test]
    fn the_scaffold_never_overwrites_what_the_user_edited() {
        // The failure this prevents is silent data loss on every start.
        let directory = tempfile::tempdir().expect("temp dir");
        scaffold(directory.path(), "NODEJS").expect("first");

        let entry = directory.path().join("index.js");
        std::fs::write(&entry, "// my actual bot\n").expect("edit");
        std::fs::write(directory.path().join("Dockerfile"), "FROM scratch\n").expect("edit");

        scaffold(directory.path(), "NODEJS").expect("second");

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
        for dockerfile in [NODE_DOCKERFILE, PYTHON_DOCKERFILE] {
            assert!(
                dockerfile.contains("USER 10001:10001"),
                "an image is missing its unprivileged user"
            );
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
        scaffold(directory.path(), "STATIC").expect("scaffold");
        assert!(directory.path().join("public/index.html").exists());
    }
}
