-- More runtimes than Node, Python and static.
--
-- Ten more: TypeScript as its own family (a compile step, not a different
-- interpreter), Bun and Deno, Go, Rust, Java, PHP, Ruby, .NET, and POLYGLOT for
-- a project that genuinely needs several toolchains in one image.
--
-- SQLite cannot alter a CHECK constraint, so `project_runtimes` is rebuilt the
-- same way `projects` was in 0003: copy, drop, rename, with foreign keys
-- disabled by `Database::migrate` for the duration and checked afterwards.
--
-- This rebuild is the easy kind. Nothing references `project_runtimes` — it is a
-- leaf — so dropping it cascades nowhere. The rebuild is still done with the
-- column list written out rather than `SELECT *`, because a positional copy is
-- the mistake that survives review.

CREATE TABLE project_runtimes_rebuilt (
    project_id            TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    runtime               TEXT NOT NULL CHECK (runtime IN (
        'NODEJS','TYPESCRIPT','BUN','DENO','PYTHON','GO','RUST','JAVA','PHP',
        'RUBY','DOTNET','STATIC','POLYGLOT')),
    runtime_version       TEXT NOT NULL,
    package_manager       TEXT NOT NULL DEFAULT 'NONE' CHECK (package_manager IN (
        'PNPM','NPM','YARN','BUN','DENO','PIP','POETRY','UV','PIPENV',
        'GO_MODULES','CARGO','MAVEN','GRADLE','COMPOSER','BUNDLER','NUGET','NONE')),
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

INSERT INTO project_runtimes_rebuilt (
    project_id, runtime, runtime_version, package_manager,
    install_command, build_command, start_command, working_dir,
    entry_file, publish_dir, template_id,
    health_check_type, health_check_target,
    health_interval_s, health_timeout_s, health_retries, health_start_period_s
)
SELECT
    project_id, runtime, runtime_version, package_manager,
    install_command, build_command, start_command, working_dir,
    entry_file, publish_dir, template_id,
    health_check_type, health_check_target,
    health_interval_s, health_timeout_s, health_retries, health_start_period_s
FROM project_runtimes;

DROP TABLE project_runtimes;

ALTER TABLE project_runtimes_rebuilt RENAME TO project_runtimes;
