# Host run mode

Date: 2026-08-01
Status: implemented, 2026-08-07 — except §14 (resource limits) and the
`STATIC` stage. Developed on Windows; no Docker, WSL or Linux was available
here, so every Docker path remains unproven.

The Unix paths are no longer unproven, and they were wrong. CI on
ubuntu-22.04 found two defects in `platform::process` that no amount of
Windows testing could have shown:

- Ending a tree signalled a **negated pid**, which addresses a process
  group rather than a process. It did not go where the number said: the
  target survived and the caller was signalled, killing the CI runner
  outright. Trees are now ended by naming every pid, which cannot address
  a group at all.
- `is_alive` counted a **zombie** as running. On Unix an exited child keeps
  its row until it is reaped, so a process that stopped on time looked as
  though it never stopped, and `terminate_tree` spent the whole grace
  period waiting for it.

Both are fixed and both platforms are green. The lesson is worth keeping:
"unproven" was doing more work in this document than it looked like.

§14 will not land as written: a Windows memory cap needs a Job Object,
which needs `unsafe`, which the workspace forbids. Host projects are
therefore not capped, and admission control took its place — see
`2026-08-07-concurrent-projects-and-server-mode-design.md`.

Let a project run as an ordinary process on the user's machine instead of inside
a Docker container, chosen per project.

Today every project is a container, and a machine without a Docker daemon can
create projects, edit their files and keep their settings — but cannot run any
of them. The controls are disabled wholesale: `ProjectConsole.tsx` computes
`controlsDisabled = isBusy || isTransitioning || !dockerAvailable`. For a user
with no daemon, the product's central verb does not work.

This design adds a second execution substrate rather than replacing the first.
Docker projects keep today's behaviour exactly, including the parts host mode
cannot match.

---

## 1. What is given up, and why it is said out loud

A container supplies filesystem isolation, network isolation, resource limits, a
non-root user and a daemon that outlives the application. A host process
supplies none of those. It runs as the user, with the user's files and the
user's network, and it is a child of the application.

Two consequences are load-bearing for the whole design:

**Host-run projects stop when the application quits.** They are not detached and
not adopted on next launch. The alternative — recording a PID and reattaching —
fails on the case that matters: after a reboot the PID has been reused, and the
application adopts an unrelated process. Quitting therefore stops host projects
cleanly, states that it is doing so, and offers to start them again next launch.

**Host mode is never selected silently.** It is an explicit per-project choice
with a one-sentence statement of what is given up, confirmed once per project.
Host-run projects carry a `[host]` badge everywhere they appear. Where no Docker
daemon exists the host option is preselected — but the confirmation is still
required, so the trade is never invisible. This matters because the product
invites pasting an arbitrary GitHub URL, and outside a container that code runs
as the user.

---

## 2. Where run mode lives

A new column on `projects`, migration `0005_run_mode.sql`:

```sql
ALTER TABLE projects ADD COLUMN run_mode TEXT NOT NULL DEFAULT 'DOCKER'
    CHECK (run_mode IN ('DOCKER','HOST'));
```

Unlike `0003` and `0004` this needs no table rebuild. Those rebuilt because
SQLite cannot _alter_ a `CHECK` constraint; adding a new column that carries its
own is supported directly. The `DOCKER` default is what makes every existing
project keep its current behaviour without a data migration.

`ProjectRecord` gains `run_mode: RunMode`, and `api-types` gains the enum beside
`ProjectStatus` and `DesiredState`, with the same round-trip test the other wire
enums have.

---

## 3. The seam

`lifecycle::start` already separates cleanly into two halves. One is
substrate-neutral bookkeeping — set desired state, write `STARTING`, run the
work, then write the status that was _observed_ — and the module documentation
is emphatic that this rule is the reason `status` and `desired_state` are
separate columns. The other half, `start_inner`, is entirely Docker.

Only the second half is dispatched:

```rust
#[async_trait]
pub trait ProjectRunner: Send + Sync {
    async fn start(&self, ctx: &StartContext<'_>) -> Result<Observed, RunnerError>;
    async fn stop(&self, project: &ProjectRecord, grace: Option<Duration>)
        -> Result<(), RunnerError>;
    async fn kill(&self, project: &ProjectRecord) -> Result<(), RunnerError>;
    async fn observe(&self, project: &ProjectRecord) -> Result<Option<Observed>, RunnerError>;
}
```

`Observed` is the substrate-neutral answer to "what is true right now":
status, health, exit code, start time, failure reason. It is deliberately not
`ContainerState`, which carries Docker's vocabulary.

The trait speaks in project terms. It does not borrow `ensure_network`,
`ensure_volume`, `has_image` or `build_image` from `ContainerRunner` — four of
that type's ten methods are meaningless for a process, and an implementation
that stubbed them `Ok(())` would be evidence of a boundary drawn in the wrong
place. Those four stay inside the Docker implementation, which is today's
`start_inner` moved behind the trait and otherwise unchanged.

`restart` is not on the trait. Docker has a native restart; the host has stop
followed by start. The lifecycle layer expresses restart in terms of the two
primitives, and the Docker implementation is free to shortcut internally.

### 3.1 Crate layout

`crates/host-runner`, depending on neither `docker-manager` nor `api-types`,
for the same reason `detection` depends on neither:

| Module          | Responsibility                                               |
| --------------- | ------------------------------------------------------------ |
| `probe.rs`      | Find a runtime's executable on the host and read its version |
| `command.rs`    | Turn a `RuntimeSpec` into a concrete process command — pure  |
| `supervisor.rs` | Own one running child: output, health, restart, shutdown     |
| `health.rs`     | HTTP / TCP / command polling                                 |
| `output.rs`     | Pump stdout and stderr to the project's log file             |

---

## 4. From RuntimeSpec to a process

`RuntimeSpec` is already substrate-neutral — `install_command`,
`build_command`, `start_command`, `entry_file`, plus the health fields. Nothing
in it mentions Docker. One field is a trap:

**`working_dir` defaults to `/app` and must never reach a host command.** It is
a container path. Host mode uses the project's own directory, and there is a
test asserting that no built command carries `/app` in its working directory.

`command.rs` is a pure function — `RuntimeSpec` + project directory + resolved
toolchain + environment in, `ProcessCommand { program, args, cwd, env }` out. No
process is spawned to test it, which makes every runtime's translation
unit-testable on a machine with none of them installed. This is the
highest-value test surface in the design.

Install and build commands run the same way before the start command, as
separate short-lived processes whose failure fails the start with their output
attached.

### 4.1 Toolchain probing

For each runtime, a list of candidate executables (`node`; `python3` then
`python`; `dotnet`; and so on), resolved against `PATH` and then run with a
version flag. The result is `Found { executable, version }` or `NotFound`, and
host mode is offered only for runtimes that resolve. A project whose runtime is
missing is refused at plan time with a message naming what was looked for and
the version the project wants — not at the moment the user presses Start.

`STATIC` is the awkward case rather than the easy one. A static site needs no
_language_ toolchain, but it does need something to serve it, and in Docker that
something is the Caddy image (`runtime_plan.rs` maps `Runtime::Static` to
`caddy`, publishing `public` on port 80). On the host there is no equivalent:
the application has no HTTP server, and adds none today — `reqwest` appears in
the tree as a client, in `file-manager`'s archive fetching, and nothing serves.

Host mode for `STATIC` therefore needs a static file server that does not yet
exist, and that is a dependency decision rather than a detail. It is its own
stage, after the runtimes that need nothing new, and until it lands `STATIC` is
offered in Docker mode only — refused in host mode by the same mechanism and
with the same clarity as a missing toolchain.

Probing goes through an injected resolver so tests do not depend on what happens
to be installed on the machine running them.

---

## 5. The supervisor

Log capture, health checks and restart-on-crash together mean a long-lived task
per running host project. This is the largest new piece.

One `tokio` task owns:

- the child process, spawned in its own process group or job object
- two output pumps, stdout and stderr, line-oriented, to
  `logs/projects/<slug>/run-<date>.log`
- a health poller on the spec's interval, after its start period
- restart-on-crash with exponential backoff, capped at five attempts, after
  which the status is `FAILED` and no further attempt is made
- a stop signal: graceful termination first, hard kill after the grace period

A registry in `AppState` maps project id to supervisor handle. The supervisor
reports observed state back through the same `Observed` type, so the rule that
status is written from what is observed rather than what was intended holds
identically for both substrates.

### 5.1 Killing the tree

`npm start` spawns `node`. Killing `npm` leaves `node` running and holding the
port, and the next start then fails with a port conflict that has no visible
cause. Every host child is therefore spawned as a process-group leader and
terminated as a group:

**The workspace forbids `unsafe`.** `Cargo.toml` sets
`unsafe_code = "forbid"` for every crate, there is not one `unsafe` block in the
tree, and `forbid` cannot be downgraded by an `allow` at the crate or module
level. Raw Job Object and `setsid` calls are therefore not available, and any
design that reaches for them is wrong for this codebase rather than merely
unfashionable. What is available is safe and sufficient:

- **Windows** — `CommandExt::creation_flags(CREATE_NEW_PROCESS_GROUP)` at spawn,
  and `taskkill /T /F /PID` to end the tree. Both are safe std or subprocess
  calls.
- **Unix** — `CommandExt::process_group(0)` at spawn, stable since Rust 1.64 and
  safe, then signalling the group through a safe wrapper rather than raw `libc`.

The memory limit in §14 is affected by the same rule: a Job Object memory cap
needs `unsafe`, so on Windows it is either dropped or moved behind a crate that
encapsulates it. That is a decision for that stage, not this one.

Both live behind a `platform` capability. `docs/platform-support.md` is explicit
that OS differences live in that crate and nowhere else, and this is exactly the
kind of difference it means.

### 5.2 Shutdown

The supervisor registry subscribes to the existing `Shutdown` watch channel. On
trigger it stops every child, writes `STOPPED`, and records which projects were
running so the "start them again next launch" option has something to act on.

The quit path in the desktop shell gains a count of running host projects and
states plainly that they will stop while Docker projects will not.

---

## 6. Ports

A container publishes a mapped port; a host process binds one directly, so the
mapping collapses. The allocated host port is passed to the project through its
environment, and `container_port` is ignored in host mode.

`PortPool::is_available` is checked immediately before spawning. Docker fails a
port conflict with a clear daemon error; a host process may instead start and
then exit obscurely, or silently bind a different interface, so the check has to
happen on this side.

---

## 7. Interface

- **Create and settings** — a run-mode control, with the Docker option disabled
  and explained when no daemon is present, and the host option disabled and
  explained per runtime when the toolchain is missing. The probe result is shown
  rather than summarised: which executable was found, and at what version.
- **Confirmation** — one sentence on what is given up, plus an explicit
  acknowledgement. Nothing extra is stored for this: accepting is what sets
  `run_mode` to `HOST`, so a project already in host mode is not asked again,
  and switching back and forth asks each time it is switched _to_.
- **Badge** — `[host]` wherever a project appears.
- **Controls** — `controlsDisabled` becomes mode-aware. Today a missing daemon
  disables the controls for every project; it must disable them only for Docker
  projects. This one line is what makes the feature reachable at all.
- **Quit** — the count of host projects that will stop, and the restart-next-time
  option.

---

## 8. Errors

`RunnerError` carries the substrate-neutral cases plus the host-specific ones:

| Case                | Message names                                    |
| ------------------- | ------------------------------------------------ |
| `ToolchainMissing`  | runtime, version wanted, executables looked for  |
| `PortUnavailable`   | the port, and that something else holds it       |
| `SpawnFailed`       | the program that could not be started, and why   |
| `ExitedImmediately` | exit code, and the last lines of captured output |

`ExitedImmediately` is the reason log capture is in the first version. A project
that dies during startup otherwise leaves `FAILED` and nothing to read: a
container's output is retained by the daemon, a pipe's is gone when the process
is.

---

## 9. Testing

The machine this is written on has no Docker, no WSL and no Linux, so what can
and cannot be verified is stated rather than assumed.

Verifiable here, and the bulk of the value:

- `command.rs` — every runtime's translation, and that `/app` never leaks into a
  host command's working directory
- `probe.rs` — against an injected resolver, so results do not depend on the
  machine
- `health.rs` — against an in-process TCP listener and HTTP server
- the migration, following the existing schema tests
- supervisor behaviour on Windows: start, output capture, graceful stop, hard
  kill, backoff, and giving up after the cap

Not verifiable here, and to be treated as unproven until run elsewhere:

- process-group termination on Unix
- resource limits on Linux, whether via `setrlimit` or cgroup v2
- every Docker path, as today

`app-core/src/lifecycle.rs` already carries a header saying none of it has run
against a daemon. The host implementation gets an equally explicit note about
which platform it has run on.

---

## 10. Staging

Ordered so that each stage is useful and the riskiest, least verifiable work is
last:

1. `run_mode` column, enum, and the `ProjectRunner` trait with the Docker
   implementation behind it — no behaviour change, and the point at which the
   existing tests should still pass untouched
2. `probe.rs` and `command.rs` — pure, fully tested, nothing spawned
3. Supervisor: spawn, output capture, stop, kill, process groups
4. Health checks
5. Restart on crash with backoff
6. Resource limits, per OS
7. Interface: run mode, confirmation, badge, mode-aware controls, quit dialog
8. A static file server, and `STATIC` in host mode — separate because it is the
   one stage that adds a dependency rather than using what is already here

Stage 1 is worth landing on its own. It is a refactor with no user-visible
change, and it either leaves the Docker path exactly as it was or it does not —
which is much easier to judge before any host code exists to blame.
