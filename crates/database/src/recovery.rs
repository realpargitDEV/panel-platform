//! Startup recovery.
//!
//! The agent cannot assume it shut down cleanly. A power cut mid-restore
//! leaves a row saying `RESTORING` and a lock nobody holds; without this pass
//! the project would be permanently unusable and the operation permanently
//! ambiguous.
//!
//! Everything here is idempotent, so running recovery on an already-clean
//! database is a no-op rather than a corruption.

use crate::error::Result;
use crate::{locks, time, Database};

/// What recovery had to repair. Logged, and surfaced in health output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryReport {
    pub was_clean_shutdown: bool,
    pub integrity_ok: bool,
    pub locks_cleared: u64,
    pub deployments_interrupted: u64,
    pub backup_operations_interrupted: u64,
    pub projects_reset_from_transient: u64,
    pub sessions_pruned: u64,
}

impl RecoveryReport {
    /// True when nothing needed repairing.
    pub fn is_uneventful(&self) -> bool {
        self.locks_cleared == 0
            && self.deployments_interrupted == 0
            && self.backup_operations_interrupted == 0
            && self.projects_reset_from_transient == 0
    }
}

/// Repair state left behind by a previous run.
///
/// `was_clean_shutdown` decides how aggressive this is. After a clean stop only
/// expired leases are cleared; after a crash every lock is dropped, because the
/// process that held them is definitively gone.
pub async fn recover(database: &Database, was_clean_shutdown: bool) -> Result<RecoveryReport> {
    let mut report = RecoveryReport {
        was_clean_shutdown,
        ..RecoveryReport::default()
    };

    // An unclean stop can corrupt a database file. Checking costs a moment and
    // is the difference between a clear error and mystifying failures later.
    report.integrity_ok = if was_clean_shutdown {
        true
    } else {
        database.integrity_check().await?
    };

    report.locks_cleared = if was_clean_shutdown {
        locks::clear_expired(database).await?
    } else {
        locks::clear_all(database).await?
    };

    // A deployment stuck in a transient state cannot resume: the build context
    // and the process are gone. Marking it INTERRUPTED records what happened
    // instead of leaving a row that looks like it is still running.
    let deployments = sqlx::query(
        "UPDATE deployments
         SET status = 'INTERRUPTED',
             finished_at = ?,
             error_code = 'AGENT_RESTARTED',
             error_message = 'The agent stopped while this deployment was running.'
         WHERE status IN ('PENDING', 'BUILDING', 'STARTING')",
    )
    .bind(time::now())
    .execute(database.pool())
    .await?;
    report.deployments_interrupted = deployments.rows_affected();

    let backups = sqlx::query(
        "UPDATE backup_operations
         SET state = 'INTERRUPTED',
             finished_at = ?,
             error_code = 'AGENT_RESTARTED',
             error_message = 'The agent stopped while this operation was running.'
         WHERE state IN ('PENDING', 'RUNNING')",
    )
    .bind(time::now())
    .execute(database.pool())
    .await?;
    report.backup_operations_interrupted = backups.rows_affected();

    // A backup left half-written is not a backup. Marking it CORRUPT keeps it
    // visible — and unrestorable — rather than silently offering a broken
    // archive as a restore point.
    sqlx::query(
        "UPDATE project_backups SET status = 'CORRUPT'
         WHERE status IN ('PENDING', 'CREATING')",
    )
    .execute(database.pool())
    .await?;

    // Transient project states describe an action in progress. With the agent
    // restarted, none is. The reconciler (Phase 4) converges these against
    // Docker; until then they are reset so nothing appears mid-flight forever.
    let projects = sqlx::query(
        "UPDATE projects SET status = 'STOPPED', updated_at = ?
         WHERE status IN ('STARTING', 'STOPPING', 'RESTARTING', 'BUILDING', 'CREATING', 'DELETING')",
    )
    .bind(time::now())
    .execute(database.pool())
    .await?;
    report.projects_reset_from_transient = projects.rows_affected();

    // Housekeeping: sessions that expired more than a week ago.
    let cutoff = time::add_seconds(&time::now(), -7 * 86_400);
    report.sessions_pruned = crate::queries::prune_expired_sessions(database, &cutoff).await?;

    Ok(report)
}
