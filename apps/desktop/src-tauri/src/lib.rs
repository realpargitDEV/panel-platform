//! The desktop shell.
//!
//! Deliberately thin. Every command here does three things and no more: take
//! validated arguments, call into a domain crate, and turn the result into
//! something the window can render. No business logic lives in this crate,
//! because anything that lives here cannot be tested without a window.
//!
//! The [`Runtime`] is started once, before the window opens, and handed to
//! Tauri as managed state. Commands borrow it; none of them own it.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

use project_host_core::provisioning::SourceSpec;
use project_host_core::{resolve_paths, AppConfig, AppState, Runtime};
use project_host_database::projects;
use project_host_project_manager::names::{sanitise_display_name, Slug};
use project_host_project_manager::ports::PortPool;
use project_host_security::Secret;
use project_host_updater::{evaluate, ReleaseManifest, UpdateCheck, UpdatePreferences};

/// How long the release check may take before giving up.
///
/// Short, because it runs at startup and a slow feed must never delay the
/// window appearing.
const UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a command hands back when it fails.
///
/// A string rather than a rich type: these are shown to a person, and a failed
/// command must never leak a path, a query or a token into the interface.
#[derive(Debug, Serialize)]
pub struct CommandError {
    message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(error: E) -> Self {
        // The full error goes to the log; the window gets the summary.
        tracing::error!(%error, "command failed");
        Self {
            message: error.to_string(),
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// The state the window shows in its header.
#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub app_version: String,
    pub schema_version: u32,
    pub uptime_seconds: u64,
    pub started_at: String,
    pub docker_available: bool,
    pub docker_summary: String,
    pub docker_version: Option<String>,
    /// Present only when Docker is missing, and phrased as something the user
    /// can act on.
    pub docker_hint: Option<String>,
}

/// One row in the project list.
#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub project_type: String,
    pub status: String,
    pub desired_state: String,
    pub color: Option<String>,
}

#[tauri::command]
async fn system_status(state: tauri::State<'_, AppState>) -> CommandResult<SystemStatus> {
    // Bound through the deref first: `tauri::State` has its own inherent
    // `inner()`, which returns the managed value and would otherwise shadow
    // `AppState::inner()` at every call site.
    let app: &AppState = &state;
    let facts = app.inner();

    let docker = app.docker_status().await;
    Ok(SystemStatus {
        app_version: facts.app_version.clone(),
        schema_version: facts.schema_version,
        uptime_seconds: app.uptime_seconds(),
        started_at: facts.started_at_wall.clone(),
        docker_available: docker.available,
        docker_summary: docker.summary(),
        docker_version: docker.version.clone(),
        docker_hint: if docker.available {
            None
        } else {
            docker.install_hint.clone().or_else(|| docker.error.clone())
        },
    })
}

#[tauri::command]
async fn list_projects(state: tauri::State<'_, AppState>) -> CommandResult<Vec<ProjectSummary>> {
    let records = projects::list_projects(state.database(), false, None, 200).await?;
    Ok(records
        .into_iter()
        .map(|record| ProjectSummary {
            id: record.id,
            slug: record.slug,
            display_name: record.display_name,
            description: record.description,
            project_type: record.project_type,
            status: record.status,
            desired_state: record.desired_state,
            color: record.color,
        })
        .collect())
}

/// What the creation form sends.
#[derive(Debug, serde::Deserialize)]
pub struct NewProjectRequest {
    pub display_name: String,
    pub description: String,
    /// One of `NODEJS`, `PYTHON`, `STATIC`.
    pub runtime: String,
    /// Where the files come from. Absent means an empty project, which keeps
    /// older callers working.
    #[serde(default)]
    pub source: Option<SourceRequest>,
}

/// The source half of the creation form.
///
/// `Deserialize` only — this type is never sent back, which is what keeps the
/// token out of every response. `Debug` is hand-written for the same reason it is
/// on `SourceCredential`: the realistic leak is a handler logging the request it
/// failed to process.
#[derive(serde::Deserialize)]
pub struct SourceRequest {
    /// `EMPTY`, `GIT_CLONE` or `REMOTE_ARCHIVE`.
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    /// Branch or tag for `GIT_CLONE`. A commit id is refused with an explanation:
    /// fetching an arbitrary object by id needs the server's permission, and most
    /// servers do not give it.
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub subdirectory: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

impl std::fmt::Debug for SourceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceRequest")
            .field("kind", &self.kind)
            .field("url", &self.url)
            .field("git_ref", &self.git_ref)
            .field("subdirectory", &self.subdirectory)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Turn the form's strings into a source specification.
///
/// The only validation here is "did the form fill in the fields this kind
/// needs". Whether the URL is one the application may fetch is
/// `file-manager`'s answer, not this function's — it is asked later, in the one
/// place that opens connections.
fn source_spec_from(request: Option<SourceRequest>) -> CommandResult<SourceSpec> {
    let Some(request) = request else {
        return Ok(SourceSpec::Empty);
    };

    let trimmed = |value: Option<String>| -> Option<String> {
        value
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    };

    let url = trimmed(request.url);
    let token = trimmed(request.token).map(Secret::new);

    match request.kind.as_str() {
        "EMPTY" => Ok(SourceSpec::Empty),
        "GIT_CLONE" => Ok(SourceSpec::Git {
            url: url.ok_or_else(|| CommandError {
                message: "A repository address is needed to clone from.".to_string(),
            })?,
            git_ref: trimmed(request.git_ref),
            subdirectory: trimmed(request.subdirectory),
            token,
        }),
        "REMOTE_ARCHIVE" => Ok(SourceSpec::Archive {
            url: url.ok_or_else(|| CommandError {
                message: "An archive address is needed to download from.".to_string(),
            })?,
            token,
        }),
        other => Err(CommandError {
            message: format!("`{other}` is not a source this build offers."),
        }),
    }
}

/// The runtime defaults for a template.
///
/// Inline for now. The `project-manager` template registry reads these from
/// `manifest.toml` files, but those are not deployed alongside the binary yet —
/// that is an installer question, and shipping a wrong path here would fail at
/// the moment a user pressed Create rather than at build time.
fn runtime_spec_for(runtime: &str) -> Option<projects::RuntimeSpec> {
    let (image_runtime, version, manager, install, start, entry, port) = match runtime {
        "NODEJS" => (
            "NODEJS",
            "22",
            "NPM",
            Some("npm ci --omit=dev"),
            "node index.js",
            Some("index.js"),
            3000,
        ),
        "PYTHON" => (
            "PYTHON",
            "3.12",
            "PIP",
            Some("pip install --no-cache-dir -r requirements.txt"),
            "python main.py",
            Some("main.py"),
            8000,
        ),
        "STATIC" => ("STATIC", "1", "NONE", None, "caddy", None, 80),
        _ => return None,
    };

    Some((
        projects::RuntimeSpec {
            runtime: image_runtime.to_string(),
            runtime_version: version.to_string(),
            package_manager: manager.to_string(),
            install_command: install.map(str::to_string),
            build_command: None,
            start_command: start.to_string(),
            working_dir: "/app".to_string(),
            entry_file: entry.map(str::to_string),
            publish_dir: if runtime == "STATIC" {
                Some("public".to_string())
            } else {
                None
            },
            template_id: runtime.to_ascii_lowercase(),
            health_check_type: "NONE".to_string(),
            health_check_target: None,
            health_interval_s: 30,
            health_timeout_s: 5,
            health_retries: 3,
            health_start_period_s: 20,
        },
        port,
    ))
    .map(|(spec, _)| spec)
}

fn container_port_for(runtime: &str) -> i64 {
    match runtime {
        "PYTHON" => 8000,
        "STATIC" => 80,
        _ => 3000,
    }
}

/// Create a project.
///
/// The user supplies a display name and nothing else that becomes an
/// identifier. The slug, directory, container, network and volume names are all
/// derived from a generated id — see `project-manager`'s `names` module for why
/// a user-provided name must never become a path or a container name.
#[tauri::command]
async fn create_project(
    state: tauri::State<'_, AppState>,
    request: NewProjectRequest,
) -> CommandResult<ProjectSummary> {
    let app: &AppState = &state;

    let display_name = sanitise_display_name(request.display_name.trim());
    if display_name.is_empty() {
        return Err(CommandError {
            message: "A project needs a name.".to_string(),
        });
    }
    if display_name.chars().count() > 60 {
        return Err(CommandError {
            message: "That name is too long — 60 characters at most.".to_string(),
        });
    }

    let runtime = runtime_spec_for(&request.runtime).ok_or_else(|| CommandError {
        message: format!("`{}` is not a runtime this build offers.", request.runtime),
    })?;

    let count = projects::list_projects(app.database(), true, None, 1000)
        .await?
        .len();
    let limit = app.config().max_projects as usize;
    if count >= limit {
        return Err(CommandError {
            message: format!("You have reached the limit of {limit} projects."),
        });
    }

    // The id comes first: everything that names anything is derived from it.
    let id = project_host_api_types::ids::ProjectId::generate();
    let slug = Slug::from_project_id(id.as_str());

    let taken: std::collections::BTreeSet<u16> = projects::allocated_host_ports(app.database())
        .await?
        .into_iter()
        .collect();
    let pool = PortPool::new(app.config().port_pool_start, app.config().port_pool_end);
    let host_port = pool.allocate(&taken).map_err(CommandError::from)?;

    let projects_root = app
        .config()
        .projects_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("projects"));
    let directory = projects_root.join(slug.as_str());

    // Staging lives *inside* the projects directory rather than in the
    // application's temp directory. Promotion is a rename, a rename is atomic
    // only within one filesystem, and a user who pointed `projects_dir` at
    // another volume would otherwise get a cross-device failure at the last step
    // of every fetch.
    let staging_root = projects_root.join(".staging");
    std::fs::create_dir_all(&staging_root).map_err(CommandError::from)?;

    let spec = source_spec_from(request.source)?;

    // The files come first. A fetch that fails leaves nothing on disk and
    // nothing in the database, because the row below has not been written yet;
    // the other order would leave a project the user can see and cannot use
    // every time a remote is unreachable.
    let outcome = project_host_core::provisioning::materialise_source(
        &spec,
        &directory,
        &staging_root,
        id.as_str(),
    )
    .await?;

    let record = match projects::create_project(
        app.database(),
        &projects::NewProject {
            slug: slug.to_string(),
            display_name: display_name.clone(),
            description: sanitise_display_name(request.description.trim()),
            project_type: "GENERIC".to_string(),
            icon: None,
            color: None,
            source_type: spec.source_type().to_string(),
            directory: directory.display().to_string(),
            source_url: outcome.source_url.clone(),
            source_ref: outcome.source_ref.clone(),
            source_commit: outcome.source_commit.clone(),
            container_name: format!("projecthost-{slug}"),
            network_name: format!("projecthost-net-{slug}"),
            volume_name: format!("projecthost-data-{slug}"),
            autostart: false,
            restart_policy: "UNLESS_STOPPED".to_string(),
            network_mode: "INTERNET".to_string(),
            memory_limit_mb: 512,
            cpu_limit_cores: 1.0,
            storage_limit_mb: 2048,
            process_limit: 128,
            runtime,
            ports: vec![projects::NewPort {
                container_port: container_port_for(&request.runtime),
                host_port: Some(i64::from(host_port)),
                protocol: "tcp".to_string(),
                bind_address: "127.0.0.1".to_string(),
                is_primary: true,
            }],
        },
    )
    .await
    {
        Ok(record) => record,
        Err(error) => {
            // The files are already there. Leaving them would make the slug's
            // directory occupied for a project that does not exist, and the next
            // attempt with the same id would refuse to write into it.
            project_host_core::provisioning::discard_directory(&directory);
            return Err(CommandError::from(error));
        }
    };

    // No key is held at runtime yet, so this stores nothing and says so. See
    // `provisioning`'s module documentation.
    let stored = project_host_core::provisioning::store_source_token(
        app.database(),
        None,
        &record.id,
        &spec,
    )
    .await?;

    if stored.token_used_but_not_stored {
        tracing::info!(
            project = %record.id,
            "the access token was used for the fetch and not stored: no key store exists yet"
        );
    }

    Ok(ProjectSummary {
        id: record.id,
        slug: record.slug,
        display_name: record.display_name,
        description: record.description,
        project_type: record.project_type,
        status: record.status,
        desired_state: record.desired_state,
        color: record.color,
    })
}

/// Start a project.
///
/// Long-running: building an image the first time takes minutes. The window
/// keeps the button in a pending state rather than this pretending to be fast.
#[tauri::command]
async fn start_project(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> CommandResult<String> {
    let app: &AppState = &state;
    let version = app.inner().app_version.clone();
    project_host_core::lifecycle::start(app.database(), &project_id, &version)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
async fn stop_project(state: tauri::State<'_, AppState>, project_id: String) -> CommandResult<()> {
    let app: &AppState = &state;
    project_host_core::lifecycle::stop(app.database(), &project_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
async fn restart_project(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> CommandResult<String> {
    let app: &AppState = &state;
    let version = app.inner().app_version.clone();
    project_host_core::lifecycle::restart(app.database(), &project_id, &version)
        .await
        .map_err(CommandError::from)
}

/// The configuration the window shows on the settings screen.
///
/// Read-only for now. Every value here is loaded at startup from `config.toml`
/// and the `PROJECT_HOST_*` environment; there is no write path yet, so the
/// screen shows what is in force rather than pretending to accept edits.
#[derive(Debug, Serialize)]
pub struct AppSettings {
    pub mode: String,
    pub log_level: String,
    pub log_json: bool,
    pub log_retention_days: u16,
    pub max_projects: u32,
    pub max_upload_bytes: u64,
    pub port_pool_start: u16,
    pub port_pool_end: u16,
    pub port_pool_size: u32,
    pub docker_enabled: bool,
    pub data_dir: String,
    pub projects_dir: String,
    pub logs_dir: String,
    pub backups_dir: String,
}

#[tauri::command]
async fn app_settings(state: tauri::State<'_, AppState>) -> CommandResult<AppSettings> {
    use project_host_platform::PathProvider;

    let app: &AppState = &state;
    let config = app.config();
    let paths = project_host_core::resolve_paths(config).map_err(CommandError::from)?;

    Ok(AppSettings {
        mode: format!("{:?}", config.mode).to_lowercase(),
        log_level: config.log_level.as_str().to_string(),
        log_json: config.log_json,
        log_retention_days: config.log_retention_days,
        max_projects: config.max_projects,
        max_upload_bytes: config.max_upload_bytes,
        port_pool_start: config.port_pool_start,
        port_pool_end: config.port_pool_end,
        port_pool_size: config.port_pool_size(),
        docker_enabled: config.docker_enabled,
        data_dir: paths.data_dir().display().to_string(),
        projects_dir: paths.projects_dir().display().to_string(),
        logs_dir: paths.log_dir().display().to_string(),
        backups_dir: paths.backups_dir().display().to_string(),
    })
}

/// Ask the release feed whether there is a newer version.
///
/// Every rule about *whether to offer* an update lives in the `updater` crate;
/// this only performs the fetch, which is the one part that needs a network and
/// therefore cannot be unit tested.
#[tauri::command]
async fn check_for_update() -> CommandResult<UpdateCheck> {
    let client = reqwest::Client::builder()
        .timeout(UPDATE_TIMEOUT)
        .user_agent(concat!("project-host/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(CommandError::from)?;

    let response = client
        .get(project_host_updater::RELEASE_FEED_URL)
        .send()
        .await
        .map_err(CommandError::from)?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // The honest answer while no release has ever been published, and a
        // clearer one than "the feed returned 404".
        return Err(CommandError {
            message: "No releases have been published yet.".to_string(),
        });
    }
    let response = response.error_for_status().map_err(CommandError::from)?;

    let manifest: ReleaseManifest = response.json().await.map_err(CommandError::from)?;

    evaluate(
        &manifest,
        project_host_updater::CURRENT_VERSION,
        &UpdatePreferences::default(),
    )
    .map_err(CommandError::from)
}

/// Build and run the application.
///
/// The runtime is started *before* the window so that a database that cannot be
/// opened is a clear failure at launch rather than an interface full of errors.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load(&std::path::PathBuf::from("config.toml"))?;
    let paths = resolve_paths(&config)?;

    let tokio_runtime = tokio::runtime::Runtime::new()?;
    let mut runtime = tokio_runtime.block_on(Runtime::start(config, paths))?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio_runtime.block_on(async {
        runtime.spawn_docker_refresher(shutdown_rx);
    });

    let state = runtime.state().clone();
    // Kept alive for the life of the process so the shutdown path can run it.
    let runtime = Arc::new(tokio::sync::Mutex::new(Some(runtime)));

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            system_status,
            list_projects,
            create_project,
            start_project,
            stop_project,
            restart_project,
            app_settings,
            check_for_update
        ])
        .build(tauri::generate_context!())?
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                // Close the database cleanly, so the next start does not have to
                // run recovery. Project containers are untouched: Docker keeps
                // them running under their own restart policy.
                let _ = shutdown_tx.send_replace(true);
                if let Some(runtime) = runtime.blocking_lock().take() {
                    app.state::<AppState>();
                    tokio_runtime.block_on(runtime.shutdown());
                }
            }
        });

    Ok(())
}
