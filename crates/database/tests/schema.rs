//! Integration tests against a real SQLite database.
//!
//! These need no Docker and no Linux, so they run everywhere — which is the
//! point: the constraints that protect secrets, ports and concurrency are
//! verified on any developer machine, not only on the release hardware.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use project_host_api_types::*;
use project_host_database::schema_parity::{check_values, table_body};
use project_host_database::{time, Database, DatabaseError, INITIAL_MIGRATION};
use sqlx::Row;

async fn db() -> Database {
    Database::open_in_memory()
        .await
        .expect("in-memory database should open and migrate")
}

/// Insert a minimal valid project and return its id.
async fn insert_project(database: &Database) -> String {
    let id = ProjectId::generate().to_string();
    let now = time::now();
    sqlx::query(
        "INSERT INTO projects (id, slug, display_name, project_type, source_type,
                               directory, created_at, updated_at)
         VALUES (?, ?, ?, 'NODE_APP', 'EMPTY', ?, ?, ?)",
    )
    .bind(&id)
    // The whole UUID body, not a prefix: two UUIDv7 values minted in the same
    // millisecond share their leading bytes and would collide on the slug.
    .bind(format!("proj-{}", &id[4..]))
    .bind("Test Project")
    .bind(format!("/var/lib/project-host/projects/{id}"))
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await
    .expect("project insert should succeed");
    id
}

// ---------------------------------------------------------------- migrations

#[tokio::test]
async fn migrations_apply_and_report_their_version() {
    let database = db().await;
    // Tied to the constant rather than a literal: a new migration that forgets
    // to bump `SUPPORTED_SCHEMA_VERSION` is the bug worth catching here, and a
    // literal would instead fail every time a migration is added correctly.
    assert_eq!(
        database.schema_version().await.unwrap(),
        project_host_database::SUPPORTED_SCHEMA_VERSION
    );
    database.assert_schema_supported().await.unwrap();
}

#[tokio::test]
async fn every_expected_table_exists() {
    let database = db().await;
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
        .fetch_all(database.pool())
        .await
        .unwrap();
    let tables: Vec<String> = rows
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();

    for expected in [
        "users",
        "sessions",
        "trusted_clients",
        "recovery_codes",
        "pairing_codes",
        "projects",
        "project_runtimes",
        "project_ports",
        "deployments",
        "container_events",
        "environment_variables",
        "project_backups",
        "backup_operations",
        "project_locks",
        "audit_logs",
        "project_metrics",
        "system_metrics",
        "notifications",
        "system_settings",
        "agent_state",
    ] {
        assert!(
            tables.iter().any(|name| name == expected),
            "missing table {expected}"
        );
    }
}

#[tokio::test]
async fn a_fresh_database_passes_its_integrity_check() {
    assert!(db().await.integrity_check().await.unwrap());
}

// ------------------------------------------------------------ enum parity

/// Each Rust enum must list exactly the values its `CHECK` constraint allows.
/// A variant added on one side only would otherwise fail at insert time, in
/// production, on the one code path that finally uses it.
#[test]
fn rust_enums_match_the_check_constraints() {
    fn assert_parity(table: &str, column: &str, variants: &[&str]) {
        let body = table_body(INITIAL_MIGRATION, table)
            .unwrap_or_else(|| panic!("no CREATE TABLE for {table}"));
        let allowed = check_values(body, column)
            .unwrap_or_else(|| panic!("no CHECK list for {table}.{column}"));

        let mut expected: Vec<&str> = variants.to_vec();
        let mut actual: Vec<&str> = allowed.iter().map(String::as_str).collect();
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(
            actual, expected,
            "{table}.{column} disagrees with its Rust enum"
        );
    }

    let as_strs = |values: &[&'static str]| values.to_vec();

    assert_parity("users", "role", &as_strs(&["ADMIN"]));

    assert_parity(
        "projects",
        "project_type",
        &ProjectType::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "projects",
        "status",
        &ProjectStatus::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "projects",
        "desired_state",
        &DesiredState::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "projects",
        "health",
        &HealthState::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "projects",
        "restart_policy",
        &RestartPolicy::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "projects",
        "network_mode",
        &NetworkMode::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "projects",
        "source_type",
        &SourceType::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "project_runtimes",
        "runtime",
        &Runtime::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
    );
    assert_parity(
        "project_runtimes",
        "package_manager",
        &PackageManager::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "project_runtimes",
        "health_check_type",
        &HealthCheckType::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "deployments",
        "type",
        &DeploymentType::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "deployments",
        "status",
        &DeploymentStatus::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "container_events",
        "type",
        &ContainerEventType::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "project_backups",
        "status",
        &BackupStatus::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "backup_operations",
        "kind",
        &BackupOperationKind::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "backup_operations",
        "state",
        &BackupOperationState::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "audit_logs",
        "result",
        &AuditResult::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
    assert_parity(
        "notifications",
        "level",
        &NotificationLevel::ALL
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>(),
    );
}

// ------------------------------------------------------- secret constraint

#[tokio::test]
async fn a_secret_cannot_be_stored_in_plaintext() {
    let database = db().await;
    let project = insert_project(&database).await;
    let now = time::now();

    let result = sqlx::query(
        "INSERT INTO environment_variables
           (id, project_id, key, value_plain, is_secret, created_at, updated_at)
         VALUES (?, ?, 'DISCORD_TOKEN', 'a-real-token', 1, ?, ?)",
    )
    .bind(EnvVarId::generate().to_string())
    .bind(&project)
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await;

    let error = DatabaseError::from(result.unwrap_err());
    assert!(
        matches!(error, DatabaseError::CheckViolation),
        "a plaintext secret must be refused by the database, got {error:?}"
    );
}

#[tokio::test]
async fn a_non_secret_cannot_be_stored_encrypted() {
    let database = db().await;
    let project = insert_project(&database).await;
    let now = time::now();

    let result = sqlx::query(
        "INSERT INTO environment_variables
           (id, project_id, key, value_cipher, value_nonce, is_secret, created_at, updated_at)
         VALUES (?, ?, 'PORT', X'00', X'01', 0, ?, ?)",
    )
    .bind(EnvVarId::generate().to_string())
    .bind(&project)
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await;

    assert!(matches!(
        DatabaseError::from(result.unwrap_err()),
        DatabaseError::CheckViolation
    ));
}

#[tokio::test]
async fn a_well_formed_secret_is_accepted() {
    let database = db().await;
    let project = insert_project(&database).await;
    let now = time::now();

    sqlx::query(
        "INSERT INTO environment_variables
           (id, project_id, key, value_cipher, value_nonce, is_secret, created_at, updated_at)
         VALUES (?, ?, 'DISCORD_TOKEN', X'DEADBEEF', X'0102', 1, ?, ?)",
    )
    .bind(EnvVarId::generate().to_string())
    .bind(&project)
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await
    .expect("an encrypted secret should be accepted");
}

#[tokio::test]
async fn environment_variable_keys_are_constrained() {
    let database = db().await;
    let project = insert_project(&database).await;
    let now = time::now();

    for bad_key in ["2FA", "MY-KEY", "MY KEY", "KEY;rm -rf /"] {
        let result = sqlx::query(
            "INSERT INTO environment_variables
               (id, project_id, key, value_plain, is_secret, created_at, updated_at)
             VALUES (?, ?, ?, 'x', 0, ?, ?)",
        )
        .bind(EnvVarId::generate().to_string())
        .bind(&project)
        .bind(bad_key)
        .bind(&now)
        .bind(&now)
        .execute(database.pool())
        .await;
        assert!(result.is_err(), "key {bad_key:?} should have been refused");
    }
}

// ------------------------------------------------------------ concurrency

#[tokio::test]
async fn a_project_can_hold_only_one_lock() {
    let database = db().await;
    let project = insert_project(&database).await;
    let now = time::now();

    sqlx::query(
        "INSERT INTO project_locks (project_id, operation, operation_id, acquired_at, expires_at)
         VALUES (?, 'RESTORE', ?, ?, ?)",
    )
    .bind(&project)
    .bind(OperationId::generate().to_string())
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await
    .expect("first lock should be granted");

    // The primary key is what refuses the second operation. This is the
    // mechanism behind "no two simultaneous restores".
    let result = sqlx::query(
        "INSERT INTO project_locks (project_id, operation, operation_id, acquired_at, expires_at)
         VALUES (?, 'REBUILD', ?, ?, ?)",
    )
    .bind(&project)
    .bind(OperationId::generate().to_string())
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await;

    assert!(matches!(
        DatabaseError::from(result.unwrap_err()),
        DatabaseError::UniqueViolation { .. }
    ));
}

#[tokio::test]
async fn two_projects_cannot_claim_the_same_host_port() {
    let database = db().await;
    let first = insert_project(&database).await;
    let second = insert_project(&database).await;

    let claim = |project: String| {
        let pool = database.pool().clone();
        async move {
            sqlx::query(
                "INSERT INTO project_ports (id, project_id, container_port, host_port)
                 VALUES (?, ?, 3000, 20001)",
            )
            .bind(PortId::generate().to_string())
            .bind(project)
            .execute(&pool)
            .await
        }
    };

    claim(first).await.expect("first claim should succeed");
    let result = claim(second).await;
    assert!(matches!(
        DatabaseError::from(result.unwrap_err()),
        DatabaseError::UniqueViolation { .. }
    ));
}

#[tokio::test]
async fn privileged_ports_cannot_be_stored() {
    let database = db().await;
    let project = insert_project(&database).await;

    for port in [80, 443, 1023] {
        let result = sqlx::query(
            "INSERT INTO project_ports (id, project_id, container_port, host_port)
             VALUES (?, ?, 3000, ?)",
        )
        .bind(PortId::generate().to_string())
        .bind(&project)
        .bind(port)
        .execute(database.pool())
        .await;
        assert!(result.is_err(), "port {port} should have been refused");
    }
}

// ------------------------------------------------- referential integrity

#[tokio::test]
async fn foreign_keys_are_enforced_on_every_connection() {
    // SQLite disables foreign keys by default and the setting is per
    // connection. This is the test that catches a pool configured to set it
    // only once.
    let database = db().await;
    let result = sqlx::query(
        "INSERT INTO environment_variables
           (id, project_id, key, value_plain, is_secret, created_at, updated_at)
         VALUES (?, 'prj_does_not_exist', 'KEY', 'v', 0, ?, ?)",
    )
    .bind(EnvVarId::generate().to_string())
    .bind(time::now())
    .bind(time::now())
    .execute(database.pool())
    .await;

    assert!(matches!(
        DatabaseError::from(result.unwrap_err()),
        DatabaseError::ForeignKeyViolation
    ));
}

#[tokio::test]
async fn deleting_a_project_cascades_to_its_children() {
    let database = db().await;
    let project = insert_project(&database).await;
    let now = time::now();

    sqlx::query(
        "INSERT INTO environment_variables
           (id, project_id, key, value_plain, is_secret, created_at, updated_at)
         VALUES (?, ?, 'PORT', '3000', 0, ?, ?)",
    )
    .bind(EnvVarId::generate().to_string())
    .bind(&project)
    .bind(&now)
    .bind(&now)
    .execute(database.pool())
    .await
    .unwrap();

    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(&project)
        .execute(database.pool())
        .await
        .unwrap();

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM environment_variables WHERE project_id = ?")
            .bind(&project)
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn audit_rows_survive_the_deletion_of_what_they_describe() {
    let database = db().await;
    let project = insert_project(&database).await;

    sqlx::query(
        "INSERT INTO audit_logs (id, occurred_at, action, target_type, target_id,
                                 target_label, result)
         VALUES (?, ?, 'project.delete', 'project', ?, 'Test Project', 'SUCCESS')",
    )
    .bind(AuditId::generate().to_string())
    .bind(time::now())
    .bind(&project)
    .execute(database.pool())
    .await
    .unwrap();

    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(&project)
        .execute(database.pool())
        .await
        .unwrap();

    // An audit log that vanishes with its subject is not an audit log.
    let label: String =
        sqlx::query_scalar("SELECT target_label FROM audit_logs WHERE target_id = ?")
            .bind(&project)
            .fetch_one(database.pool())
            .await
            .expect("the audit row must outlive the project");
    assert_eq!(label, "Test Project");
}

// --------------------------------------------------------------- invariants

#[tokio::test]
async fn resource_limits_outside_their_bounds_are_refused() {
    let database = db().await;
    let now = time::now();

    for (column, value) in [
        ("memory_limit_mb", "32"),
        ("memory_limit_mb", "999999"),
        ("cpu_limit_cores", "0"),
        ("process_limit", "2"),
    ] {
        let id = ProjectId::generate().to_string();
        let sql = format!(
            "INSERT INTO projects (id, slug, display_name, project_type, source_type,
                                   directory, created_at, updated_at, {column})
             VALUES (?, ?, 'x', 'NODE_APP', 'EMPTY', ?, ?, ?, {value})"
        );
        let result = sqlx::query(&sql)
            .bind(&id)
            // The whole UUID body, not a prefix: two UUIDv7 values minted in the same
            // millisecond share their leading bytes and would collide on the slug.
            .bind(format!("proj-{}", &id[4..]))
            .bind(format!("/projects/{id}"))
            .bind(&now)
            .bind(&now)
            .execute(database.pool())
            .await;
        assert!(
            result.is_err(),
            "{column} = {value} should have been refused"
        );
    }
}

#[tokio::test]
async fn agent_state_holds_at_most_one_row() {
    let database = db().await;
    let now = time::now();

    let insert = |id: i64| {
        let pool = database.pool().clone();
        let now = now.clone();
        async move {
            sqlx::query(
                "INSERT INTO agent_state (id, agent_version, schema_version, instance_id,
                                          started_at, last_heartbeat_at, bind_address)
                 VALUES (?, '0.1.0', 1, 'inst', ?, ?, '127.0.0.1:8787')",
            )
            .bind(id)
            .bind(&now)
            .bind(&now)
            .execute(&pool)
            .await
        }
    };

    insert(1).await.expect("the single row should insert");
    assert!(
        insert(2).await.is_err(),
        "a second agent_state row must be refused"
    );
}

#[tokio::test]
async fn timestamps_sort_in_chronological_order() {
    let database = db().await;
    let project = insert_project(&database).await;

    for seconds in [1_600_000_000i64, 1_700_000_000, 1_650_000_000] {
        sqlx::query(
            "INSERT INTO container_events (id, project_id, type, occurred_at)
             VALUES (?, ?, 'STARTED', ?)",
        )
        .bind(AuditId::generate().to_string())
        .bind(&project)
        .bind(time::format_unix_seconds(seconds))
        .execute(database.pool())
        .await
        .unwrap();
    }

    let rows = sqlx::query(
        "SELECT occurred_at FROM container_events WHERE project_id = ? ORDER BY occurred_at DESC",
    )
    .bind(&project)
    .fetch_all(database.pool())
    .await
    .unwrap();

    let ordered: Vec<String> = rows.iter().map(|row| row.get::<String, _>(0)).collect();
    assert_eq!(ordered[0], time::format_unix_seconds(1_700_000_000));
    assert_eq!(ordered[2], time::format_unix_seconds(1_600_000_000));
}
