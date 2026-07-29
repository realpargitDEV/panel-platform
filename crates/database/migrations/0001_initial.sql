-- Initial schema. See docs/database-schema.md for the reasoning behind each
-- constraint; this file is the authority for what is actually enforced.
--
-- Conventions:
--   * Primary keys are prefixed UUIDv7 strings (prj_…, bkp_…).
--   * Timestamps are TEXT, RFC 3339, UTC. Sortable as text.
--   * Enums are TEXT with CHECK constraints, mirrored by Rust enums whose
--     parity with these lists is asserted by a test.

-- ---------------------------------------------------------------- identity

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL,
    email_lower   TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'ADMIN' CHECK (role IN ('ADMIN')),
    failed_logins INTEGER NOT NULL DEFAULT 0,
    locked_until  TEXT,
    last_login_at TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE trusted_clients (
    id           TEXT PRIMARY KEY,
    label        TEXT NOT NULL,
    public_key   BLOB NOT NULL UNIQUE,
    fingerprint  TEXT NOT NULL UNIQUE,
    platform     TEXT,
    paired_at    TEXT NOT NULL,
    last_seen_at TEXT,
    revoked_at   TEXT
);

CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,
    user_id             TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash          TEXT NOT NULL UNIQUE,
    trusted_client_id   TEXT REFERENCES trusted_clients(id) ON DELETE CASCADE,
    client_label        TEXT NOT NULL,
    source_addr         TEXT,
    created_at          TEXT NOT NULL,
    last_seen_at        TEXT NOT NULL,
    idle_expires_at     TEXT NOT NULL,
    absolute_expires_at TEXT NOT NULL,
    revoked_at          TEXT
);
CREATE INDEX idx_sessions_user ON sessions (user_id);
CREATE INDEX idx_sessions_expiry ON sessions (absolute_expires_at) WHERE revoked_at IS NULL;

CREATE TABLE recovery_codes (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash  TEXT NOT NULL,
    used_at    TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_recovery_user ON recovery_codes (user_id) WHERE used_at IS NULL;

CREATE TABLE pairing_codes (
    id          TEXT PRIMARY KEY,
    code_hash   TEXT NOT NULL UNIQUE,
    expires_at  TEXT NOT NULL,
    consumed_at TEXT,
    created_at  TEXT NOT NULL
);

-- ---------------------------------------------------------------- projects

CREATE TABLE projects (
    id             TEXT PRIMARY KEY,
    slug           TEXT NOT NULL UNIQUE,
    display_name   TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    project_type   TEXT NOT NULL CHECK (project_type IN (
        'DISCORD_BOT','NODE_APP','PYTHON_APP','WEBSITE','STATIC_SITE',
        'REST_API','WORKER','SERVICE')),
    icon           TEXT,
    color          TEXT,

    status         TEXT NOT NULL DEFAULT 'CREATING' CHECK (status IN (
        'CREATING','STOPPED','STARTING','RUNNING','STOPPING','RESTARTING',
        'BUILDING','FAILED','UNHEALTHY','ARCHIVED','DELETING')),
    desired_state  TEXT NOT NULL DEFAULT 'STOPPED' CHECK (desired_state IN (
        'RUNNING','STOPPED','ARCHIVED')),
    health         TEXT NOT NULL DEFAULT 'UNKNOWN' CHECK (health IN (
        'UNKNOWN','STARTING','HEALTHY','UNHEALTHY','NONE')),

    container_id   TEXT,
    container_name TEXT UNIQUE,
    image_tag      TEXT,
    network_name   TEXT UNIQUE,
    volume_name    TEXT UNIQUE,

    source_type    TEXT NOT NULL CHECK (source_type IN (
        'EMPTY','ZIP_UPLOAD','LOCAL_FOLDER','DUPLICATE','IMPORT_ARCHIVE')),
    directory      TEXT NOT NULL UNIQUE,

    autostart      INTEGER NOT NULL DEFAULT 0 CHECK (autostart IN (0, 1)),
    restart_policy TEXT NOT NULL DEFAULT 'UNLESS_STOPPED' CHECK (restart_policy IN (
        'NO','ON_FAILURE','UNLESS_STOPPED','ALWAYS')),
    network_mode   TEXT NOT NULL DEFAULT 'INTERNAL' CHECK (network_mode IN (
        'NONE','INTERNAL','LAN','INTERNET')),

    memory_limit_mb  INTEGER NOT NULL DEFAULT 512,
    cpu_limit_cores  REAL    NOT NULL DEFAULT 1.0,
    storage_limit_mb INTEGER NOT NULL DEFAULT 2048,
    process_limit    INTEGER NOT NULL DEFAULT 128,

    started_at          TEXT,
    stopped_at          TEXT,
    last_exit_code      INTEGER,
    last_failure_at     TEXT,
    last_failure_reason TEXT,
    restart_count       INTEGER NOT NULL DEFAULT 0,

    archived_at TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,

    CHECK (memory_limit_mb BETWEEN 64 AND 65536),
    CHECK (cpu_limit_cores > 0 AND cpu_limit_cores <= 64),
    CHECK (storage_limit_mb BETWEEN 128 AND 1048576),
    CHECK (process_limit BETWEEN 8 AND 4096),
    -- The slug is generated, never derived from display_name. This is a
    -- backstop against a bug, not the primary defence.
    CHECK (slug GLOB '[a-z0-9][a-z0-9-]*')
);
CREATE INDEX idx_projects_status ON projects (status);
CREATE INDEX idx_projects_desired ON projects (desired_state) WHERE archived_at IS NULL;

CREATE TABLE project_runtimes (
    project_id            TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    runtime               TEXT NOT NULL CHECK (runtime IN ('NODEJS','PYTHON','STATIC')),
    runtime_version       TEXT NOT NULL,
    package_manager       TEXT NOT NULL DEFAULT 'NONE' CHECK (package_manager IN (
        'PNPM','NPM','YARN','PIP','POETRY','UV','PIPENV','NONE')),
    install_command       TEXT,
    build_command         TEXT,
    start_command         TEXT NOT NULL,
    working_dir           TEXT NOT NULL DEFAULT '/app',
    entry_file            TEXT,
    publish_dir           TEXT,
    template_id           TEXT NOT NULL,
    health_check_type     TEXT NOT NULL DEFAULT 'NONE' CHECK (health_check_type IN (
        'NONE','HTTP','TCP','COMMAND')),
    health_check_target   TEXT,
    health_interval_s     INTEGER NOT NULL DEFAULT 30,
    health_timeout_s      INTEGER NOT NULL DEFAULT 5,
    health_retries        INTEGER NOT NULL DEFAULT 3,
    health_start_period_s INTEGER NOT NULL DEFAULT 20
);

CREATE TABLE project_ports (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    container_port INTEGER NOT NULL CHECK (container_port BETWEEN 1 AND 65535),
    -- Starts at 1024: a privileged port cannot be requested at all.
    host_port      INTEGER CHECK (host_port BETWEEN 1024 AND 65535),
    protocol       TEXT NOT NULL DEFAULT 'tcp' CHECK (protocol IN ('tcp','udp')),
    bind_address   TEXT NOT NULL DEFAULT '127.0.0.1',
    is_primary     INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    -- Makes double allocation a database error rather than a race.
    UNIQUE (host_port, protocol, bind_address)
);
CREATE INDEX idx_ports_project ON project_ports (project_id);

-- ---------------------------------------------------- deployments & events

CREATE TABLE deployments (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type           TEXT NOT NULL CHECK (type IN (
        'INITIAL','REBUILD','RESTORE','CONFIG_CHANGE','IMPORT')),
    status         TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN (
        'PENDING','BUILDING','STARTING','SUCCEEDED','FAILED','CANCELLED','INTERRUPTED')),
    operation_id   TEXT,
    image_tag      TEXT,
    build_log_path TEXT,
    error_code     TEXT,
    error_message  TEXT,
    started_at     TEXT NOT NULL,
    finished_at    TEXT,
    duration_ms    INTEGER
);
CREATE INDEX idx_deploy_project ON deployments (project_id, started_at DESC);

CREATE TABLE container_events (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type        TEXT NOT NULL CHECK (type IN (
        'CREATED','STARTED','STOPPED','RESTARTED','DIED','OOM_KILLED',
        'HEALTH_PASS','HEALTH_FAIL','DESTROYED')),
    exit_code   INTEGER,
    detail      TEXT,
    occurred_at TEXT NOT NULL
);
CREATE INDEX idx_events_project ON container_events (project_id, occurred_at DESC);

-- ------------------------------------------------------------ environment

CREATE TABLE environment_variables (
    id               TEXT PRIMARY KEY,
    project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key              TEXT NOT NULL,
    value_plain      TEXT,
    value_cipher     BLOB,
    value_nonce      BLOB,
    is_secret        INTEGER NOT NULL DEFAULT 0 CHECK (is_secret IN (0, 1)),
    restart_required INTEGER NOT NULL DEFAULT 1 CHECK (restart_required IN (0, 1)),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    UNIQUE (project_id, key),
    CHECK (key GLOB '[A-Za-z_]*' AND key NOT GLOB '*[^A-Za-z0-9_]*'),
    -- A secret stored in plaintext is a constraint violation the database
    -- refuses, not a bug waiting to be noticed in review.
    CHECK (
        (is_secret = 0 AND value_plain IS NOT NULL AND value_cipher IS NULL)
        OR
        (is_secret = 1 AND value_cipher IS NOT NULL AND value_nonce IS NOT NULL
         AND value_plain IS NULL)
    )
);

-- ---------------------------------------------------------------- backups

CREATE TABLE project_backups (
    id               TEXT PRIMARY KEY,
    project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status           TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN (
        'PENDING','CREATING','COMPLETED','FAILED','CANCELLED','CORRUPT')),
    file_path        TEXT UNIQUE,
    size_bytes       INTEGER,
    checksum_sha256  TEXT,
    includes_files   INTEGER NOT NULL DEFAULT 1 CHECK (includes_files IN (0, 1)),
    includes_volumes INTEGER NOT NULL DEFAULT 1 CHECK (includes_volumes IN (0, 1)),
    includes_config  INTEGER NOT NULL DEFAULT 1 CHECK (includes_config IN (0, 1)),
    -- Key names and secret flags only. Never values: a backup archive must not
    -- become a secret store.
    env_metadata     TEXT,
    note             TEXT,
    verified_at      TEXT,
    created_at       TEXT NOT NULL,
    completed_at     TEXT
);
CREATE INDEX idx_backups_project ON project_backups (project_id, created_at DESC);

CREATE TABLE backup_operations (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    backup_id     TEXT REFERENCES project_backups(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL CHECK (kind IN (
        'CREATE','RESTORE','VERIFY','EXPORT','IMPORT','DELETE')),
    state         TEXT NOT NULL DEFAULT 'PENDING' CHECK (state IN (
        'PENDING','RUNNING','COMPLETED','FAILED','CANCELLED','INTERRUPTED')),
    progress_pct  INTEGER NOT NULL DEFAULT 0 CHECK (progress_pct BETWEEN 0 AND 100),
    bytes_done    INTEGER NOT NULL DEFAULT 0,
    bytes_total   INTEGER,
    error_code    TEXT,
    error_message TEXT,
    temp_path     TEXT,
    started_at    TEXT NOT NULL,
    finished_at   TEXT
);
CREATE INDEX idx_backup_ops_active ON backup_operations (project_id)
    WHERE state IN ('PENDING', 'RUNNING');

-- project_id as PRIMARY KEY is the whole mechanism: a second concurrent
-- operation cannot insert a row. "No two simultaneous restores" is enforced by
-- a uniqueness constraint rather than by application-level checking.
CREATE TABLE project_locks (
    project_id   TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    operation    TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    acquired_at  TEXT NOT NULL,
    expires_at   TEXT NOT NULL
);

-- ----------------------------------------------------------- observability

CREATE TABLE audit_logs (
    id           TEXT PRIMARY KEY,
    occurred_at  TEXT NOT NULL,
    -- SET NULL, not CASCADE: a record of "who deleted project X" that vanishes
    -- with project X is not an audit log.
    user_id      TEXT REFERENCES users(id) ON DELETE SET NULL,
    client_id    TEXT REFERENCES trusted_clients(id) ON DELETE SET NULL,
    client_label TEXT,
    source_addr  TEXT,
    action       TEXT NOT NULL,
    target_type  TEXT,
    target_id    TEXT,
    target_label TEXT,
    result       TEXT NOT NULL CHECK (result IN ('SUCCESS','FAILURE','DENIED')),
    error_code   TEXT,
    request_id   TEXT,
    operation_id TEXT,
    metadata     TEXT
);
CREATE INDEX idx_audit_time ON audit_logs (occurred_at DESC);
CREATE INDEX idx_audit_action ON audit_logs (action, occurred_at DESC);
CREATE INDEX idx_audit_target ON audit_logs (target_type, target_id, occurred_at DESC);

CREATE TABLE project_metrics (
    project_id         TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    bucket_at          TEXT NOT NULL,
    cpu_pct            REAL,
    memory_bytes       INTEGER,
    memory_limit_bytes INTEGER,
    net_rx_bytes       INTEGER,
    net_tx_bytes       INTEGER,
    disk_read_bytes    INTEGER,
    disk_write_bytes   INTEGER,
    PRIMARY KEY (project_id, bucket_at)
) WITHOUT ROWID;

CREATE TABLE system_metrics (
    bucket_at          TEXT PRIMARY KEY,
    cpu_pct            REAL,
    memory_used_bytes  INTEGER,
    memory_total_bytes INTEGER,
    swap_used_bytes    INTEGER,
    disk_used_bytes    INTEGER,
    disk_total_bytes   INTEGER,
    disk_read_bytes    INTEGER,
    disk_write_bytes   INTEGER,
    net_rx_bytes       INTEGER,
    net_tx_bytes       INTEGER,
    cpu_temp_c         REAL,
    process_count      INTEGER,
    uptime_s           INTEGER
) WITHOUT ROWID;

CREATE TABLE notifications (
    id           TEXT PRIMARY KEY,
    level        TEXT NOT NULL CHECK (level IN ('INFO','SUCCESS','WARNING','ERROR')),
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    project_id   TEXT REFERENCES projects(id) ON DELETE CASCADE,
    action_route TEXT,
    read_at      TEXT,
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_notif_unread ON notifications (created_at DESC) WHERE read_at IS NULL;

CREATE TABLE system_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Single row. The CHECK makes that invariant structural.
CREATE TABLE agent_state (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    agent_version       TEXT NOT NULL,
    schema_version      INTEGER NOT NULL,
    instance_id         TEXT NOT NULL,
    started_at          TEXT NOT NULL,
    last_heartbeat_at   TEXT NOT NULL,
    -- 0 on start, 1 only on graceful stop. Finding 0 at startup is what
    -- triggers full reconciliation and interrupted-operation recovery.
    last_clean_shutdown INTEGER NOT NULL DEFAULT 0 CHECK (last_clean_shutdown IN (0, 1)),
    docker_available    INTEGER NOT NULL DEFAULT 0 CHECK (docker_available IN (0, 1)),
    docker_version      TEXT,
    bind_address        TEXT NOT NULL
);
