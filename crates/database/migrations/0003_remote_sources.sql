-- Remote project sources: a project may come from a git remote or an HTTPS
-- archive, not only from a local folder, an upload, another project, or nothing.
--
-- Three things happen here:
--
-- 1. `projects.source_type` gains 'GIT_CLONE' and 'REMOTE_ARCHIVE'. SQLite
--    cannot alter a CHECK constraint, so the table is rebuilt: copy, drop,
--    rename. `Database::migrate` disables foreign keys around the whole
--    migration run for exactly this reason — with enforcement on, the implicit
--    DELETE FROM inside `DROP TABLE projects` fires every ON DELETE CASCADE
--    aimed at it and takes the user's environment variables, ports and backups
--    with it. The pragma cannot be set here: sqlx wraps this file in a
--    transaction, and the pragma is a no-op inside one.
--
-- 2. `projects` gains provenance columns. `source_commit` is the commit that
--    was actually checked out, which is the only honest answer to "what is
--    running" when the requested ref was a moving branch.
--
-- 3. A token for a private remote gets a table with a ciphertext column and a
--    nonce column and nothing else — the same shape, and for the same reason,
--    as `discord_bot`. A writer wanting to store a token in the clear would
--    have to alter the table first.

CREATE TABLE projects_rebuilt (
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
        'EMPTY','ZIP_UPLOAD','LOCAL_FOLDER','DUPLICATE','IMPORT_ARCHIVE',
        'GIT_CLONE','REMOTE_ARCHIVE')),
    directory      TEXT NOT NULL UNIQUE,

    -- Provenance. Null for every source that has no remote.
    source_url     TEXT,
    source_ref     TEXT,
    source_commit  TEXT,

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
    CHECK (slug GLOB '[a-z0-9][a-z0-9-]*'),
    -- A remote source without a URL is not a remote source. Enforced here so a
    -- half-written row cannot claim a provenance it does not have.
    CHECK (source_type NOT IN ('GIT_CLONE','REMOTE_ARCHIVE') OR source_url IS NOT NULL),
    -- ...and the converse: no local source may carry a URL, so that
    -- `source_url IS NOT NULL` is a reliable question to ask.
    CHECK (source_type IN ('GIT_CLONE','REMOTE_ARCHIVE') OR source_url IS NULL),
    -- Only a git clone has a ref or a commit.
    CHECK (source_type = 'GIT_CLONE' OR (source_ref IS NULL AND source_commit IS NULL)),
    -- A URL that carries a token in its userinfo would put a secret in this
    -- column, in every backup of this file, and in any log line that echoes it.
    CHECK (source_url IS NULL OR source_url NOT LIKE '%@%')
);

-- Column list written out rather than `SELECT *`: the new columns have no
-- counterpart in the old table, and a positional copy would silently shift
-- every value after `directory` by three columns.
INSERT INTO projects_rebuilt (
    id, slug, display_name, description, project_type, icon, color,
    status, desired_state, health,
    container_id, container_name, image_tag, network_name, volume_name,
    source_type, directory,
    autostart, restart_policy, network_mode,
    memory_limit_mb, cpu_limit_cores, storage_limit_mb, process_limit,
    started_at, stopped_at, last_exit_code, last_failure_at,
    last_failure_reason, restart_count,
    archived_at, created_at, updated_at
)
SELECT
    id, slug, display_name, description, project_type, icon, color,
    status, desired_state, health,
    container_id, container_name, image_tag, network_name, volume_name,
    source_type, directory,
    autostart, restart_policy, network_mode,
    memory_limit_mb, cpu_limit_cores, storage_limit_mb, process_limit,
    started_at, stopped_at, last_exit_code, last_failure_at,
    last_failure_reason, restart_count,
    archived_at, created_at, updated_at
FROM projects;

DROP TABLE projects;

-- No child table references `projects_rebuilt`, so this rename rewrites nothing:
-- the children still name `projects`, which this table is about to become.
ALTER TABLE projects_rebuilt RENAME TO projects;

CREATE INDEX idx_projects_status ON projects (status);
CREATE INDEX idx_projects_desired ON projects (desired_state) WHERE archived_at IS NULL;

-- The access token for a private remote. One row per project or none.
CREATE TABLE project_source_credentials (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    -- XChaCha20-Poly1305, same scheme as secret environment variables and the
    -- Discord bot token.
    ciphertext BLOB NOT NULL,
    nonce      BLOB NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (length(ciphertext) > 0),
    CHECK (length(nonce) = 24)
);
