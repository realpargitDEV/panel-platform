# Database Schema

SQLite via SQLx, one file, owned exclusively by the agent. No other process
opens it — the desktop client reaches data through the API, never the file.

```
Windows  C:\ProgramData\ProjectHost\data\project-host.db
Linux    /var/lib/project-host/project-host.db
```

---

## 1. Connection configuration

```sql
PRAGMA journal_mode = WAL;         -- readers never block the writer
PRAGMA synchronous  = NORMAL;      -- WAL-safe; FULL costs more than it buys here
PRAGMA foreign_keys = ON;          -- off by default in SQLite; must be set per connection
PRAGMA busy_timeout = 5000;
PRAGMA temp_store   = MEMORY;
PRAGMA cache_size   = -16000;      -- 16 MiB
PRAGMA wal_autocheckpoint = 1000;
```

`foreign_keys = ON` is set in SQLx's `after_connect` hook, not once at startup —
it is a per-connection setting, and a pool that sets it once silently loses
referential integrity on every other connection.

**Pool shape:** one writer connection and N readers. SQLite permits a single
writer; funnelling writes through one connection converts lock contention into a
queue and makes `SQLITE_BUSY` essentially unreachable. Metric writes are batched
per sampling tick rather than written per sample.

**Time:** all timestamps are `TEXT` in RFC 3339 UTC (`2026-07-29T00:11:04Z`).
Sortable as text, unambiguous, and readable when inspecting the file by hand.

**Identifiers:** every primary key is a prefixed UUIDv7 string — `prj_01J…`,
`bkp_01J…`. UUIDv7 sorts by creation time, which makes cursor pagination a plain
`WHERE id > ?`. The prefix makes a misrouted ID obvious in a log.

---

## 2. Migrations

Numbered, forward-only, applied in a transaction, embedded in the binary via
`sqlx::migrate!`.

```
crates/database/migrations/
  0001_initial.sql
  0002_discord.sql
  0003_remote_sources.sql
  0004_runtimes.sql
  0005_run_mode.sql
  0006_discord_bots.sql
  0007_discord_bot_projects.sql
  0008_local_runtime.sql
```

The application applies pending migrations at startup before serving, and refuses
to start if the database's schema version is _newer_ than the binary — a downgrade
after a failed update must fail loudly rather than corrupt data. A timestamped
copy of the database is taken before any migration that is not purely additive.

**Foreign keys are disabled for the duration of the run, and checked
afterwards.** SQLite cannot alter a `CHECK` constraint, so widening one means
rebuilding the table: copy, drop, rename. With enforcement on, the implicit
`DELETE FROM` inside `DROP TABLE projects` fires every `ON DELETE CASCADE`
aimed at it and takes the user's environment variables, ports and backups with
it. The pragma cannot live in the migration file — sqlx wraps each migration in a
transaction, `PRAGMA foreign_keys` is a no-op inside one, and the SQLite driver
ignores the `-- no-transaction` marker — so `Database::migrate` acquires one
connection, sets it there, and restores it afterwards even when a migration
failed. `PRAGMA foreign_key_check` then runs, and a migration that orphaned a row
fails startup rather than being discovered by a query months later.

Because 0003 rebuilds `projects`, the definition of that table in 0001 is no
longer the one in force. The enum-parity test therefore reads `sqlite_master`
rather than the migration text: a test trusting the first file would keep passing
while the database it describes had moved on.

---

## 3. Enumerations

Stored as `TEXT` with `CHECK` constraints. Readable in the file, validated by the
database, and mirrored by Rust enums with a parity test that fails when the two
drift.

| Enum                   | Values                                                                                                                                                  |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `UserRole`             | `ADMIN`                                                                                                                                                 |
| `ProjectType`          | `DISCORD_BOT`, `NODE_APP`, `PYTHON_APP`, `WEBSITE`, `STATIC_SITE`, `REST_API`, `WORKER`, `SERVICE`                                                      |
| `Runtime`              | `NODEJS`, `TYPESCRIPT`, `BUN`, `DENO`, `PYTHON`, `GO`, `RUST`, `JAVA`, `PHP`, `RUBY`, `DOTNET`, `STATIC`, `POLYGLOT`                                    |
| `PackageManager`       | `PNPM`, `NPM`, `YARN`, `BUN`, `DENO`, `PIP`, `POETRY`, `UV`, `PIPENV`, `GO_MODULES`, `CARGO`, `MAVEN`, `GRADLE`, `COMPOSER`, `BUNDLER`, `NUGET`, `NONE` |
| `ProjectStatus`        | `CREATING`, `STOPPED`, `STARTING`, `RUNNING`, `STOPPING`, `RESTARTING`, `BUILDING`, `CRASHED`, `FAILED`, `UNHEALTHY`, `ARCHIVED`, `DELETING`            |
| `RunMode`              | `DOCKER`, `HOST`                                                                                                                                        |
| `Priority`             | `LOW`, `NORMAL`, `HIGH`                                                                                                                                 |
| `DesiredState`         | `RUNNING`, `STOPPED`, `ARCHIVED`                                                                                                                        |
| `RestartPolicy`        | `NO`, `ON_FAILURE`, `UNLESS_STOPPED`, `ALWAYS`                                                                                                          |
| `NetworkMode`          | `NONE`, `INTERNAL`, `LAN`, `INTERNET`                                                                                                                   |
| `DeploymentType`       | `INITIAL`, `REBUILD`, `RESTORE`, `CONFIG_CHANGE`, `IMPORT`                                                                                              |
| `DeploymentStatus`     | `PENDING`, `BUILDING`, `STARTING`, `SUCCEEDED`, `FAILED`, `CANCELLED`, `INTERRUPTED`                                                                    |
| `ContainerEventType`   | `CREATED`, `STARTED`, `STOPPED`, `RESTARTED`, `DIED`, `OOM_KILLED`, `HEALTH_PASS`, `HEALTH_FAIL`, `DESTROYED`                                           |
| `BackupStatus`         | `PENDING`, `CREATING`, `COMPLETED`, `FAILED`, `CANCELLED`, `CORRUPT`                                                                                    |
| `BackupOperationKind`  | `CREATE`, `RESTORE`, `VERIFY`, `EXPORT`, `IMPORT`, `DELETE`                                                                                             |
| `BackupOperationState` | `PENDING`, `RUNNING`, `COMPLETED`, `FAILED`, `CANCELLED`, `INTERRUPTED`                                                                                 |
| `SourceType`           | `EMPTY`, `ZIP_UPLOAD`, `LOCAL_FOLDER`, `DUPLICATE`, `IMPORT_ARCHIVE`, `GIT_CLONE`, `REMOTE_ARCHIVE`                                                     |
| `AuditResult`          | `SUCCESS`, `FAILURE`, `DENIED`                                                                                                                          |
| `HealthState`          | `UNKNOWN`, `STARTING`, `HEALTHY`, `UNHEALTHY`, `NONE`                                                                                                   |
| `ConnectionKind`       | `LOCAL`, `LAN`, `TAILSCALE`, `MANUAL`                                                                                                                   |
| `NotificationLevel`    | `INFO`, `SUCCESS`, `WARNING`, `ERROR`                                                                                                                   |

`UserRole` has one value today. The column exists so that adding roles later is
a migration of one table rather than a redesign.

---

## 4. Identity and access

```sql
CREATE TABLE users (
    id             TEXT PRIMARY KEY,
    email          TEXT NOT NULL,
    email_lower    TEXT NOT NULL UNIQUE,      -- case-insensitive uniqueness
    display_name   TEXT NOT NULL,
    password_hash  TEXT NOT NULL,             -- Argon2id PHC string
    role           TEXT NOT NULL DEFAULT 'ADMIN' CHECK (role IN ('ADMIN')),
    failed_logins  INTEGER NOT NULL DEFAULT 0,
    locked_until   TEXT,
    last_login_at  TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE recovery_codes (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash   TEXT NOT NULL,                -- Argon2id; never stored plainly
    used_at     TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_recovery_user ON recovery_codes(user_id) WHERE used_at IS NULL;

CREATE TABLE sessions (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash       TEXT NOT NULL UNIQUE,    -- SHA-256 of the opaque token
    trusted_client_id TEXT REFERENCES trusted_clients(id) ON DELETE CASCADE,
    client_label     TEXT NOT NULL,
    source_addr      TEXT,
    created_at       TEXT NOT NULL,
    last_seen_at     TEXT NOT NULL,
    idle_expires_at  TEXT NOT NULL,
    absolute_expires_at TEXT NOT NULL,
    revoked_at       TEXT
);
CREATE INDEX idx_sessions_user   ON sessions(user_id);
CREATE INDEX idx_sessions_expiry ON sessions(absolute_expires_at) WHERE revoked_at IS NULL;

CREATE TABLE trusted_clients (
    id             TEXT PRIMARY KEY,
    label          TEXT NOT NULL,
    public_key     BLOB NOT NULL UNIQUE,      -- Ed25519, 32 bytes
    fingerprint    TEXT NOT NULL UNIQUE,      -- SHA-256, shown to the user
    platform       TEXT,
    paired_at      TEXT NOT NULL,
    last_seen_at   TEXT,
    revoked_at     TEXT
);

CREATE TABLE pairing_codes (
    id          TEXT PRIMARY KEY,
    code_hash   TEXT NOT NULL UNIQUE,
    expires_at  TEXT NOT NULL,
    consumed_at TEXT,
    created_at  TEXT NOT NULL
);
```

Only hashes are stored for session tokens, recovery codes and pairing codes.
Reading this database yields no credential that can be replayed.

`server_connections` is the mirror image — the _client's_ record of agents it
knows, stored in the desktop client's own small database, not the agent's:

```sql
CREATE TABLE server_connections (
    id                  TEXT PRIMARY KEY,
    label               TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK (kind IN ('LOCAL','LAN','TAILSCALE','MANUAL')),
    address             TEXT NOT NULL,
    port                INTEGER NOT NULL DEFAULT 8787,
    cert_fingerprint    TEXT NOT NULL,        -- pinned
    device_key_ref      TEXT,                 -- keychain handle, never the key
    auto_connect        INTEGER NOT NULL DEFAULT 0,
    last_connected_at   TEXT,
    created_at          TEXT NOT NULL,
    UNIQUE (address, port)
);
```

---

## 5. Projects

```sql
CREATE TABLE projects (
    id              TEXT PRIMARY KEY,          -- prj_<uuidv7>
    slug            TEXT NOT NULL UNIQUE,      -- generated; NOT from the display name
    display_name    TEXT NOT NULL,             -- arbitrary user text; display only
    description     TEXT NOT NULL DEFAULT '',
    project_type    TEXT NOT NULL,
    icon            TEXT,
    color           TEXT,

    status          TEXT NOT NULL DEFAULT 'CREATING',
    desired_state   TEXT NOT NULL DEFAULT 'STOPPED',
    health          TEXT NOT NULL DEFAULT 'UNKNOWN',

    container_id    TEXT,                      -- Docker's id, null when absent
    container_name  TEXT UNIQUE,               -- ph_<slug>; generated
    image_tag       TEXT,
    network_name    TEXT UNIQUE,
    volume_name     TEXT UNIQUE,

    source_type     TEXT NOT NULL,
    directory       TEXT NOT NULL UNIQUE,      -- absolute, canonical, UUID-derived

    source_url      TEXT,                      -- GIT_CLONE and REMOTE_ARCHIVE only
    source_ref      TEXT,                      -- GIT_CLONE only: branch or tag
    source_commit   TEXT,                      -- the commit actually checked out

    autostart       INTEGER NOT NULL DEFAULT 0,
    restart_policy  TEXT NOT NULL DEFAULT 'UNLESS_STOPPED',
    network_mode    TEXT NOT NULL DEFAULT 'INTERNAL',

    memory_limit_mb   INTEGER NOT NULL DEFAULT 512,
    cpu_limit_cores   REAL    NOT NULL DEFAULT 1.0,
    storage_limit_mb  INTEGER NOT NULL DEFAULT 2048,
    process_limit     INTEGER NOT NULL DEFAULT 128,

    started_at      TEXT,
    stopped_at      TEXT,
    last_exit_code  INTEGER,
    last_failure_at TEXT,
    last_failure_reason TEXT,
    restart_count   INTEGER NOT NULL DEFAULT 0,

    archived_at     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,

    CHECK (memory_limit_mb  BETWEEN 64 AND 65536),
    CHECK (cpu_limit_cores  > 0 AND cpu_limit_cores <= 64),
    CHECK (process_limit    BETWEEN 8 AND 4096),
    CHECK (slug GLOB '[a-z0-9][a-z0-9-]*'),
    -- A remote source without a URL is not a remote source...
    CHECK (source_type NOT IN ('GIT_CLONE','REMOTE_ARCHIVE') OR source_url IS NOT NULL),
    -- ...and no local source may carry one, which makes `source_url IS NOT NULL`
    -- a reliable question to ask.
    CHECK (source_type IN ('GIT_CLONE','REMOTE_ARCHIVE') OR source_url IS NULL),
    CHECK (source_type = 'GIT_CLONE' OR (source_ref IS NULL AND source_commit IS NULL)),
    -- A URL carrying a token in its userinfo would put a secret in this column,
    -- in every backup of this file, and in any log line that echoes it.
    CHECK (source_url IS NULL OR source_url NOT LIKE '%@%')
);
CREATE INDEX idx_projects_status  ON projects(status);
CREATE INDEX idx_projects_desired ON projects(desired_state) WHERE archived_at IS NULL;
```

Three columns carry the central safety property. `display_name` is whatever the
user typed and is used only for display. `slug`, `directory` and `container_name`
are all derived from the generated `id`. There is no code path from user text to
a path or a Docker identifier — the `CHECK` on `slug` is a backstop for a bug,
not the primary defence.

```sql
CREATE TABLE project_runtimes (
    project_id        TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    runtime           TEXT NOT NULL,
    runtime_version   TEXT NOT NULL,
    package_manager   TEXT NOT NULL DEFAULT 'NONE',
    install_command   TEXT,
    build_command     TEXT,
    start_command     TEXT NOT NULL,
    working_dir       TEXT NOT NULL DEFAULT '/app',
    entry_file        TEXT,
    publish_dir       TEXT,                    -- static sites
    template_id       TEXT NOT NULL,           -- approved template, allow-listed
    health_check_type TEXT NOT NULL DEFAULT 'NONE',
    health_check_target TEXT,                  -- path or command
    health_interval_s INTEGER NOT NULL DEFAULT 30,
    health_timeout_s  INTEGER NOT NULL DEFAULT 5,
    health_retries    INTEGER NOT NULL DEFAULT 3,
    health_start_period_s INTEGER NOT NULL DEFAULT 20
);

CREATE TABLE project_ports (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    container_port INTEGER NOT NULL CHECK (container_port BETWEEN 1 AND 65535),
    host_port      INTEGER CHECK (host_port BETWEEN 1024 AND 65535),
    protocol       TEXT NOT NULL DEFAULT 'tcp' CHECK (protocol IN ('tcp','udp')),
    bind_address   TEXT NOT NULL DEFAULT '127.0.0.1',
    is_primary     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (host_port, protocol, bind_address)
);
```

`host_port` starts at 1024, so a privileged port cannot be requested at all. The
`UNIQUE` constraint makes double-allocation a database error rather than a race:
two concurrent creations cannot both win.

---

## 6. Deployments, events, environment

```sql
CREATE TABLE deployments (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type          TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'PENDING',
    operation_id  TEXT,
    image_tag     TEXT,
    build_log_path TEXT,                       -- on disk; not in the database
    error_code    TEXT,
    error_message TEXT,
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    duration_ms   INTEGER
);
CREATE INDEX idx_deploy_project ON deployments(project_id, started_at DESC);

CREATE TABLE container_events (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type        TEXT NOT NULL,
    exit_code   INTEGER,
    detail      TEXT,
    occurred_at TEXT NOT NULL
);
CREATE INDEX idx_events_project ON container_events(project_id, occurred_at DESC);

CREATE TABLE environment_variables (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key           TEXT NOT NULL,
    value_plain   TEXT,                        -- non-secrets only
    value_cipher  BLOB,                        -- XChaCha20-Poly1305
    value_nonce   BLOB,
    is_secret     INTEGER NOT NULL DEFAULT 0,
    restart_required INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    UNIQUE (project_id, key),
    CHECK (key GLOB '[A-Za-z_][A-Za-z0-9_]*'),
    CHECK ((is_secret = 0 AND value_plain IS NOT NULL AND value_cipher IS NULL)
        OR (is_secret = 1 AND value_cipher IS NOT NULL AND value_plain IS NULL))
);
```

The final `CHECK` is worth more than it looks: it makes "a secret stored in
plaintext" a constraint violation the database refuses, not a bug waiting to be
noticed in review. The `key` pattern blocks the injection of names that would
confuse a shell or an env file.

### Source credentials

An access token for a project's private remote, if one was supplied:

```sql
CREATE TABLE project_source_credentials (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    ciphertext BLOB NOT NULL,                  -- XChaCha20-Poly1305
    nonce      BLOB NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (length(ciphertext) > 0),
    CHECK (length(nonce) = 24)
);
```

The same shape, and for the same reason, as `discord_bot`: a ciphertext column, a
nonce column, and nothing a plaintext token could occupy. A writer that wanted to
store one in the clear would have to alter the table first.

Two absences are deliberate. The repository over this table never receives an
encryption key — a token arrives as ciphertext and leaves as ciphertext, so the
one piece of code that can turn a stored blob back into a usable credential lives
outside the layer that talks to SQLite. And there is no query that lists every
credential; nothing needs one, and its only use would be building the report a
compromise wants.

The ciphertext is bound to its project by the associated data, so a row copied to
another project does not decrypt. The API answers `has_credential: bool` and has
no route that returns the token.

---

## 7. Backups

```sql
CREATE TABLE project_backups (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status         TEXT NOT NULL DEFAULT 'PENDING',
    file_path      TEXT UNIQUE,
    size_bytes     INTEGER,
    checksum_sha256 TEXT,
    includes_files   INTEGER NOT NULL DEFAULT 1,
    includes_volumes INTEGER NOT NULL DEFAULT 1,
    includes_config  INTEGER NOT NULL DEFAULT 1,
    env_metadata   TEXT,                       -- keys and is_secret only; no values
    note           TEXT,
    verified_at    TEXT,
    created_at     TEXT NOT NULL,
    completed_at   TEXT
);
CREATE INDEX idx_backups_project ON project_backups(project_id, created_at DESC);

CREATE TABLE backup_operations (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    backup_id     TEXT REFERENCES project_backups(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL,
    state         TEXT NOT NULL DEFAULT 'PENDING',
    progress_pct  INTEGER NOT NULL DEFAULT 0,
    bytes_done    INTEGER NOT NULL DEFAULT 0,
    bytes_total   INTEGER,
    error_code    TEXT,
    error_message TEXT,
    temp_path     TEXT,                        -- cleaned on interrupted recovery
    started_at    TEXT NOT NULL,
    finished_at   TEXT
);
CREATE INDEX idx_backup_ops_active ON backup_operations(project_id)
    WHERE state IN ('PENDING','RUNNING');

CREATE TABLE project_locks (
    project_id  TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    operation   TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    expires_at  TEXT NOT NULL
);
```

`project_locks` having `project_id` as its **primary key** is the whole
mechanism: a second concurrent operation cannot insert a row. "No two
simultaneous restores" and "no deletion during restore" are enforced by a
uniqueness constraint rather than by application-level checking. `expires_at`
prevents a crashed agent from leaving a project locked forever — startup
recovery clears expired locks and marks the abandoned operation `INTERRUPTED`.

---

## 8. Observability

```sql
CREATE TABLE audit_logs (
    id           TEXT PRIMARY KEY,
    occurred_at  TEXT NOT NULL,
    user_id      TEXT REFERENCES users(id) ON DELETE SET NULL,
    client_id    TEXT REFERENCES trusted_clients(id) ON DELETE SET NULL,
    client_label TEXT,
    source_addr  TEXT,
    action       TEXT NOT NULL,
    target_type  TEXT,
    target_id    TEXT,
    target_label TEXT,
    result       TEXT NOT NULL,
    error_code   TEXT,
    request_id   TEXT,
    operation_id TEXT,
    metadata     TEXT                          -- sanitised JSON; never secret values
);
CREATE INDEX idx_audit_time   ON audit_logs(occurred_at DESC);
CREATE INDEX idx_audit_action ON audit_logs(action, occurred_at DESC);
CREATE INDEX idx_audit_target ON audit_logs(target_type, target_id, occurred_at DESC);
```

Audit rows survive the deletion of what they describe: the foreign keys are
`ON DELETE SET NULL`, and `target_label` keeps a human-readable copy. A record
of "who deleted project X" that vanishes with project X is not an audit log.

```sql
CREATE TABLE project_metrics (
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    bucket_at     TEXT NOT NULL,               -- 10s buckets
    cpu_pct       REAL,
    memory_bytes  INTEGER,
    memory_limit_bytes INTEGER,
    net_rx_bytes  INTEGER,
    net_tx_bytes  INTEGER,
    disk_read_bytes  INTEGER,
    disk_write_bytes INTEGER,
    PRIMARY KEY (project_id, bucket_at)
) WITHOUT ROWID;

CREATE TABLE system_metrics (
    bucket_at     TEXT PRIMARY KEY,
    cpu_pct       REAL,
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
```

`WITHOUT ROWID` with a composite key stores metrics clustered by project and
time — the exact order they are queried in. Live charts read the ring buffer in
memory; these tables serve history and are downsampled by retention (10s for 6
hours, 1min for 7 days, 5min for 90 days).

```sql
CREATE TABLE notifications (
    id          TEXT PRIMARY KEY,
    level       TEXT NOT NULL,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,
    action_route TEXT,
    read_at     TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_notif_unread ON notifications(created_at DESC) WHERE read_at IS NULL;

CREATE TABLE system_settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,                 -- JSON
    updated_at  TEXT NOT NULL
);

CREATE TABLE application_settings (
    key         TEXT PRIMARY KEY,              -- desktop client's own database
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE agent_state (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),   -- single row
    agent_version       TEXT NOT NULL,
    schema_version      INTEGER NOT NULL,
    instance_id         TEXT NOT NULL,
    started_at          TEXT NOT NULL,
    last_heartbeat_at   TEXT NOT NULL,
    last_clean_shutdown INTEGER NOT NULL DEFAULT 0,
    docker_available    INTEGER NOT NULL DEFAULT 0,
    docker_version      TEXT,
    bind_address        TEXT NOT NULL
);
```

`last_clean_shutdown` is set to 0 on start and 1 only on graceful stop. Finding
0 at startup is what triggers full reconciliation and interrupted-operation
recovery. `CHECK (id = 1)` makes the single-row invariant structural.

---

## 9. Transactions

Multi-step writes are wrapped, always:

| Operation              | Atomic unit                                                                                 |
| ---------------------- | ------------------------------------------------------------------------------------------- |
| Create project         | `projects` + `project_runtimes` + `project_ports` + `environment_variables` + `deployments` |
| Delete project         | lock, cascade deletes, port release, audit row                                              |
| Restore backup         | lock acquire, operation row, config replacement, operation completion                       |
| Rotate encryption key  | re-encrypt every secret, or roll back entirely                                              |
| Import project archive | project rows + file move, with the move last                                                |

Docker calls sit **outside** transactions — a database transaction cannot roll
back a created container. The pattern is: write intent, commit, act, then record
the outcome. A crash between commit and act leaves a row whose reality the
reconciler repairs at next start. The inverse order would leave containers with
no database record, which is unrecoverable without guesswork.

---

## 10. Retention

Enforced by a nightly task and bounded at write time:

| Data                                 | Kept                                              |
| ------------------------------------ | ------------------------------------------------- |
| `container_events`                   | 90 days or 1000 rows per project                  |
| `deployments`                        | 50 per project                                    |
| `audit_logs`                         | 365 days (configurable; never silently truncated) |
| `system_metrics` / `project_metrics` | 10s→6h, 1min→7d, 5min→90d                         |
| `notifications`                      | 30 days once read                                 |
| `sessions`                           | deleted 7 days after expiry                       |
| `project_backups`                    | per-project retention limit, default 10           |

Audit log trimming writes an audit entry recording what was trimmed.
