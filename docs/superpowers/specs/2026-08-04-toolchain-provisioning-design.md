# Toolchain provisioning

Date: 2026-08-04
Status: designed, not implemented

Before a project starts, find out whether this machine has the language it needs.
If it does not, say exactly what is missing and exactly what would be installed,
ask once, and — only with permission — install it through the system's own
package manager under a single elevation that ends before any project code runs.

The machines this exists for are the fresh ones: a new server with no Node, no
Python, no build tools, and often no `winget` either. Today such a machine gets
`Toolchain::Missing { looked_for: ["python3", "python"] }` from
`crates/host-runner/src/probe.rs`. That value is honest and it is a dead end: it
names what was looked for and nothing about how to fix it. This design supplies
the missing half.

---

## 1. What this does not do, said out loud

**It does not run the application as administrator.** The request that started
this asked for the app itself to relaunch elevated. It does not, and the reason
is that this application hosts other people's code: remote repositories cloned
from a URL, a Monaco editor writing to disk, host-mode project processes. An
elevated window makes every one of those a machine-wide administrator. Elevation
is scoped to one short-lived process per install step, and is over before any
project code is executed.

**It does not install silently.** The offer names every package, every
prerequisite and every command before the first one runs. Declining is a normal
outcome that leaves the project unstarted with the reason shown, not an error.

**It does not carry its own copies of Node or Python.** The system package
manager owns the download, the signature, the version and the uninstall. This
crate owns a command line and an elevation prompt.

**It does not claim an install worked because a command exited zero.** Success is
re-probing the machine and finding the executable. See §6 — this is where the
subtle failure lives.

**It does not guess at `POLYGLOT`.** A project needing several toolchains cannot
be resolved from one enum value, and is reported as such rather than
half-provisioned.

---

## 2. Scope

Every project, every runtime mode. A project whose runtime is `NODEJS` gets Node
on the host whether it will run in a container or directly.

This was chosen deliberately and against the alternative of host-mode-only. It is
recorded here because the tradeoff is real and will look like a bug to someone
reading later: **for a Docker project the container image already carries the
runtime, so the host copy is software the project itself never uses.** The
benefit is a machine that is uniformly ready — a project can switch to host mode,
or be worked on in the editor, without a second provisioning event. The cost is
disk and installs on the host. The user made this call with the tradeoff stated.

---

## 3. Shape

Five units. The split exists so every decision is a pure function of a value and
can be tested on a machine with no Linux and no elevated session — which is the
machine this is written on.

| Unit                             | Responsibility                                            | Depends on                 |
| -------------------------------- | --------------------------------------------------------- | -------------------------- |
| `crates/toolchain/catalog.rs`    | What each runtime needs, per platform. Data only.         | `platform`                 |
| `crates/toolchain/plan.rs`       | missing set + `SystemSnapshot` → ordered steps or blocker | `catalog`, `platform`      |
| `crates/toolchain/blocker.rs`    | Why no plan is possible, in concrete values               | nothing                    |
| `crates/toolchain/execute.rs`    | Runs one step. The only impure file in the crate.         | `plan`                     |
| `app-core/src/toolchain_flow.rs` | The step machine the interface drives                     | `toolchain`, `host-runner` |

`crates/toolchain` depends on `project-host-platform` and nothing else — not
`docker-manager`, not `api-types` — for the reason the `compatibility` and
`host-runner` crate docs both already state: it should be possible to reason
about whether a machine can run a language without also holding the container
model, or the wire format, in mind.

Detection is not rebuilt. `host_runner::probe::{probe, candidates_for,
ExecutableResolver}` already answers "is it here", against a resolver the tests
supply, so no test result changes when someone installs Deno. This crate answers
only "and what would fix it".

---

## 4. The catalogue

```rust
pub struct ToolchainSpec {
    pub id: &'static str,          // stable; how a log line names a choice
    pub runtime: &'static str,     // "NODEJS", matching api_types::Runtime
    pub display_name: &'static str,
    pub winget_id: Option<&'static str>,        // "OpenJS.NodeJS.LTS"
    pub linux_packages: &'static [(PackageManager, &'static str)],
    pub prerequisites: &'static [&'static str], // ids into PREREQUISITES
}

/// A prerequisite is a package, not a runtime: nothing probes for it by
/// executable name and nothing starts a project with it. It carries the same
/// per-platform package identifiers and no probe candidates.
pub struct Prerequisite {
    pub id: &'static str,          // "git", "msvc-build-tools"
    pub display_name: &'static str,
    pub winget_id: Option<&'static str>,
    pub linux_packages: &'static [(PackageManager, &'static str)],
}
```

Requirements sit beside the artefact rather than inside the planner, so adding a
language is a data change that the property test in `plan.rs` covers
automatically — the same arrangement, and the same justification, as
`compatibility::catalog`.

Prerequisites are the packages a toolchain needs before it is usable rather than
merely present: `git` broadly, MSVC Build Tools where native modules compile,
the distribution's `-dev` packages on Linux. Without them the install reports
success and the first `npm install` with a native dependency fails.

---

## 5. The plan

```rust
pub struct Step {
    pub elevated: bool,
    pub program: String,
    pub args: Vec<String>,
    pub describes: String,  // shown to the user verbatim, before anything runs
}

pub enum Plan {
    Nothing,                       // STATIC, or everything already present
    Install { steps: Vec<Step> },
    Blocked(Blocker),
}
```

Steps are ordered in three layers:

1. **Bootstrap.** If `winget` is absent — the normal state of a fresh Windows
   Server image, and precisely the machine this feature exists for — the first
   step installs App Installer. Where that cannot be done, the outcome is
   `Blocker::NoPackageManager` naming Microsoft's download page. On Linux the
   manager comes from `LinuxInfo::package_manager` in the existing snapshot; its
   absence is the same blocker.
2. **Prerequisites**, then **the toolchain**. Both elevated.
3. **The project's own `install_command`** — `npm install`, `pip install -r
requirements.txt` — run **unelevated**, in the project directory, through the
   existing `host_runner::command` machinery. This is the last step and it is the
   first one that executes anything belonging to the project.

`plan` is pure: platform, package manager and probe results are all arguments.
Nothing is read from the environment, so all platforms' plans are checked on any
host. This is the rule `setup::handoff::plan` already follows, for the reason
`crates/setup/src/lib.rs` records: this project once shipped an application that
had never started for a non-root Linux user, because the only test that would
have caught it could not run on the development machine.

---

## 6. Elevation, and the exit codes that matter

One short-lived elevated process per elevated step. The application's own process
is never elevated.

- **Windows** — `ShellExecuteEx` with the `runas` verb, which is the UAC prompt.
  A dismissed prompt returns `ERROR_CANCELLED` (1223) and maps to
  `NotAuthorised`, never to `Failed`.
- **Linux** — `pkexec`, which prompts through the desktop and needs no terminal a
  double-clicked binary does not have. Its 126 and 127 map to `NotAuthorised`.

The distinction is the difference between "try again" and "something is wrong",
and `crates/setup/src/handoff.rs` already draws it — including a test asserting
the two messages differ. The same rule applies here.

### The stale-PATH trap

After `winget` installs Node, **the already-running application still holds the
`PATH` it inherited at launch.** A re-probe using that `PATH` finds nothing and
reports that a successful install failed.

So confirmation re-reads `PATH` from `HKLM\SYSTEM\CurrentControlSet\Control\
Session Manager\Environment` and `HKCU\Environment` rather than trusting the
inherited copy, and resolves against that. If the executable is still not found
after a step that exited zero, the message is _"installed — restart Panel
Platform to pick it up"_. Reporting a failure there would send the user to
reinstall software they already have.

---

## 7. Flow

Pressing Start scans, and blocks only when it must:

1. Resolve the project's `Runtime`. `STATIC` needs no toolchain and never sees
   this. `POLYGLOT` produces `Blocker::PolyglotUnresolvable`.
2. Probe the toolchain. Prerequisites are not probed — nothing resolves
   "MSVC Build Tools" to an executable — they are included in the plan whenever
   the toolchain that names them is being installed, and are a no-op for a
   package manager that finds them already present.
3. Nothing missing → start exactly as today. This is the overwhelmingly common
   path and it costs one `PATH` lookup per candidate.
4. Something missing → Start is replaced by an offer listing every step's
   `describes` string. Nothing has run yet.
5. Approved → run steps in order, emitting progress per step. Declined → the
   project stays stopped, showing what was missing and what would have fixed it.
6. Re-probe per §6. Confirmed → start the project.

Progress reaches the window as events, which requires `core:event:allow-listen`
in `apps/desktop/src-tauri/capabilities/default.json`. That grant already exists;
it is named here because its absence once silently swallowed every progress event
in the updater, and commands registered via `generate_handler` are not governed
by the ACL, which hides the gap behind everything else working.

---

## 8. Errors

`Blocker`, in the established style — every variant names concrete values,
because "installation failed" without the package and the exit code is the
failure mode this design exists to replace.

| Variant                    | Carries                         | Fixable |
| -------------------------- | ------------------------------- | ------- |
| `NoPackageManager`         | platform, where to get one      | yes     |
| `RuntimeUnsupported`       | runtime name                    | no      |
| `PolyglotUnresolvable`     | —                               | no      |
| `NotAuthorised`            | the step that was declined      | yes     |
| `StepFailed`               | program, exit code, last output | yes     |
| `StillMissingAfterInstall` | executable, packages installed  | yes     |
| `HostUnrecognised`         | —                               | no      |

`is_fixable()` drives whether the interface offers a retry, exactly as
`compatibility::Blocker::is_fixable` does. Offering a retry for an unsupported
runtime sends the user round a loop that cannot terminate.

---

## 9. Testing

Pure, and therefore complete:

- Every `Runtime` × Windows/Linux × all four package managers yields a plan.
- A property test: every runtime except `STATIC` and `POLYGLOT` produces a
  non-empty plan on every supported platform. A language added to the catalogue
  without a package id fails this test rather than failing when a user presses
  Start. This mirrors `probe.rs`'s existing
  `every_language_runtime_has_at_least_one_candidate`.
- Distinct ids across the catalogue, as `compatibility::catalog` asserts.
- Every prerequisite id in the catalogue resolves to a `PREREQUISITES` entry, and
  every prerequisite step is ordered before the toolchain that named it.
- Ordering: no unelevated project command precedes an elevated toolchain step.
- `NotAuthorised` and `StepFailed` render differently.
- Probing uses the existing fake `ExecutableResolver`.

**Unverifiable on this machine, and marked as such in the code:** this host has
no Linux and no elevated session, so the `pkexec` path and a real UAC prompt will
not have been executed. The `winget` path can be driven by hand here — winget
1.29.280 is present. Per this project's standing rule, untested behaviour is
described as untested and not claimed to work.

---

## 10. Out of scope

Uninstalling toolchains. Pinning or upgrading a version once a toolchain is
present — a machine with Node 18 where the project wants 22 is a separate
problem, and solving it badly here would mean silently replacing a runtime other
software on the machine depends on. Version managers (`nvm`, `pyenv`, `rustup`),
which would remove the elevation requirement but need a different mechanism per
language.
