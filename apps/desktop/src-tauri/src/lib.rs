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
    /// A runtime to use regardless of what the files say. Absent means "look at
    /// the files and decide", which is the normal case for anything fetched.
    ///
    /// An empty project has no files, so one is required there — nothing can be
    /// detected from nothing.
    #[serde(default)]
    pub runtime: Option<String>,
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
    /// `EMPTY`, `GIT_CLONE`, `REMOTE_ARCHIVE` or `GITHUB_CLI`.
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
        "GITHUB_CLI" => Ok(SourceSpec::GitHubCli {
            // The same field the other kinds use for a URL; here it holds
            // `owner/repo`, which `github_cli` parses and validates.
            repo: url.ok_or_else(|| CommandError {
                message: "A repository name like `owner/repo` is needed.".to_string(),
            })?,
            git_ref: trimmed(request.git_ref),
            subdirectory: trimmed(request.subdirectory),
        }),
        other => Err(CommandError {
            message: format!("`{other}` is not a source this build offers."),
        }),
    }
}

/// Whether the GitHub CLI can be used, and as whom.
///
/// Asked before the option is offered, so a user without `gh` is told to install
/// it or paste a token rather than meeting a failure at Create.
#[derive(Debug, Serialize)]
pub struct GitHubCliStatus {
    pub installed: bool,
    /// The logged-in account, when there is one. `None` with `installed: true`
    /// means `gh` is there but nobody is logged in.
    pub account: Option<String>,
    /// What to tell the user, when something needs doing.
    pub hint: Option<String>,
}

#[tauri::command]
async fn github_cli_status() -> CommandResult<GitHubCliStatus> {
    use project_host_file_manager::github_cli::{self, GhCommand};

    // Spawning a process is blocking work, and this is called while the dialog is
    // being drawn.
    let status = tokio::task::spawn_blocking(|| {
        if !github_cli::is_available(&GhCommand) {
            return GitHubCliStatus {
                installed: false,
                account: None,
                hint: Some(
                    "The GitHub CLI (`gh`) is not installed, or not on the PATH. Install it,                      or use `Git repository` with a token instead."
                        .to_string(),
                ),
            };
        }

        match github_cli::logged_in_user(&GhCommand) {
            Ok(account) => GitHubCliStatus {
                installed: true,
                account,
                hint: None,
            },
            Err(_) => GitHubCliStatus {
                installed: true,
                account: None,
                hint: Some("`gh` is installed but nobody is logged in. Run `gh auth login`.".to_string()),
            },
        }
    })
    .await
    .map_err(|_| CommandError {
        message: "Checking the GitHub CLI did not finish.".to_string(),
    })?;

    Ok(status)
}

/// The runtimes this build can plan for, for the override list.
///
/// Served from the same table the planner uses, so the interface cannot offer a
/// choice that fails on Create.
#[derive(Debug, Serialize)]
pub struct RuntimeOption {
    pub id: String,
    pub label: String,
}

#[tauri::command]
fn supported_runtimes() -> Vec<RuntimeOption> {
    project_host_core::runtime_plan::supported_runtimes()
        .into_iter()
        .map(|(id, label)| RuntimeOption {
            id: id.to_string(),
            label: label.to_string(),
        })
        .collect()
}

/// What a created project's runtime turned out to be.
///
/// Returned alongside the project because the interesting half of "create" is now
/// an answer rather than an echo: the user did not say `GO`, the files did.
#[derive(Debug, Serialize)]
pub struct CreatedProject {
    #[serde(flatten)]
    pub project: ProjectSummary,
    /// The runtime wire value that was stored.
    pub runtime: String,
    /// True when it came from the files rather than from the user.
    pub detected: bool,
    /// Every language found in the tree, for display.
    pub languages: Vec<String>,
    /// Detection warnings, in the words the user should read.
    pub notes: Vec<String>,
}

/// Choose the runtime for a project whose files are already in place.
///
/// An empty project cannot be detected — there is nothing to look at — so it
/// needs an explicit choice, and saying so beats reporting "no language found"
/// for a directory that was deliberately created empty.
fn plan_runtime(
    directory: &std::path::Path,
    source: &SourceSpec,
    named: Option<&str>,
) -> CommandResult<project_host_core::RuntimePlan> {
    let empty_source = matches!(source, SourceSpec::Empty);

    match (named, empty_source) {
        (Some(runtime), _) if empty_source => {
            project_host_core::plan_named(runtime).map_err(CommandError::from)
        }
        (None, true) => Err(CommandError {
            message: "An empty project has no files to inspect, so it needs a \
                      runtime. Choose one, or start from a repository instead."
                .to_string(),
        }),
        (named, false) => project_host_core::plan_detected(directory, named).map_err(|error| {
            // Detection's message names every marker file it looked for, which is
            // the most useful thing a user can be told here.
            CommandError {
                message: error.to_string(),
            }
        }),
        // Unreachable: covered by the first arm.
        (Some(runtime), _) => project_host_core::plan_named(runtime).map_err(CommandError::from),
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
) -> CommandResult<CreatedProject> {
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

    // The runtime is decided *after* the files are there, because deciding it
    // before means asking the user a question the files can answer. An empty
    // project has no files, so there a choice is required and detection would
    // only report that it found nothing.
    let plan = match plan_runtime(&directory, &spec, request.runtime.as_deref()) {
        Ok(plan) => plan,
        Err(error) => {
            // Nothing is in the database yet, but the files are on disk. They go,
            // so a retry with a corrected runtime starts from a clean directory.
            project_host_core::provisioning::discard_directory(&directory);
            return Err(error);
        }
    };

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
            runtime: plan.spec.clone(),
            ports: vec![projects::NewPort {
                container_port: plan.container_port,
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

    Ok(CreatedProject {
        project: ProjectSummary {
            id: record.id,
            slug: record.slug,
            display_name: record.display_name,
            description: record.description,
            project_type: record.project_type,
            status: record.status,
            desired_state: record.desired_state,
            color: record.color,
        },
        runtime: plan.spec.runtime,
        detected: plan.detected,
        languages: plan.languages,
        notes: plan.notes,
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

// -------------------------------------------------------------- project files

/// One row in a directory listing, as the window reads it.
#[derive(Debug, Serialize)]
pub struct FileEntryDto {
    pub name: String,
    pub path: String,
    /// `file`, `directory` or `other`.
    pub kind: String,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<i64>,
    pub is_symlink: bool,
}

impl From<project_host_file_manager::FileEntry> for FileEntryDto {
    fn from(entry: project_host_file_manager::FileEntry) -> Self {
        Self {
            name: entry.name,
            path: entry.path,
            kind: entry.kind.as_str().to_string(),
            size_bytes: entry.size_bytes,
            modified_unix_ms: entry.modified_unix_ms,
            is_symlink: entry.is_symlink,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListingDto {
    pub path: String,
    pub entries: Vec<FileEntryDto>,
    /// True when there were more entries than the limit. The window says so
    /// rather than showing a partial listing as if it were complete.
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct TextFileDto {
    pub path: String,
    pub text: String,
    pub size_bytes: u64,
    pub language: String,
    /// True when the project is mid-build or mid-deletion. The editor opens the
    /// file read-only rather than letting a save race the operation.
    pub read_only: bool,
}

/// Statuses during which a project's files must not be written.
///
/// `BUILDING` copies the tree into an image; `DELETING` is removing it. A save
/// landing in either window either vanishes or corrupts what is being read.
fn is_read_only_status(status: &str) -> bool {
    matches!(status, "BUILDING" | "DELETING")
}

/// A project's root directory, from its stored record.
///
/// Every file command starts here, and every one of them passes a *relative*
/// request string alongside it. The window cannot express an absolute path,
/// which is the invariant `file-manager` was built around: `SafePath` is
/// constructed on this side of the bridge, from a root the database supplied and
/// a string that is only ever a suffix.
async fn project_root(
    app: &AppState,
    project_id: &str,
) -> CommandResult<(std::path::PathBuf, bool)> {
    let record = projects::find_project(app.database(), project_id)
        .await?
        .ok_or_else(|| CommandError {
            message: "That project no longer exists.".to_string(),
        })?;
    let root = std::path::PathBuf::from(record.directory);

    // A project row written before creation materialised any files has a
    // directory that does not exist yet. The row owns that path — it is `UNIQUE`
    // in the schema and derived from the project's own id — so creating it is
    // what an empty project should have looked like all along, and is better
    // than greeting the user with a path error they can do nothing about.
    if !root.exists() {
        std::fs::create_dir_all(&root).map_err(CommandError::from)?;
    }

    Ok((root, is_read_only_status(&record.status)))
}

fn file_limits(app: &AppState) -> project_host_file_manager::FileLimits {
    project_host_file_manager::FileLimits {
        max_upload_bytes: app.config().max_upload_bytes,
        ..project_host_file_manager::FileLimits::default()
    }
}

#[tauri::command]
async fn list_project_files(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
) -> CommandResult<ListingDto> {
    let app: &AppState = &state;
    let (root, _) = project_root(app, &project_id).await?;
    let listing =
        project_host_file_manager::operations::list_directory(&root, &path, &file_limits(app))
            .map_err(CommandError::from)?;

    Ok(ListingDto {
        path: listing.path,
        entries: listing
            .entries
            .into_iter()
            .map(FileEntryDto::from)
            .collect(),
        truncated: listing.truncated,
    })
}

#[tauri::command]
async fn read_project_file(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
) -> CommandResult<TextFileDto> {
    let app: &AppState = &state;
    let (root, read_only) = project_root(app, &project_id).await?;
    let file =
        project_host_file_manager::operations::read_text_file(&root, &path, &file_limits(app))
            .map_err(CommandError::from)?;

    Ok(TextFileDto {
        path: file.path,
        text: file.text,
        size_bytes: file.size_bytes,
        language: file.language.to_string(),
        read_only,
    })
}

#[tauri::command]
async fn write_project_file(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
    text: String,
) -> CommandResult<FileEntryDto> {
    let app: &AppState = &state;
    let (root, read_only) = project_root(app, &project_id).await?;
    if read_only {
        return Err(CommandError {
            message: "This project is being built or removed; its files cannot be saved right now."
                .to_string(),
        });
    }

    project_host_file_manager::operations::write_text_file(&root, &path, &text, &file_limits(app))
        .map(FileEntryDto::from)
        .map_err(CommandError::from)
}

#[tauri::command]
async fn create_project_file(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
    directory: bool,
) -> CommandResult<FileEntryDto> {
    let app: &AppState = &state;
    let (root, read_only) = project_root(app, &project_id).await?;
    if read_only {
        return Err(CommandError {
            message:
                "This project is being built or removed; its files cannot be changed right now."
                    .to_string(),
        });
    }

    let result = if directory {
        project_host_file_manager::operations::create_directory(&root, &path)
    } else {
        project_host_file_manager::operations::create_file(&root, &path)
    };
    result.map(FileEntryDto::from).map_err(CommandError::from)
}

#[tauri::command]
async fn rename_project_file(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
    new_name: String,
) -> CommandResult<FileEntryDto> {
    let app: &AppState = &state;
    let (root, read_only) = project_root(app, &project_id).await?;
    if read_only {
        return Err(CommandError {
            message:
                "This project is being built or removed; its files cannot be changed right now."
                    .to_string(),
        });
    }

    project_host_file_manager::operations::rename(&root, &path, &new_name)
        .map(FileEntryDto::from)
        .map_err(CommandError::from)
}

#[tauri::command]
async fn delete_project_file(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
    recursive: bool,
) -> CommandResult<()> {
    let app: &AppState = &state;
    let (root, read_only) = project_root(app, &project_id).await?;
    if read_only {
        return Err(CommandError {
            message:
                "This project is being built or removed; its files cannot be changed right now."
                    .to_string(),
        });
    }

    project_host_file_manager::operations::delete(&root, &path, recursive)
        .map_err(CommandError::from)
}

#[tauri::command]
async fn search_project_files(
    state: tauri::State<'_, AppState>,
    project_id: String,
    query: String,
) -> CommandResult<Vec<FileEntryDto>> {
    let app: &AppState = &state;
    let (root, _) = project_root(app, &project_id).await?;
    project_host_file_manager::operations::search(&root, "", &query, &file_limits(app))
        .map(|entries| entries.into_iter().map(FileEntryDto::from).collect())
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
            check_for_update,
            supported_runtimes,
            github_cli_status,
            list_project_files,
            read_project_file,
            write_project_file,
            create_project_file,
            rename_project_file,
            delete_project_file,
            search_project_files
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
