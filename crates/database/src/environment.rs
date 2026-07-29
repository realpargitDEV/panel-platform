//! Environment variable storage.
//!
//! This module never sees an encryption key. A secret arrives already encrypted
//! and leaves as ciphertext; the key lives in the platform keychain and the
//! encrypt/decrypt calls happen a layer up. That split is what makes the schema
//! `CHECK` meaningful — a secret with a plaintext value is refused by SQLite
//! itself, and there is no code path here that could construct one.
//!
//! The read used by the API is [`list_variables`], which returns
//! [`StoredValue::Secret`] carrying ciphertext. Decryption is a separate,
//! explicit call for the one place that needs it: assembling a container's
//! environment at start.

use project_host_api_types::EnvVarId;
use sqlx::Row;

use crate::error::{DatabaseError, Result};
use crate::time;
use crate::Database;

/// A stored value, in whichever form the schema allows for its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredValue {
    Plain(String),
    Secret { cipher: Vec<u8>, nonce: Vec<u8> },
}

impl StoredValue {
    pub fn is_secret(&self) -> bool {
        matches!(self, Self::Secret { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVarRecord {
    pub id: String,
    pub project_id: String,
    pub key: String,
    pub value: StoredValue,
    pub restart_required: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn value_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<StoredValue> {
    let is_secret: i64 = row.get("is_secret");
    if is_secret == 1 {
        let cipher: Option<Vec<u8>> = row.get("value_cipher");
        let nonce: Option<Vec<u8>> = row.get("value_nonce");
        // The schema forbids this combination, so reaching it means the file was
        // edited outside the agent. Refusing is better than returning an empty
        // secret that would then be handed to a container.
        match (cipher, nonce) {
            (Some(cipher), Some(nonce)) => Ok(StoredValue::Secret { cipher, nonce }),
            _ => Err(DatabaseError::CheckViolation),
        }
    } else {
        Ok(StoredValue::Plain(
            row.get::<Option<String>, _>("value_plain")
                .unwrap_or_default(),
        ))
    }
}

fn record_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<EnvVarRecord> {
    Ok(EnvVarRecord {
        id: row.get("id"),
        project_id: row.get("project_id"),
        key: row.get("key"),
        value: value_from_row(row)?,
        restart_required: row.get::<i64, _>("restart_required") == 1,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

const ENV_COLUMNS: &str = "id, project_id, key, value_plain, value_cipher, value_nonce,
     is_secret, restart_required, created_at, updated_at";

/// Insert or replace one variable.
///
/// Upsert rather than insert-then-update: the desktop client's editor saves a
/// row without knowing whether it already exists, and a check-then-insert would
/// be a race between two clients editing the same project.
///
/// Changing a variable's kind clears the other representation explicitly, so a
/// value that was once plaintext leaves no readable copy behind after it is
/// marked secret.
pub async fn upsert_variable(
    database: &Database,
    project_id: &str,
    key: &str,
    value: &StoredValue,
) -> Result<EnvVarRecord> {
    let now = time::now();
    let (plain, cipher, nonce, is_secret) = match value {
        StoredValue::Plain(text) => (Some(text.clone()), None, None, 0i64),
        StoredValue::Secret { cipher, nonce } => {
            (None, Some(cipher.clone()), Some(nonce.clone()), 1i64)
        }
    };

    sqlx::query(
        "INSERT INTO environment_variables
            (id, project_id, key, value_plain, value_cipher, value_nonce,
             is_secret, restart_required, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
         ON CONFLICT(project_id, key) DO UPDATE SET
             value_plain  = excluded.value_plain,
             value_cipher = excluded.value_cipher,
             value_nonce  = excluded.value_nonce,
             is_secret    = excluded.is_secret,
             updated_at   = excluded.updated_at",
    )
    .bind(EnvVarId::generate().to_string())
    .bind(project_id)
    .bind(key)
    .bind(plain)
    .bind(cipher)
    .bind(nonce)
    .bind(is_secret)
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await?;

    find_variable(database, project_id, key)
        .await?
        .ok_or(DatabaseError::NotFound {
            entity: "environment variable",
        })
}

/// Replace a project's entire set in one transaction.
///
/// Used by `.env` import and bulk edit. All-or-nothing matters: a half-applied
/// import would leave the project with a mixture of two configurations and no
/// indication of which variables came from where.
pub async fn replace_all(
    database: &Database,
    project_id: &str,
    variables: &[(String, StoredValue)],
) -> Result<u64> {
    let now = time::now();
    let mut transaction = database.pool().begin().await?;

    sqlx::query("DELETE FROM environment_variables WHERE project_id = ?")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;

    for (key, value) in variables {
        let (plain, cipher, nonce, is_secret) = match value {
            StoredValue::Plain(text) => (Some(text.clone()), None, None, 0i64),
            StoredValue::Secret { cipher, nonce } => {
                (None, Some(cipher.clone()), Some(nonce.clone()), 1i64)
            }
        };
        sqlx::query(
            "INSERT INTO environment_variables
                (id, project_id, key, value_plain, value_cipher, value_nonce,
                 is_secret, restart_required, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(EnvVarId::generate().to_string())
        .bind(project_id)
        .bind(key)
        .bind(plain)
        .bind(cipher)
        .bind(nonce)
        .bind(is_secret)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
    }

    transaction.commit().await?;
    Ok(variables.len() as u64)
}

pub async fn find_variable(
    database: &Database,
    project_id: &str,
    key: &str,
) -> Result<Option<EnvVarRecord>> {
    let row = sqlx::query(&format!(
        "SELECT {ENV_COLUMNS} FROM environment_variables WHERE project_id = ? AND key = ?"
    ))
    .bind(project_id)
    .bind(key)
    .fetch_optional(database.pool())
    .await?;

    row.as_ref().map(record_from_row).transpose()
}

/// Every variable for a project, ordered by name so the editor is stable.
pub async fn list_variables(database: &Database, project_id: &str) -> Result<Vec<EnvVarRecord>> {
    let rows = sqlx::query(&format!(
        "SELECT {ENV_COLUMNS} FROM environment_variables WHERE project_id = ? ORDER BY key"
    ))
    .bind(project_id)
    .fetch_all(database.pool())
    .await?;

    rows.iter().map(record_from_row).collect()
}

pub async fn delete_variable(database: &Database, project_id: &str, key: &str) -> Result<()> {
    let result = sqlx::query("DELETE FROM environment_variables WHERE project_id = ? AND key = ?")
        .bind(project_id)
        .bind(key)
        .execute(database.pool())
        .await?;
    if result.rows_affected() == 0 {
        return Err(DatabaseError::NotFound {
            entity: "environment variable",
        });
    }
    Ok(())
}

/// Count secrets, for the review step of the creation wizard and the project
/// summary — a figure that can be shown without touching any value.
pub async fn count_variables(database: &Database, project_id: &str) -> Result<(i64, i64)> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS total, COALESCE(SUM(is_secret), 0) AS secrets
         FROM environment_variables WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_one(database.pool())
    .await?;
    Ok((row.get("total"), row.get("secrets")))
}
