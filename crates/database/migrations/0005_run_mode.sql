-- How a project runs: inside a container, or as a process on this machine.
--
-- No table rebuild here, unlike 0003 and 0004. Those rebuilt because SQLite
-- cannot *alter* an existing CHECK constraint; adding a new column that brings
-- its own is supported directly, and a rebuild of `projects` is the expensive
-- kind — four tables reference it.
--
-- DOCKER as the default is the whole compatibility story. Every project that
-- exists was created when a container was the only way to run one, and reads
-- back as a container project without a data migration touching a single row.
ALTER TABLE projects ADD COLUMN run_mode TEXT NOT NULL DEFAULT 'DOCKER'
    CHECK (run_mode IN ('DOCKER','HOST'));
