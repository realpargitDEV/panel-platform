# Host Run Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a project run as an ordinary host process instead of a Docker container, chosen per project, so a machine with no Docker daemon can still run its projects.

**Architecture:** A `ProjectRunner` trait in `app-core` splits substrate-neutral lifecycle bookkeeping from substrate-specific work. `DockerRunner` wraps today's `start_inner` unchanged; `HostRunner` (new crate `crates/host-runner`) spawns and supervises processes. Run mode is a column on `projects` defaulting to `DOCKER`, so nothing existing changes behaviour.

**Tech Stack:** Rust, tokio, sqlx/SQLite, Tauri 2, React 19.

Spec: `docs/superpowers/specs/2026-08-01-host-run-mode-design.md`

## Global Constraints

- `run_mode` values are exactly `DOCKER` and `HOST`; the column defaults to `DOCKER`.
- `working_dir` from `RuntimeSpec` is a container path (`/app`) and must never reach a host command.
- Status is written from what is **observed**, never from what was intended.
- No `#[cfg(windows)]` / `#[cfg(unix)]` outside `crates/platform` and that crate's own tests.
- Host projects stop when the app quits; they are never detached or adopted by PID.
- Every command runs through `pnpm verify` (contracts, rustfmt, clippy `-D warnings`, cargo test, tsc, eslint, prettier, vitest) before commit.
- The machine this is developed on has no Docker, no WSL and no Linux. Unix and Docker paths are unverifiable here and must be marked as such in module docs.

---

### Task 1: `RunMode` enum in `api-types`

**Files:**

- Modify: `crates/api-types/src/enums.rs`
- Modify: `crates/api-types/src/lib.rs` (re-export)

**Interfaces:**

- Produces: `RunMode::{Docker, Host}`, `RunMode::as_str() -> &'static str`, `FromStr for RunMode`.

- [ ] **Step 1: Write the failing test**

In `crates/api-types/src/enums.rs` tests:

```rust
#[test]
fn run_mode_round_trips_through_its_wire_value() {
    for mode in [RunMode::Docker, RunMode::Host] {
        assert_eq!(RunMode::from_str(mode.as_str()), Ok(mode));
    }
    assert_eq!(RunMode::Docker.as_str(), "DOCKER");
    assert_eq!(RunMode::Host.as_str(), "HOST");
    assert!(RunMode::from_str("PODMAN").is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-api-types run_mode`
Expected: FAIL, `RunMode` not found.

- [ ] **Step 3: Implement**

Follow the existing macro/pattern used by `ProjectStatus` and `DesiredState` in the same file. Default is `Docker`.

- [ ] **Step 4: Run test**

Run: `cargo test -p project-host-api-types run_mode` → PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api-types
git commit -m "Add RunMode enum"
```

---

### Task 2: `run_mode` column

**Files:**

- Create: `crates/database/migrations/0005_run_mode.sql`
- Modify: `crates/database/src/projects.rs` (`ProjectRecord`, `project_from_row`, insert)
- Test: `crates/database/tests/schema.rs`

**Interfaces:**

- Consumes: `RunMode` from Task 1 (as a `String` at the database boundary, matching how `status` is stored).
- Produces: `ProjectRecord.run_mode: String`, `projects::set_run_mode(db, project_id, mode) -> Result<(), DatabaseError>`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn projects_default_to_docker_and_accept_host() {
    let db = memory_database().await;
    let id = insert_test_project(&db, "demo").await;

    let project = projects::find_project(&db, &id).await.expect("query").expect("row");
    assert_eq!(project.run_mode, "DOCKER", "an existing project must keep its behaviour");

    projects::set_run_mode(&db, &id, "HOST").await.expect("set");
    let project = projects::find_project(&db, &id).await.expect("query").expect("row");
    assert_eq!(project.run_mode, "HOST");

    assert!(projects::set_run_mode(&db, &id, "PODMAN").await.is_err(), "the CHECK must hold");
}
```

- [ ] **Step 2: Run to verify it fails.** `cargo test -p project-host-database run_mode`

- [ ] **Step 3: Write the migration**

```sql
-- How a project runs: in a container, or as a process on this machine.
--
-- No table rebuild. 0003 and 0004 rebuilt because SQLite cannot alter an
-- existing CHECK constraint; adding a new column that carries its own is
-- supported directly. DOCKER is the default so every project that exists
-- keeps behaving exactly as it did.
ALTER TABLE projects ADD COLUMN run_mode TEXT NOT NULL DEFAULT 'DOCKER'
    CHECK (run_mode IN ('DOCKER','HOST'));
```

Add `run_mode` to `ProjectRecord` and `project_from_row`, and write `set_run_mode`.

- [ ] **Step 4: Run tests.** `cargo test -p project-host-database` → PASS.

- [ ] **Step 5: Commit**

---

### Task 3: `ProjectRunner` trait and `Observed`

**Files:**

- Create: `crates/app-core/src/runner.rs`
- Modify: `crates/app-core/src/lib.rs`

**Interfaces:**

- Produces:

```rust
pub struct Observed {
    pub status: ProjectStatus,
    pub health: Option<HealthState>,
    pub exit_code: Option<i64>,
    pub failure_reason: Option<String>,
}

#[async_trait::async_trait]
pub trait ProjectRunner: Send + Sync {
    async fn start(&self, ctx: StartContext<'_>) -> Result<Observed, LifecycleError>;
    async fn stop(&self, project: &ProjectRecord) -> Result<(), LifecycleError>;
    async fn kill(&self, project: &ProjectRecord) -> Result<(), LifecycleError>;
    async fn observe(&self, project: &ProjectRecord) -> Result<Option<Observed>, LifecycleError>;
}

pub struct StartContext<'a> {
    pub project: &'a ProjectRecord,
    pub runtime: &'a projects::RuntimeRecord,
    pub directory: &'a Path,
    pub app_version: &'a str,
}
```

- [ ] **Step 1: Write the failing test** — a fake runner asserting the trait is object-safe and usable behind `Arc<dyn ProjectRunner>`.

```rust
#[tokio::test]
async fn a_runner_can_be_held_behind_a_trait_object() {
    struct Fake;
    #[async_trait::async_trait]
    impl ProjectRunner for Fake {
        async fn start(&self, _: StartContext<'_>) -> Result<Observed, LifecycleError> {
            Ok(Observed { status: ProjectStatus::Running, health: None, exit_code: None, failure_reason: None })
        }
        async fn stop(&self, _: &ProjectRecord) -> Result<(), LifecycleError> { Ok(()) }
        async fn kill(&self, _: &ProjectRecord) -> Result<(), LifecycleError> { Ok(()) }
        async fn observe(&self, _: &ProjectRecord) -> Result<Option<Observed>, LifecycleError> { Ok(None) }
    }
    let runner: std::sync::Arc<dyn ProjectRunner> = std::sync::Arc::new(Fake);
    assert!(runner.observe(&sample_project()).await.expect("observe").is_none());
}
```

- [ ] **Step 2: Run to verify it fails.**
- [ ] **Step 3: Implement `runner.rs`.**
- [ ] **Step 4: Run tests.**
- [ ] **Step 5: Commit.**

---

### Task 4: `DockerRunner` — today's behaviour behind the trait

**Files:**

- Create: `crates/app-core/src/runner/docker.rs`
- Modify: `crates/app-core/src/lifecycle.rs` (`start_inner`, `stop`, `kill`, `restart` move behind the trait)

**Interfaces:**

- Consumes: `ProjectRunner`, `Observed`, `StartContext` (Task 3).
- Produces: `DockerRunner::new()`, and `lifecycle::runner_for(&ProjectRecord) -> Arc<dyn ProjectRunner>`.

This task is a **pure refactor**. The deliverable is that every existing test in `crates/app-core` passes untouched.

- [ ] **Step 1:** Move the body of `start_inner` into `DockerRunner::start`, converting its return into `Observed` via the existing `project_status`/`health_state` helpers.
- [ ] **Step 2:** Move `stop`/`kill`'s Docker calls into `DockerRunner::stop`/`kill`, leaving the desired-state and status writes in `lifecycle.rs`.
- [ ] **Step 3:** Add `runner_for`, returning `DockerRunner` for every project (host mode does not exist yet).
- [ ] **Step 4:** Run `cargo test -p project-host-core` — every existing test must pass with no edits to the tests themselves. If a test needed changing, the refactor changed behaviour.
- [ ] **Step 5: Commit.**

---

### Task 5: Toolchain probe

**Files:**

- Create: `crates/host-runner/Cargo.toml`, `crates/host-runner/src/lib.rs`, `crates/host-runner/src/probe.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**

- Produces:

```rust
pub trait ExecutableResolver: Send + Sync {
    /// Absolute path to `name` on this machine, or None.
    fn resolve(&self, name: &str) -> Option<PathBuf>;
}
pub struct PathResolver;                    // real, searches PATH
pub enum Toolchain { Found { executable: PathBuf, version: String }, Missing { looked_for: Vec<String> } }
pub fn candidates_for(runtime: &str) -> &'static [&'static str];
pub fn probe(runtime: &str, resolver: &dyn ExecutableResolver) -> Toolchain;
```

- [ ] **Step 1: Write the failing tests**

```rust
struct FakeResolver(Vec<&'static str>);
impl ExecutableResolver for FakeResolver {
    fn resolve(&self, name: &str) -> Option<PathBuf> {
        self.0.contains(&name).then(|| PathBuf::from(format!("/usr/bin/{name}")))
    }
}

#[test]
fn python3_is_preferred_over_python() {
    let found = probe("PYTHON", &FakeResolver(vec!["python3", "python"]));
    assert!(matches!(found, Toolchain::Found { executable, .. } if executable.ends_with("python3")));
}

#[test]
fn a_missing_toolchain_reports_everything_it_looked_for() {
    match probe("GO", &FakeResolver(vec![])) {
        Toolchain::Missing { looked_for } => assert_eq!(looked_for, vec!["go"]),
        other => panic!("expected Missing, got {other:?}"),
    }
}

#[test]
fn every_runtime_has_at_least_one_candidate_executable() {
    for runtime in ["NODEJS","TYPESCRIPT","BUN","DENO","PYTHON","GO","RUST","JAVA","PHP","RUBY","DOTNET","POLYGLOT"] {
        assert!(!candidates_for(runtime).is_empty(), "{runtime} has no candidates");
    }
}
```

- [ ] **Step 2: Run to verify they fail.**
- [ ] **Step 3: Implement.** Version is read by running `<exe> --version` and taking the first line; that call is behind the resolver-injected path so tests never spawn.
- [ ] **Step 4: Run tests.** `cargo test -p project-host-host-runner probe`
- [ ] **Step 5: Commit.**

---

### Task 6: `RuntimeSpec` → `ProcessCommand` (pure)

**Files:**

- Create: `crates/host-runner/src/command.rs`

**Interfaces:**

- Consumes: `Toolchain` (Task 5).
- Produces:

```rust
pub struct ProcessCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}
pub struct CommandInputs<'a> {
    pub runtime: &'a str,
    pub start_command: &'a str,
    pub project_directory: &'a Path,
    pub toolchain: &'a Toolchain,
    pub env: BTreeMap<String, String>,
    pub port: Option<u16>,
}
pub fn start_command(inputs: CommandInputs<'_>) -> Result<ProcessCommand, CommandError>;
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_container_working_directory_never_reaches_a_host_command() {
    let command = start_command(inputs("NODEJS", "npm start")).expect("command");
    assert_eq!(command.cwd, PathBuf::from("/projects/demo"));
    assert!(!command.cwd.to_string_lossy().contains("/app"), "the container path leaked");
}

#[test]
fn the_allocated_port_is_passed_through_the_environment() {
    let command = start_command(CommandInputs { port: Some(8081), ..inputs("NODEJS", "npm start") }).expect("c");
    assert_eq!(command.env.get("PORT").map(String::as_str), Some("8081"));
}

#[test]
fn a_missing_toolchain_refuses_to_build_a_command() {
    let missing = Toolchain::Missing { looked_for: vec!["node".into()] };
    assert!(matches!(
        start_command(CommandInputs { toolchain: &missing, ..inputs("NODEJS", "npm start") }),
        Err(CommandError::ToolchainMissing { .. })
    ));
}

#[test]
fn quoted_arguments_survive_splitting() {
    let command = start_command(inputs("PYTHON", "python -m http.server --bind \"127.0.0.1\"")).expect("c");
    assert_eq!(command.args.last().map(String::as_str), Some("127.0.0.1"));
}
```

- [ ] **Step 2: Run to verify they fail.**
- [ ] **Step 3: Implement.** Split `start_command` respecting quotes; resolve the first word against the toolchain where it names the runtime's own executable, otherwise resolve it as a sibling tool (`npm` beside `node`).
- [ ] **Step 4: Run tests.**
- [ ] **Step 5: Commit.**

---

### Task 7: Process groups in `platform`

**Files:**

- Create: `crates/platform/src/process.rs`
- Modify: `crates/platform/src/lib.rs`

**Interfaces:**

- Produces: `ProcessGroup::spawn(command: ProcessCommand) -> io::Result<GroupedChild>`, `GroupedChild::{terminate(grace), kill(), id(), wait()}`.

Windows uses a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; Unix uses `setsid` at spawn and `kill(-pgid, …)` at stop. Both `#[cfg]` blocks live in this file and nowhere else.

- [ ] **Step 1:** Test that a spawned child that itself spawns a grandchild leaves no survivor after `kill()`. Windows-verifiable; the Unix arm is `#[cfg(unix)]` and marked unverified in the module doc.
- [ ] **Step 2–5:** Run, implement, run, commit.

---

### Task 8: Supervisor with output capture

**Files:**

- Create: `crates/host-runner/src/supervisor.rs`, `crates/host-runner/src/output.rs`

**Interfaces:**

- Produces: `Supervisor::start(ProcessCommand, log_path) -> Result<SupervisorHandle, HostError>`, `SupervisorHandle::{observe(), stop(grace), kill()}`.

- [ ] **Step 1:** Test that a child writing to stdout has its output in the log file, and that a child exiting non-zero is observed as `FAILED` with its exit code and last output lines.
- [ ] **Step 2–5:** Run, implement, run, commit.

---

### Task 9: Health checks

**Files:** Create `crates/host-runner/src/health.rs`

- [ ] Poll `HTTP` (GET, 2xx/3xx healthy), `TCP` (connect succeeds), `COMMAND` (exit 0), honouring interval/timeout/retries/start period. Tested against an in-process listener.

---

### Task 10: Restart on crash

**Files:** Modify `crates/host-runner/src/supervisor.rs`

- [ ] Exponential backoff, capped at five attempts, then `FAILED` with no further attempts. Tested with a child that always exits non-zero, asserting exactly five spawns.

---

### Task 11: `HostRunner` implements `ProjectRunner`

**Files:** Create `crates/app-core/src/runner/host.rs`; modify `lifecycle::runner_for` to dispatch on `project.run_mode`.

- [ ] The registry of supervisors lives in `AppState`; `runner_for` returns `HostRunner` when `run_mode == "HOST"`.

---

### Task 12: Shutdown stops host projects

**Files:** Modify `crates/app-core/src/shutdown.rs` consumers, `apps/desktop/src-tauri/src/lib.rs`

- [ ] On the `Shutdown` watch firing, every supervisor is stopped and its project written `STOPPED`. Test: trigger shutdown with two fake supervisors registered, assert both stopped.

---

### Task 13: Interface

**Files:** Modify `apps/desktop/src/views/Projects.tsx`, `ProjectConsole.tsx`, `api.ts`; add Tauri commands.

- [ ] `controlsDisabled` becomes mode-aware — today `!dockerAvailable` disables controls for **every** project; it must disable them only for Docker projects. Without this the feature is unreachable.
- [ ] Run-mode control with per-runtime probe results, the isolation confirmation, and the `[host]` badge.
- [ ] Quit dialog naming how many host projects will stop.

---

### Task 14: Resource limits

**Files:** Modify `crates/platform/src/process.rs`

- [ ] Memory cap via Job Object on Windows, `setrlimit`/cgroup v2 on Linux. Last because it is the most per-OS work and the least verifiable here.

---

### Task 15: Static file server, `STATIC` in host mode

**Files:** Create `crates/host-runner/src/static_site.rs`

- [ ] The only task that adds a dependency. Until it lands, `STATIC` is refused in host mode like a missing toolchain.
