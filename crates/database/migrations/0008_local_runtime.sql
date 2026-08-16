-- Local processes become the way a project runs, and the row gains the columns
-- that only make sense once it does.
--
-- Four changes, one rebuild. SQLite cannot alter a CHECK constraint or a column
-- default, and three of the four need one, so `projects` is rebuilt the way
-- 0003 rebuilt it: copy, drop, rename, with foreign keys disabled for the
-- duration by `Database::migrate` and verified afterwards.
--
-- 1. `status` gains 'CRASHED'. Until now a project that started cleanly and
--    then died was written as FAILED, which is the same word used for a project
--    that never started at all. Those are different situations with different
--    answers — one is "your code threw", the other is "the runtime is missing"
--    — and a status column that cannot tell them apart forces the interface to
--    guess.
--
-- 2. `run_mode` defaults to 'HOST', and every existing row is moved to it. The
--    product no longer asks the user to install a container daemon to run their
--    own code on their own machine. The DOCKER value is kept in the CHECK
--    because `docker-manager` still compiles and a hand-set row must remain
--    readable, but nothing in the interface produces it any more.
--
-- 3. `priority` records what the resource manager should do when this machine
--    is under pressure. NORMAL for everything that exists, because a priority
--    nobody chose is not a preference.
--
-- 4. `keep_awake` records whether this project's availability should hold sleep
--    off while it runs. Off by default: preventing a machine from sleeping is
--    something the user opts into, per project, not something a new install
--    starts doing.

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
        'BUILDING','CRASHED','FAILED','UNHEALTHY','ARCHIVED','DELETING')),
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

    source_url     TEXT,
    source_ref     TEXT,
    source_commit  TEXT,

    autostart      INTEGER NOT NULL DEFAULT 0 CHECK (autostart IN (0, 1)),
    restart_policy TEXT NOT NULL DEFAULT 'UNLESS_STOPPED' CHECK (restart_policy IN (
        'NO','ON_FAILURE','UNLESS_STOPPED','ALWAYS')),
    network_mode   TEXT NOT NULL DEFAULT 'INTERNAL' CHECK (network_mode IN (
        'NONE','INTERNAL','LAN','INTERNET')),
    run_mode       TEXT NOT NULL DEFAULT 'HOST' CHECK (run_mode IN ('DOCKER','HOST')),

    -- What the resource manager should sacrifice last. Never a hard limit: a
    -- cap that can terminate a workload is exactly what §7 of the request rules
    -- out, so this only ever moves the operating system's scheduling priority.
    priority       TEXT NOT NULL DEFAULT 'NORMAL' CHECK (priority IN ('LOW','NORMAL','HIGH')),
    -- Whether this project running is a reason to hold off automatic sleep.
    keep_awake     INTEGER NOT NULL DEFAULT 0 CHECK (keep_awake IN (0, 1)),

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
    CHECK (slug GLOB '[a-z0-9][a-z0-9-]*'),
    CHECK (source_type NOT IN ('GIT_CLONE','REMOTE_ARCHIVE') OR source_url IS NOT NULL),
    CHECK (source_type IN ('GIT_CLONE','REMOTE_ARCHIVE') OR source_url IS NULL),
    CHECK (source_type = 'GIT_CLONE' OR (source_ref IS NULL AND source_commit IS NULL)),
    CHECK (source_url IS NULL OR source_url NOT LIKE '%@%')
);

-- Column list written out rather than `SELECT *`, for the reason 0003 gives:
-- the two new columns have no counterpart in the old table and a positional
-- copy is the mistake that survives review.
--
-- `run_mode` is the one value not copied through unchanged. Every project moves
-- to HOST, including those already there. A user who deliberately chose DOCKER
-- for a project loses that choice here, which is the point: the daemon is no
-- longer a requirement this product places on anybody, and leaving a handful of
-- rows pointing at a substrate the interface no longer exposes would mean a
-- Start button that fails for reasons the window has stopped being able to
-- explain.
INSERT INTO projects_rebuilt (
    id, slug, display_name, description, project_type, icon, color,
    status, desired_state, health,
    container_id, container_name, image_tag, network_name, volume_name,
    source_type, directory, source_url, source_ref, source_commit,
    autostart, restart_policy, network_mode, run_mode,
    memory_limit_mb, cpu_limit_cores, storage_limit_mb, process_limit,
    started_at, stopped_at, last_exit_code, last_failure_at,
    last_failure_reason, restart_count,
    archived_at, created_at, updated_at
)
SELECT
    id, slug, display_name, description, project_type, icon, color,
    status, desired_state, health,
    container_id, container_name, image_tag, network_name, volume_name,
    source_type, directory, source_url, source_ref, source_commit,
    autostart, restart_policy, network_mode, 'HOST',
    memory_limit_mb, cpu_limit_cores, storage_limit_mb, process_limit,
    started_at, stopped_at, last_exit_code, last_failure_at,
    last_failure_reason, restart_count,
    archived_at, created_at, updated_at
FROM projects;

DROP TABLE projects;

ALTER TABLE projects_rebuilt RENAME TO projects;

CREATE INDEX idx_projects_status ON projects (status);
CREATE INDEX idx_projects_desired ON projects (desired_state) WHERE archived_at IS NULL;
