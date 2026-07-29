//! The composition layer for creating a project's files.
//!
//! Three crates meet here and nowhere else:
//!
//! * `file-manager` fetches and validates, and knows nothing about projects.
//! * `database` stores rows, and knows nothing about fetching or encryption.
//! * `security` holds the key.
//!
//! It exists so the desktop shell stays a shell. Everything below can be tested
//! with `cargo test` — no window, no Docker — and the ordering that keeps a
//! failure from leaving a half-made project is written down in one place rather
//! than repeated at every call site.
//!
//! ## The ordering
//!
//! Files first, row second. A fetch that fails leaves nothing on disk (the
//! staging directory removes itself) and nothing in the database, because the
//! row has not been written yet. The reverse order would leave a project the
//! user can see and cannot use whenever a remote is unreachable.
//!
//! ## Tokens
//!
//! A token is used for the fetch and stored only if a key is available to
//! encrypt it with. Today none is: nothing in this application yet holds an
//! [`EncryptionKey`] at runtime — the key store is not built. So
//! [`store_source_token`] is called with `None`, the token is used once and
//! dropped, and [`SourceOutcome::credential_stored`] says `false` rather than
//! implying a secret was kept safe somewhere. When the key store lands, the
//! change is the argument at the call site.

use std::path::{Path, PathBuf};

use project_host_database::source_credentials::{self, SourceCredentialRecord};
use project_host_database::{Database, DatabaseError};
use project_host_file_manager::git_clone::{clone_project, CloneError, CloneLimits, CloneRequest};
use project_host_file_manager::http_archive::{
    import_remote_archive, FetchError, FetchLimits, RemoteArchiveRequest, ReqwestTransport,
};
use project_host_file_manager::remote_url::SystemResolver;
use project_host_file_manager::zip_import::ArchiveLimits;
use project_host_security::encryption::associated_data;
use project_host_security::{decrypt, encrypt, Ciphertext, EncryptionKey, Secret};

/// Names the associated data a source token is bound to, so a row moved to
/// another project will not decrypt.
const CREDENTIAL_PURPOSE: &str = "SOURCE_CREDENTIAL";

#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("{0}")]
    Clone(#[from] CloneError),
    #[error("{0}")]
    Fetch(#[from] FetchError),
    #[error("could not create the project directory: {0}")]
    Io(String),
    #[error("could not encrypt the access token")]
    Encrypt,
    /// The fetch ran on a worker thread and that thread died. Reported rather
    /// than silently retried, because the cause is a panic worth seeing.
    #[error("the fetch did not finish")]
    Interrupted,
}

/// Where a new project's files come from.
///
/// `LOCAL_FOLDER`, `ZIP_UPLOAD` and `DUPLICATE` are absent because the
/// application does not offer them yet — the file-manager can do all three, but
/// nothing in the interface asks it to. Adding one here is a variant and a match
/// arm, not a redesign.
#[derive(Debug, Clone)]
pub enum SourceSpec {
    /// An empty directory. What the creation dialog did before remote sources
    /// existed.
    Empty,
    Git {
        url: String,
        /// Branch or tag. `None` means the remote's default branch.
        git_ref: Option<String>,
        subdirectory: Option<String>,
        token: Option<Secret<String>>,
    },
    Archive {
        url: String,
        token: Option<Secret<String>>,
    },
}

impl SourceSpec {
    /// The `SourceType` this spec becomes in the database.
    pub fn source_type(&self) -> &'static str {
        match self {
            SourceSpec::Empty => "EMPTY",
            SourceSpec::Git { .. } => "GIT_CLONE",
            SourceSpec::Archive { .. } => "REMOTE_ARCHIVE",
        }
    }

    /// The URL, for the provenance column. `None` for a local source, which the
    /// schema requires.
    pub fn url(&self) -> Option<&str> {
        match self {
            SourceSpec::Empty => None,
            SourceSpec::Git { url, .. } | SourceSpec::Archive { url, .. } => Some(url),
        }
    }

    fn token(&self) -> Option<&Secret<String>> {
        match self {
            SourceSpec::Empty => None,
            SourceSpec::Git { token, .. } | SourceSpec::Archive { token, .. } => token.as_ref(),
        }
    }
}

/// What materialising a source produced: exactly the provenance columns, plus
/// what became of the token.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceOutcome {
    pub source_url: Option<String>,
    pub source_ref: Option<String>,
    pub source_commit: Option<String>,
    /// False when no token was supplied *and* when one was supplied but could
    /// not be stored. The distinction the user needs is in
    /// [`SourceOutcome::token_used_but_not_stored`].
    pub credential_stored: bool,
    /// True when a token authenticated the fetch and was then dropped because
    /// there is no key to encrypt it with. The interface says so rather than
    /// leaving the user to assume it was saved.
    pub token_used_but_not_stored: bool,
}

/// Put a project's files in place.
///
/// `directory` must not exist. `staging_root` must be on the same filesystem as
/// `directory`, because promotion is a rename and a rename is atomic only within
/// one filesystem.
///
/// The fetch is blocking work — `gix` and the HTTP client both are — so it runs
/// on a blocking worker rather than on an async thread that other commands share.
pub async fn materialise_source(
    spec: &SourceSpec,
    directory: &Path,
    staging_root: &Path,
    fetch_id: &str,
) -> Result<SourceOutcome, ProvisionError> {
    match spec {
        SourceSpec::Empty => {
            std::fs::create_dir_all(directory).map_err(|e| ProvisionError::Io(e.to_string()))?;
            Ok(SourceOutcome::default())
        }
        SourceSpec::Git {
            url,
            git_ref,
            subdirectory,
            token,
        } => {
            let report = run_clone(
                url.clone(),
                git_ref.clone(),
                subdirectory.clone(),
                token.as_ref().map(|t| t.expose().clone()),
                directory.to_path_buf(),
                staging_root.to_path_buf(),
                fetch_id.to_string(),
            )
            .await?;

            Ok(SourceOutcome {
                source_url: Some(url.clone()),
                source_ref: git_ref.clone(),
                source_commit: Some(report.commit),
                ..SourceOutcome::default()
            })
        }
        SourceSpec::Archive { url, token } => {
            run_archive(
                url.clone(),
                token.as_ref().map(|t| t.expose().clone()),
                directory.to_path_buf(),
                staging_root.to_path_buf(),
                fetch_id.to_string(),
            )
            .await?;

            Ok(SourceOutcome {
                source_url: Some(url.clone()),
                // A ref and a commit belong to a git clone and to nothing else;
                // the schema refuses them here.
                ..SourceOutcome::default()
            })
        }
    }
}

/// Owned arguments, because the closure outlives this frame.
#[allow(clippy::too_many_arguments)]
async fn run_clone(
    url: String,
    git_ref: Option<String>,
    subdirectory: Option<String>,
    token: Option<String>,
    directory: PathBuf,
    staging_root: PathBuf,
    fetch_id: String,
) -> Result<project_host_file_manager::git_clone::CloneReport, ProvisionError> {
    tokio::task::spawn_blocking(move || {
        clone_project(
            &CloneRequest {
                url: &url,
                git_ref: git_ref.as_deref(),
                subdirectory: subdirectory.as_deref(),
                token: token.as_deref(),
                staging_root: &staging_root,
                destination: &directory,
                clone_id: &fetch_id,
                limits: CloneLimits::default(),
            },
            &SystemResolver,
        )
    })
    .await
    .map_err(|_| ProvisionError::Interrupted)?
    .map_err(ProvisionError::from)
}

async fn run_archive(
    url: String,
    token: Option<String>,
    directory: PathBuf,
    staging_root: PathBuf,
    fetch_id: String,
) -> Result<(), ProvisionError> {
    tokio::task::spawn_blocking(move || {
        import_remote_archive(
            &RemoteArchiveRequest {
                url: &url,
                token: token.as_deref(),
                staging_root: &staging_root,
                destination: &directory,
                import_id: &fetch_id,
                fetch_limits: FetchLimits::default(),
                archive_limits: ArchiveLimits::default(),
            },
            &ReqwestTransport,
            &SystemResolver,
        )
    })
    .await
    .map_err(|_| ProvisionError::Interrupted)?
    .map(|_report| ())
    .map_err(ProvisionError::from)
}

/// Store a project's access token, if there is a key to encrypt it with.
///
/// Returns what the caller should report. Passing `None` for the key is not an
/// error: it is the current state of this application, and the honest answer is
/// "the token was used and not kept", not a stored blob nobody can read.
pub async fn store_source_token(
    database: &Database,
    key: Option<&EncryptionKey>,
    project_id: &str,
    spec: &SourceSpec,
) -> Result<SourceOutcome, ProvisionError> {
    let Some(token) = spec.token() else {
        return Ok(SourceOutcome::default());
    };

    let Some(key) = key else {
        return Ok(SourceOutcome {
            token_used_but_not_stored: true,
            ..SourceOutcome::default()
        });
    };

    let ciphertext = encrypt(key, token, &associated_data(project_id, CREDENTIAL_PURPOSE))
        .map_err(|_| ProvisionError::Encrypt)?;

    source_credentials::save_source_credential(
        database,
        &SourceCredentialRecord {
            project_id: project_id.to_string(),
            ciphertext: ciphertext.bytes,
            nonce: ciphertext.nonce,
        },
    )
    .await?;

    Ok(SourceOutcome {
        credential_stored: true,
        ..SourceOutcome::default()
    })
}

/// Recover a project's access token.
///
/// Nothing calls this yet. It is the half of the pair that "update this project
/// from its remote" will need, and it lives here rather than being invented
/// later somewhere else, next to the encryption it has to match.
pub async fn load_source_token(
    database: &Database,
    key: &EncryptionKey,
    project_id: &str,
) -> Result<Option<Secret<String>>, ProvisionError> {
    let Some(record) = source_credentials::load_source_credential(database, project_id).await?
    else {
        return Ok(None);
    };

    let plaintext = decrypt(
        key,
        &Ciphertext {
            bytes: record.ciphertext,
            nonce: record.nonce,
        },
        &associated_data(project_id, CREDENTIAL_PURPOSE),
    )
    .map_err(|_| ProvisionError::Encrypt)?;

    Ok(Some(plaintext))
}

/// Remove a directory a failed creation left behind.
///
/// Called when the fetch succeeded and writing the row did not. Best-effort by
/// design: a failure to clean up must not replace the error the caller is about
/// to report with a less useful one.
pub fn discard_directory(directory: &Path) {
    if let Err(error) = std::fs::remove_dir_all(directory) {
        tracing::warn!(
            %error,
            directory = %directory.display(),
            "could not remove the directory of a project that failed to be created"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_empty_source_makes_an_empty_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let directory = root.path().join("projects/prj_1");

        let outcome = materialise_source(&SourceSpec::Empty, &directory, root.path(), "empty-test")
            .await
            .expect("an empty source cannot fail");

        assert!(directory.is_dir());
        assert_eq!(outcome, SourceOutcome::default());
        assert_eq!(std::fs::read_dir(&directory).expect("read").count(), 0);
    }

    #[test]
    fn a_spec_reports_the_source_type_the_schema_expects() {
        assert_eq!(SourceSpec::Empty.source_type(), "EMPTY");
        assert_eq!(
            SourceSpec::Git {
                url: "https://github.com/o/r.git".to_string(),
                git_ref: None,
                subdirectory: None,
                token: None,
            }
            .source_type(),
            "GIT_CLONE"
        );
        assert_eq!(
            SourceSpec::Archive {
                url: "https://example.com/r.zip".to_string(),
                token: None,
            }
            .source_type(),
            "REMOTE_ARCHIVE"
        );
    }

    #[test]
    fn only_a_remote_spec_carries_a_url() {
        // The schema has a CHECK for exactly this, so disagreeing with it here
        // would be an insert that fails at the last moment.
        assert_eq!(SourceSpec::Empty.url(), None);
        assert_eq!(
            SourceSpec::Archive {
                url: "https://example.com/r.zip".to_string(),
                token: None,
            }
            .url(),
            Some("https://example.com/r.zip")
        );
    }

    #[tokio::test]
    async fn a_git_source_refuses_a_url_that_never_passes_validation() {
        // No network is reached: the URL is rejected before a connection is
        // opened, which is what makes this testable here at all.
        let root = tempfile::tempdir().expect("temp dir");
        let result = materialise_source(
            &SourceSpec::Git {
                url: "http://github.com/owner/repo.git".to_string(),
                git_ref: None,
                subdirectory: None,
                token: None,
            },
            &root.path().join("projects/prj_2"),
            root.path(),
            "scheme-test",
        )
        .await;

        assert!(
            matches!(result, Err(ProvisionError::Clone(_))),
            "{result:?}"
        );
        assert!(!root.path().join("projects/prj_2").exists());
    }

    #[tokio::test]
    async fn an_archive_source_refuses_a_url_that_never_passes_validation() {
        let root = tempfile::tempdir().expect("temp dir");
        let result = materialise_source(
            &SourceSpec::Archive {
                url: "file:///C:/Windows/System32".to_string(),
                token: None,
            },
            &root.path().join("projects/prj_3"),
            root.path(),
            "scheme-test-2",
        )
        .await;

        assert!(
            matches!(result, Err(ProvisionError::Fetch(_))),
            "{result:?}"
        );
        assert!(!root.path().join("projects/prj_3").exists());
    }

    #[tokio::test]
    async fn a_source_with_no_token_stores_nothing() {
        let database = Database::open_in_memory().await.expect("open");
        let outcome = store_source_token(&database, None, "prj_1", &SourceSpec::Empty)
            .await
            .expect("no token, no work");
        assert!(!outcome.credential_stored);
        assert!(!outcome.token_used_but_not_stored);
    }

    #[tokio::test]
    async fn a_token_with_no_key_is_reported_as_used_but_not_stored() {
        // The current state of the application, stated rather than glossed over.
        let database = Database::open_in_memory().await.expect("open");
        let spec = SourceSpec::Git {
            url: "https://github.com/owner/private.git".to_string(),
            git_ref: None,
            subdirectory: None,
            token: Some(Secret::new("ghp_token".to_string())),
        };

        let outcome = store_source_token(&database, None, "prj_1", &spec)
            .await
            .expect("not an error");

        assert!(!outcome.credential_stored);
        assert!(outcome.token_used_but_not_stored);

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_source_credentials")
            .fetch_one(database.pool())
            .await
            .expect("count");
        assert_eq!(rows, 0, "a token was written with no key to protect it");
    }

    #[test]
    fn discarding_a_directory_that_is_not_there_is_not_a_panic() {
        // Called on the error path, where the directory may already be gone.
        discard_directory(Path::new("no/such/directory/anywhere"));
    }
}
