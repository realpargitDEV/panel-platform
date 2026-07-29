//! Database errors, mapped to the stable API codes the client understands.

use project_host_api_types::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("database connection failed: {0}")]
    Connection(#[source] sqlx::Error),

    #[error("migration failed: {0}")]
    Migration(#[source] sqlx::migrate::MigrateError),

    /// The file was written by a newer build. Continuing would risk writing
    /// rows the older code cannot read back, so the agent refuses to start.
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },

    #[error("no such {entity}")]
    NotFound { entity: &'static str },

    #[error("{constraint} already exists")]
    UniqueViolation { constraint: String },

    #[error("a referenced record does not exist")]
    ForeignKeyViolation,

    /// A CHECK constraint refused the row — a secret with a plaintext value, a
    /// privileged port, a resource limit out of bounds.
    #[error("value rejected by a database constraint")]
    CheckViolation,

    #[error("query failed: {0}")]
    Query(#[source] sqlx::Error),

    /// A caller passed a value the schema would refuse.
    ///
    /// Checked before the statement runs so the error names the offending
    /// value, rather than surfacing as an opaque [`DatabaseError::CheckViolation`]
    /// from SQLite.
    #[error("invalid value: {0}")]
    Invalid(String),
}

impl DatabaseError {
    /// What the client is told. Deliberately coarse: the specific constraint
    /// name goes to the log, not to the caller.
    pub fn code(&self) -> ErrorCode {
        match self {
            DatabaseError::NotFound { .. } => ErrorCode::NotFound,
            DatabaseError::UniqueViolation { .. } => ErrorCode::Conflict,
            DatabaseError::ForeignKeyViolation => ErrorCode::Conflict,
            DatabaseError::CheckViolation | DatabaseError::Invalid(_) => {
                ErrorCode::ValidationFailed
            }
            DatabaseError::SchemaTooNew { .. }
            | DatabaseError::Connection(_)
            | DatabaseError::Migration(_)
            | DatabaseError::Query(_) => ErrorCode::Internal,
        }
    }
}

/// Classify a driver error so callers can branch on meaning rather than on
/// substring matching at every call site.
impl From<sqlx::Error> for DatabaseError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_error) = error {
            let message = db_error.message().to_ascii_uppercase();
            // SQLite reports these as extended result codes in the message.
            if message.contains("UNIQUE CONSTRAINT FAILED") {
                let constraint = db_error
                    .message()
                    .split_once(':')
                    .map(|(_, rest)| rest.trim().to_string())
                    .unwrap_or_else(|| "record".to_string());
                return DatabaseError::UniqueViolation { constraint };
            }
            if message.contains("FOREIGN KEY CONSTRAINT FAILED") {
                return DatabaseError::ForeignKeyViolation;
            }
            if message.contains("CHECK CONSTRAINT FAILED") {
                return DatabaseError::CheckViolation;
            }
        }
        DatabaseError::Query(error)
    }
}

pub type Result<T> = std::result::Result<T, DatabaseError>;
