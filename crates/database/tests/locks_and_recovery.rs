//! Operation locking and startup recovery, against a real SQLite database.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use project_host_api_types::{OperationId, ProjectId};
use project_host_database::locks::{self, Operation};
use project_host_database::{queries, recover, time, Database};

async fn db() -> Database {
    Database::open_in_memory().await.expect("open")
}

async fn insert_project(database: &Database) -> String {
    let id = ProjectId::generate().to_string();
    let now = time::now();
    sqlx::query(
        "INSERT INTO projects (id, slug, display_name, project_type, source_type,
                               directory, created_at, updated_at)
         VALUES (?, ?, 'p', 'NODE_APP', 'EMPTY', ?, ?, ?)",
    )
    .bind(&id)
    .bind(format!("proj-{}", &id[4..]))
    .bind(format!("/projects/{id}"))
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await
    .expect("insert project");
    id
}

// ---------------------------------------------------------------- locking

#[tokio::test]
async fn a_lock_can_be_taken_and_released() {
    let database = db().await;
    let project = insert_project(&database).await;
    let operation = OperationId::generate().to_string();

    let contention = locks::acquire(&database, &project, Operation::Restore, &operation, 60)
        .await
        .expect("acquire");
    assert!(contention.is_none(), "the first acquisition should succeed");

    assert!(locks::release(&database, &project, &operation)
        .await
        .expect("release"));
    assert!(locks::current_holder(&database, &project)
        .await
        .expect("holder")
        .is_none());
}

#[tokio::test]
async fn a_second_operation_is_refused_and_told_what_holds_it() {
    let database = db().await;
    let project = insert_project(&database).await;
    let first = OperationId::generate().to_string();

    locks::acquire(&database, &project, Operation::Restore, &first, 60)
        .await
        .expect("first");

    let held = locks::acquire(
        &database,
        &project,
        Operation::Rebuild,
        &OperationId::generate().to_string(),
        60,
    )
    .await
    .expect("second")
    .expect("the second acquisition must be refused");

    // The refusal has to say what to wait for, or the UI can only say "busy".
    assert_eq!(held.operation, "RESTORE");
    assert_eq!(held.operation_id, first);
}

#[tokio::test]
async fn releasing_with_the_wrong_operation_id_does_nothing() {
    // Otherwise a slow operation whose lease expired could release a lock a
    // newer operation had since taken.
    let database = db().await;
    let project = insert_project(&database).await;
    let owner = OperationId::generate().to_string();

    locks::acquire(&database, &project, Operation::Restore, &owner, 60)
        .await
        .expect("acquire");

    let released = locks::release(&database, &project, "op_someone_else")
        .await
        .expect("release");
    assert!(!released, "a non-owner must not release the lock");
    assert!(locks::current_holder(&database, &project)
        .await
        .expect("holder")
        .is_some());
}

#[tokio::test]
async fn an_expired_lease_does_not_block_forever() {
    let database = db().await;
    let project = insert_project(&database).await;

    // A lease of zero seconds is already expired.
    locks::acquire(
        &database,
        &project,
        Operation::Restore,
        &OperationId::generate().to_string(),
        0,
    )
    .await
    .expect("acquire");

    let contention = locks::acquire(
        &database,
        &project,
        Operation::Rebuild,
        &OperationId::generate().to_string(),
        60,
    )
    .await
    .expect("acquire after expiry");
    assert!(
        contention.is_none(),
        "an expired lease must not block a new operation"
    );
}

#[tokio::test]
async fn locks_are_per_project() {
    let database = db().await;
    let first = insert_project(&database).await;
    let second = insert_project(&database).await;

    locks::acquire(
        &database,
        &first,
        Operation::Restore,
        &OperationId::generate().to_string(),
        60,
    )
    .await
    .expect("first");

    let contention = locks::acquire(
        &database,
        &second,
        Operation::Restore,
        &OperationId::generate().to_string(),
        60,
    )
    .await
    .expect("second");
    assert!(
        contention.is_none(),
        "one project's lock must not block another"
    );
}

// --------------------------------------------------------------- recovery

#[tokio::test]
async fn recovery_after_a_clean_stop_is_uneventful() {
    let database = db().await;
    let report = recover(&database, true).await.expect("recover");
    assert!(report.is_uneventful());
    assert!(report.was_clean_shutdown);
}

#[tokio::test]
async fn a_crash_drops_every_lock() {
    let database = db().await;
    let project = insert_project(&database).await;

    // A long lease that has not expired: after a clean stop it would survive,
    // but after a crash the process holding it is gone.
    locks::acquire(
        &database,
        &project,
        Operation::Restore,
        &OperationId::generate().to_string(),
        3600,
    )
    .await
    .expect("acquire");

    let report = recover(&database, false).await.expect("recover");
    assert_eq!(report.locks_cleared, 1);
    assert!(locks::current_holder(&database, &project)
        .await
        .expect("holder")
        .is_none());
}

#[tokio::test]
async fn a_clean_stop_keeps_unexpired_locks() {
    let database = db().await;
    let project = insert_project(&database).await;
    locks::acquire(
        &database,
        &project,
        Operation::Restore,
        &OperationId::generate().to_string(),
        3600,
    )
    .await
    .expect("acquire");

    let report = recover(&database, true).await.expect("recover");
    assert_eq!(report.locks_cleared, 0);
}

#[tokio::test]
async fn in_flight_deployments_become_interrupted() {
    let database = db().await;
    let project = insert_project(&database).await;

    for status in ["PENDING", "BUILDING", "STARTING"] {
        sqlx::query(
            "INSERT INTO deployments (id, project_id, type, status, started_at)
             VALUES (?, ?, 'INITIAL', ?, ?)",
        )
        .bind(OperationId::generate().to_string())
        .bind(&project)
        .bind(status)
        .bind(time::now())
        .execute(database.pool())
        .await
        .expect("insert deployment");
    }

    let report = recover(&database, false).await.expect("recover");
    assert_eq!(report.deployments_interrupted, 3);

    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM deployments WHERE status IN ('PENDING','BUILDING','STARTING')",
    )
    .fetch_one(database.pool())
    .await
    .expect("count");
    assert_eq!(remaining, 0);

    let code: String = sqlx::query_scalar("SELECT error_code FROM deployments LIMIT 1")
        .fetch_one(database.pool())
        .await
        .expect("error code");
    assert_eq!(code, "AGENT_RESTARTED");
}

#[tokio::test]
async fn a_half_written_backup_is_marked_corrupt_not_offered_for_restore() {
    let database = db().await;
    let project = insert_project(&database).await;

    sqlx::query(
        "INSERT INTO project_backups (id, project_id, status, created_at)
         VALUES (?, ?, 'CREATING', ?)",
    )
    .bind(OperationId::generate().to_string())
    .bind(&project)
    .bind(time::now())
    .execute(database.pool())
    .await
    .expect("insert backup");

    recover(&database, false).await.expect("recover");

    let status: String = sqlx::query_scalar("SELECT status FROM project_backups LIMIT 1")
        .fetch_one(database.pool())
        .await
        .expect("status");
    assert_eq!(
        status, "CORRUPT",
        "a partial archive must not look restorable"
    );
}

#[tokio::test]
async fn transient_project_states_are_reset() {
    let database = db().await;
    let project = insert_project(&database).await;

    sqlx::query("UPDATE projects SET status = 'STARTING' WHERE id = ?")
        .bind(&project)
        .execute(database.pool())
        .await
        .expect("set status");

    let report = recover(&database, false).await.expect("recover");
    assert_eq!(report.projects_reset_from_transient, 1);

    let status: String = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(&project)
        .fetch_one(database.pool())
        .await
        .expect("status");
    assert_eq!(status, "STOPPED");
}

#[tokio::test]
async fn recovery_is_idempotent() {
    let database = db().await;
    let project = insert_project(&database).await;
    sqlx::query("UPDATE projects SET status = 'BUILDING' WHERE id = ?")
        .bind(&project)
        .execute(database.pool())
        .await
        .expect("set status");

    let first = recover(&database, false).await.expect("first");
    assert_eq!(first.projects_reset_from_transient, 1);

    let second = recover(&database, false).await.expect("second");
    assert!(
        second.is_uneventful(),
        "a second pass must find nothing to do"
    );
}

#[tokio::test]
async fn an_unclean_start_runs_an_integrity_check() {
    let database = db().await;
    let report = recover(&database, false).await.expect("recover");
    assert!(report.integrity_ok);
}

// -------------------------------------------------------------- agent state

#[tokio::test]
async fn the_first_start_is_treated_as_clean() {
    let database = db().await;
    let clean = queries::begin_agent_session(&database, "0.1.0", 1, "inst-1", "127.0.0.1:8787")
        .await
        .expect("begin");
    assert!(clean, "a fresh install has nothing to recover");
}

#[tokio::test]
async fn a_missing_clean_shutdown_flag_is_detected_on_the_next_start() {
    let database = db().await;
    queries::begin_agent_session(&database, "0.1.0", 1, "inst-1", "127.0.0.1:8787")
        .await
        .expect("first start");

    // No record_clean_shutdown: simulate a crash.
    let clean = queries::begin_agent_session(&database, "0.1.0", 1, "inst-2", "127.0.0.1:8787")
        .await
        .expect("second start");
    assert!(!clean, "a crash must be detected");
}

#[tokio::test]
async fn a_clean_shutdown_is_remembered() {
    let database = db().await;
    queries::begin_agent_session(&database, "0.1.0", 1, "inst-1", "127.0.0.1:8787")
        .await
        .expect("start");
    queries::record_clean_shutdown(&database)
        .await
        .expect("shutdown");

    let clean = queries::begin_agent_session(&database, "0.1.0", 1, "inst-2", "127.0.0.1:8787")
        .await
        .expect("restart");
    assert!(clean);
}

#[tokio::test]
async fn agent_state_holds_only_the_latest_instance() {
    let database = db().await;
    queries::begin_agent_session(&database, "0.1.0", 1, "inst-1", "127.0.0.1:8787")
        .await
        .expect("first");
    queries::begin_agent_session(&database, "0.1.0", 1, "inst-2", "127.0.0.1:8787")
        .await
        .expect("second");

    assert_eq!(
        queries::agent_instance_id(&database).await.expect("id"),
        "inst-2"
    );
}
