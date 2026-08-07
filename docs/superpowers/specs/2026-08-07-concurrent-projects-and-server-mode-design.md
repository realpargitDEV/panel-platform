# Running many projects at once, and tuning the machine to take it

Date: 2026-08-07
Status: designed, not implemented

Three pieces that only make sense together: a governor that decides whether the
machine can take one more project, a way to see and control everything that is
running, and a scan that proposes machine settings suited to hosting rather than
to a desktop.

Host run mode is a prerequisite and is designed separately in
`2026-08-01-host-run-mode-design.md`. That spec's staging is unchanged; this one
begins where it ends.

---

## 0. What the request was, and how it is read

Two goals were stated: run projects without Docker, and run a lot of them at
once. A third arrived after: scan the machine and change its settings so it
behaves like a private server and does not fall over.

The scale answer was "as many as the machine allows", which is the demanding
reading. It rules out a fixed cap in configuration — `max_projects` already
exists and defaults to 50, and a second constant would not be an answer. It
means the number is discovered from the machine, at the moment of asking.

The behaviour at the limit was chosen as refuse-and-explain rather than queue or
warn. So the governor's output is a decision with numbers attached, not a hint.

---

## 1. Why a governor is needed at all, and only now

Today a project is a container, and a container carries `ResourceLimits` —
memory, CPU and process caps that the daemon enforces. Twenty containers on a
machine that cannot hold twenty are twenty containers that get OOM-killed
individually, which is bad but bounded, and Docker does the bounding.

Host mode removes the bounding. `2026-08-01-host-run-mode-design.md` §5.1
established that the workspace sets `unsafe_code = "forbid"` and that this
cannot be downgraded by an `allow`; §14 followed that to its conclusion for
Windows, where a Job Object memory cap needs `unsafe` and is therefore
unavailable. A host project's memory use is consequently **not capped on
Windows**. This is the fact the whole design turns on:

> For host projects the application cannot enforce a limit. It can only decline
> to start what will not fit, and report what is happening once it has.

That asymmetry is why admission control is the mechanism rather than
enforcement. It is also why refusal is the right behaviour at the limit: with
nothing to catch an overcommit, allowing it means the machine swaps and the user
loses work in every application, not only this one.

### 1.1 Both substrates, one budget

The governor covers Docker and host projects alike. A machine does not have two
pools of memory, and a user running six containers and four host processes is
running ten things on one machine. Container usage is read from the daemon's
stats endpoint; host usage is sampled per process tree. They land in the same
type and the same total.

Where Docker is absent the container half is simply empty, which is the
situation on the development machine and must not be an error.

---

## 2. Measuring

### 2.1 What is sampled

A new crate, `crates/resources`, depending on neither `docker-manager` nor
`app-core` — the same independence `detection` and `host-runner` have, and for
the same reason: whether a machine has room is a question that should be
answerable without starting anything.

```rust
pub struct Usage {
    pub memory_bytes: u64,
    pub cpu_percent: f32,
}

pub struct MachineUsage {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub cpu_percent: f32,
    pub logical_cores: u32,
}

pub trait UsageSource: Send + Sync {
    fn machine(&self) -> MachineUsage;
    /// Usage of a process and every descendant, or None if the tree is gone.
    fn process_tree(&self, root_pid: u32) -> Option<Usage>;
}
```

`sysinfo` is already a dependency — `apps/desktop/src-tauri/src/lib.rs:1350`
uses it for disks and `crates/platform` for the system snapshot — so the real
implementation adds no new crate to the tree. It refreshes processes once per
tick and walks parent links to sum a tree, because `npm start`'s memory lives in
the `node` it spawned rather than in `npm`.

Docker usage comes from the daemon's stats API behind the same trait, so a
mixed machine has one list.

### 2.2 The sampler

One `tokio` task on a two-second interval, owned by `AppState`, holding the last
sample in an `RwLock` behind `AppState::usage()`. Two seconds is chosen against
the existing precedent: `docker_status` is already refreshed on a timer rather
than probed per call, with a comment explaining that a status bar pinging Docker
per render turns a slow daemon into a slow interface. Sampling process trees per
render would be the same mistake with a bigger constant.

The sample is a snapshot, never a stream of events. A consumer that wants
history keeps its own.

### 2.3 What sampling costs, and the honest caveat

Walking every process on the machine twice a second is not free, and on a
machine under the pressure this feature exists to detect it is least free. The
sampler therefore refreshes only what it needs — process list and memory, not
disks, not networks — and holds no lock while sampling.

CPU percentage from `sysinfo` is a delta between refreshes and is meaningless on
the first tick. The first sample after start reports CPU as unknown rather than
zero. A governor that read the first tick as "0% busy, plenty of room" would
admit a project on a machine that is pinned.

---

## 3. Admission

### 3.1 The decision

```rust
pub enum Admission {
    Allow,
    Refuse(Shortfall),
}

pub struct Shortfall {
    pub wanted_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub headroom_bytes: u64,
    pub running: Vec<RunningProject>,   // name and current usage, largest first
}
```

`Admission` is computed by a pure function from a `MachineUsage`, the requesting
project's declared limits, and the current running set. Pure because this is the
piece that most needs testing on a machine that is not under load: the whole
range of "1 GB free, wants 2 GB" cases is unreachable by arranging for the
development machine to actually be out of memory.

### 3.2 Headroom

The machine's free memory is not the budget. An operating system that is down to
its last 200 MB is already thrashing, and a governor that spent the last byte
would be the thing that killed the machine.

A reserve is held back — the larger of 2 GB or 15% of total — and admission is
against what remains. The reserve is configuration (`memory_reserve_mb`,
defaulting to the computed value) because the right number differs between a
laptop that is also running a browser and a spare box doing nothing else.

### 3.3 What a project is assumed to want

Its own `memory_limit_mb`, the column that already exists and that
`ResourceDefaults` seeds from the machine tier. For a Docker project this is
also what the daemon will enforce, so it is exact. For a host project it is an
estimate and is labelled as one in the interface: the number is what the project
is *allowed* under Docker and *expected* to use on the host.

Once a host project has run, its observed peak is recorded and used in place of
the estimate on subsequent starts. A project that turned out to need 3 GB is
admitted against 3 GB the second time. This is the one piece of state the
governor keeps across restarts, and it lives on the project row:

```sql
ALTER TABLE projects ADD COLUMN observed_peak_memory_bytes INTEGER;
```

Null means never run, which is exactly "fall back to the estimate".

### 3.4 Where it is enforced

In `lifecycle::start`, before the runner is dispatched — the substrate-neutral
half described in `2026-08-01-host-run-mode-design.md` §3. Both substrates get
the check without either runner knowing the governor exists.

An override is offered on refusal, because the estimate can be wrong and the
user knows things the governor does not. Overriding starts the project and marks
that start as overridden in the log. It is a button in a dialog that states the
numbers, not a setting that turns the governor off.

---

## 4. Starting many at once

### 4.1 Staggering

Ten projects started together run ten `npm install`s together. Each is
disk-bound and CPU-hungry, and together they are slower than the same ten in
sequence while also making the machine unusable.

A bulk start is therefore a queue with a small concurrency limit — the number of
physical cores, capped at four — where "in progress" means from spawn until the
project is observed `RUNNING` or `FAILED`. Admission is evaluated per project as
its turn comes, not once for the batch, because the projects ahead of it have
changed the answer by the time it starts.

This is the only queue in the design. Section 0 recorded that queueing was
rejected as the answer to a *single* refused start; a user who selected twelve
projects and pressed Start has already said they want them all, and running them
four at a time is carrying out that instruction rather than deferring it.

### 4.2 The running view

A view across every project that is up, whatever its substrate: name, mode
badge, status, health, port, uptime, memory, CPU. Sorted by memory descending by
default, because the question it exists to answer is "what is eating this
machine".

It carries the machine total and the headroom, and it is where a refusal sends
you — a refusal message that says "2.1 GB free, this needs 4 GB" is only
actionable next to the list of what is holding the rest.

Bulk select, then start, stop or kill. Stop and kill need no admission check.

### 4.3 Quit

`2026-08-01-host-run-mode-design.md` §5.2 already specifies that quitting stops
host projects and offers to start them again next launch, and §7 that the quit
dialog names the count. With many projects that dialog becomes the difference
between a clean shutdown and a user force-quitting the app because it seemed to
hang: stopping fifteen process trees takes time, so the dialog shows progress
per project rather than a spinner.

---

## 5. Server mode

The third request: scan the machine's components and change its settings so it
suits hosting and does not crash.

### 5.1 What this design will and will not do

It will not silently change operating-system settings. The app proposes; the
user chooses; every change is recorded and reversible. Three reasons, in order
of weight:

1. Several of these settings are global and affect every application on the
   machine. A user who installed a project runner did not consent to having
   their power plan and their antivirus configuration rewritten.
2. Most of the changes worth making need administrator rights. A privilege
   prompt the user cannot connect to an action they took is a prompt they should
   refuse.
3. A tuning tool that cannot undo itself is a tool that turns one bad guess into
   a support burden.

So the shape is: scan, propose a list with current and proposed values, apply
only what is selected, record the previous value, offer revert — individually
and all at once.

### 5.2 The scan

`crates/platform`'s `SystemSnapshot` already carries CPU, memory, volumes,
architecture, OS build, virtualization and GPUs, gathered through the injectable
`SystemProbe`. The scan is that, plus the settings-specific readings that no
existing code needs:

| Reading            | Windows source                       | Why it matters                                                        |
| ------------------ | ------------------------------------ | --------------------------------------------------------------------- |
| Active power plan  | `powercfg /getactivescheme`          | A balanced plan parks cores and sleeps the machine under a server load |
| Sleep and hibernate timeouts | `powercfg /query`          | A machine that sleeps kills every running project                      |
| Page file          | `Win32_PageFileSetting`              | Too small is the most common cause of an out-of-memory crash           |
| Defender exclusions| `Get-MpPreference`                   | Real-time scanning of `node_modules` is a large, invisible cost        |
| Ephemeral port range | `netsh int ipv4 show dynamicport tcp` | Many servers plus many restarts exhausts the default range          |

Every one of these is a read first. A proposal is only shown when the current
value is known and is worse than the proposal — never as a blanket checklist.

### 5.3 The proposals

| Proposal                       | Effect                                     | Admin | Reversible |
| ------------------------------ | ------------------------------------------ | ----- | ---------- |
| High-performance power plan    | Cores stop parking; latency drops          | No    | Yes        |
| Never sleep while projects run | The machine stays up to serve              | No    | Yes        |
| Page file ≥ 1.5× RAM, system-managed | Overcommit swaps instead of killing  | Yes   | Yes        |
| Defender exclusion for `projects_dir` | Installs and builds stop being scanned | Yes | Yes      |
| Ephemeral ports 10000–65534    | Room for many listeners and their TIME_WAIT | Yes  | Yes        |

Each carries a one-sentence statement of what it gives up. The Defender
exclusion's is the one that matters and is stated plainly: **code fetched from
an arbitrary GitHub URL will run in that directory without real-time scanning.**
That is a real reduction in protection, it is the user's to make, and it is
off by default.

"Never sleep while projects run" is scoped rather than permanent: the setting is
applied when the first project starts and restored when the last one stops, so a
laptop does not silently lose its sleep behaviour forever. This is the only
proposal that is not a one-time change, and it is the one users will most regret
being permanent.

### 5.4 Recording and reverting

```sql
CREATE TABLE machine_settings (
    key           TEXT PRIMARY KEY,
    previous      TEXT NOT NULL,
    applied       TEXT NOT NULL,
    applied_at    TEXT NOT NULL,
    reverted_at   TEXT
);
```

`previous` is captured from the live read immediately before the change, not
from an assumed default. Revert writes `previous` back and stamps `reverted_at`.
A row whose `previous` could not be read is a change that is not offered — if it
cannot be undone it is not proposed.

### 5.5 Platform reach

Every reading and every change above is Windows. `docs/platform-support.md` is
explicit that OS differences live in `crates/platform` and nowhere else, so
these live there behind a `MachineTuning` capability whose Linux and macOS
implementations report "no proposals" rather than being absent. Linux
equivalents — `vm.swappiness`, `fs.inotify.max_user_watches`, cgroup limits, the
CPU governor — are real and worth having, and are explicitly out of scope here
rather than forgotten: they cannot be verified on this machine at all.

---

## 6. Testing

The machine this is written on runs Windows 11, with no Docker, no WSL and no
Linux. Following the precedent set by `app-core/src/lifecycle.rs`'s header, what
is proven and what is merely written is stated per area.

Verifiable here, and where the value is:

- Admission arithmetic — every branch, against constructed `MachineUsage`
  values, including the first-tick unknown-CPU case and the reserve boundary
- The stagger queue — with a fake runner, asserting the concurrency limit holds
  and that admission is re-evaluated per project rather than per batch
- Process-tree summing — against a spawned child that spawns a grandchild
- Parsers for every `powercfg`, `netsh` and `Get-MpPreference` output, as pure
  functions over captured text, exactly as `probe/platform_specific.rs` already
  parses `wsl --status` and GPU listings
- The migrations, following the existing schema tests
- Revert restoring the recorded previous value

Not verifiable here, and to be treated as unproven:

- Applying any elevated setting, on any machine, since that needs a privilege
  prompt and a machine to spoil
- Docker's stats endpoint, as with every Docker path today
- All Linux and macOS behaviour

---

## 7. Staging

1. `crates/resources`: `UsageSource`, the `sysinfo` implementation, process-tree
   summing. Nothing consumes it yet.
2. The sampler in `AppState`, and `observed_peak_memory_bytes`.
3. Admission, pure, and its check in `lifecycle::start` with the override.
4. The running view, with live usage.
5. Bulk start with the stagger queue; bulk stop and kill.
6. Quit progress for many projects.
7. Server mode: scan and proposals, read-only — the full list, with nothing
   applicable yet. Useful alone, and it is where the parsers get proven.
8. Server mode: apply, record and revert, unelevated proposals only.
9. Server mode: the elevated proposals.

Stage 3 is the one that delivers the stated goal; stages 1 and 2 exist to make
its numbers real. Stage 7 is deliberately separated from 8 so that the half that
can be verified on this machine lands before the half that cannot.
