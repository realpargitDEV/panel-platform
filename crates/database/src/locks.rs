//! Operation locks.
//!
//! A project may have exactly one in-flight destructive operation. That is
//! enforced by `project_locks.project_id` being the primary key, so a second
//! acquisition is refused by the database rather than by a check that could
//! race. See `docs/database-schema.md` §7.

use crate::error::{DatabaseError, Result};
use crate::time;
use crate::Database;
use sqlx::Row;

/// What holds a lock. Recorded so the refusal can say what to wait for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Start,
    Stop,
    Restart,
    Rebuild,
    Delete,
    Restore,
    Backup,
    Import,
}

impl Operation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Operation::Start => "START",
            Operation::Stop => "STOP",
            Operation::Restart => "RESTART",
            Operation::Rebuild => "REBUILD",
            Operation::Delete => "DELETE",
            Operation::Restore => "RESTORE",
            Operation::Backup => "BACKUP",
            Operation::Import => "IMPORT",
        }
    }
}

/// Why a lock could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHeld {
    pub operation: String,
    pub operation_id: String,
    pub acquired_at: String,
}

/// Default lease. A lock outliving this is treated as abandoned, which is what
/// stops a crashed agent leaving a project locked forever.
pub const DEFAULT_LEASE_SECONDS: i64 = 3600;

/// Try to take the lock. `Ok(None)` means someone else holds it.
pub async fn acquire(
    database: &Database,
    project_id: &str,
    operation: Operation,
    operation_id: &str,
    lease_seconds: i64,
) -> Result<Option<LockHeld>> {
    // Clear an expired lease first, so a crashed predecessor does not block
    // progress forever.
    let now = time::now();
    sqlx::query("DELETE FROM project_locks WHERE project_id = ? AND expires_at <= ?")
        .bind(project_id)
        .bind(&now)
        .execute(database.pool())
        .await?;

    let expires_at = time::add_seconds(&now, lease_seconds);
    let result = sqlx::query(
        "INSERT INTO project_locks (project_id, operation, operation_id, acquired_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(project_id)
    .bind(operation.as_str())
    .bind(operation_id)
    .bind(&now)
    .bind(&expires_at)
    .execute(database.pool())
    .await;

    match result {
        Ok(_) => Ok(None),
        Err(error) => match DatabaseError::from(error) {
            // The primary key refused it: somebody else is mid-operation.
            DatabaseError::UniqueViolation { .. } => {
                match current_holder(database, project_id).await? {
                    Some(held) => Ok(Some(held)),
                    // Released between the failed insert and this read. Treat as
                    // contention rather than success: the caller retries, which is
                    // correct and cheap.
                    None => Ok(Some(LockHeld {
                        operation: "UNKNOWN".to_string(),
                        operation_id: String::new(),
                        acquired_at: now,
                    })),
                }
            }
            other => Err(other),
        },
    }
}

/// Release a lock, but only if this operation still owns it.
///
/// The `operation_id` check matters: without it, a slow operation whose lease
/// expired could release a lock that a newer operation had since taken.
pub async fn release(database: &Database, project_id: &str, operation_id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM project_locks WHERE project_id = ? AND operation_id = ?")
        .bind(project_id)
        .bind(operation_id)
        .execute(database.pool())
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn current_holder(database: &Database, project_id: &str) -> Result<Option<LockHeld>> {
    let row = sqlx::query(
        "SELECT operation, operation_id, acquired_at FROM project_locks WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(database.pool())
    .await?;

    Ok(row.map(|row| LockHeld {
        operation: row.get("operation"),
        operation_id: row.get("operation_id"),
        acquired_at: row.get("acquired_at"),
    }))
}

/// Drop every expired lease. Run during startup recovery.
pub async fn clear_expired(database: &Database) -> Result<u64> {
    let result = sqlx::query("DELETE FROM project_locks WHERE expires_at <= ?")
        .bind(time::now())
        .execute(database.pool())
        .await?;
    Ok(result.rows_affected())
}

/// Drop every lock regardless of lease.
///
/// Only correct immediately after an unclean start, when no operation from the
/// previous process can still be running — the process that held them is gone.
pub async fn clear_all(database: &Database) -> Result<u64> {
    let result = sqlx::query("DELETE FROM project_locks")
        .execute(database.pool())
        .await?;
    Ok(result.rows_affected())
}
