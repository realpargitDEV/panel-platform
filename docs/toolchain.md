# Toolchains

A project needs a language. A fresh server has none. This is what happens
between those two facts.

Design: `docs/superpowers/specs/2026-08-04-toolchain-provisioning-design.md`.

## What happens when you press Start

1. `toolchain_readiness` reads the project's runtime and probes this machine for
   the executable it needs — `node`, `python3`, `go`, and so on.
2. Present: the project starts, as before. This is the common path and costs one
   `PATH` lookup per candidate name.
3. Missing: a dialog lists **every command** that would run, marking the ones
   that need administrator. Nothing has happened yet.
4. Approved: each step runs, elevated ones as their own short-lived process,
   reporting progress on `toolchain://progress`.
5. The executable is looked for again — see _Stale PATH_ below — and the project
   starts.

Declining leaves the project stopped. It is an answer, not an error, and no
failure is reported for it.

## Where it is wired

Every path that can start a project goes through `useToolchainGate`:

| Place                            | Control                       |
| -------------------------------- | ----------------------------- |
| `pages/Projects.tsx`             | Start/Restart, cards and rows |
| `pages/ProjectDetail.tsx`        | Start/Restart buttons         |
| `App.tsx`                        | Command palette; after create |
| `workspace/ProjectWorkspace.tsx` | The workspace run actions     |

The gate is applied at each page's single action helper rather than at each
button, so a new button inherits it. Stop and Kill are never gated: neither
needs the toolchain.

## What gets installed

Three layers, in order, from `crates/toolchain/src/catalog.rs`:

1. **The package manager**, if absent. On Windows that means registering App
   Installer, which supplies `winget` — fresh Windows Server images routinely
   lack it. On Linux the manager comes from the existing system scan.
2. **Prerequisites, then the toolchain.** Prerequisites are what a toolchain
   needs to be _usable_ rather than merely present: `git`, and a C/C++ compiler
   for native modules. Without them the install reports success and the first
   `npm install` with a native dependency fails.
3. **The project's own dependencies** — its `install_command` — **unelevated**.
   Elevation is over before anything belonging to the project runs.

## Elevation

The application never runs as administrator. Each elevated step is one
short-lived process.

- **Windows** — `Start-Process -Verb RunAs`, which raises the UAC prompt.
  `ShellExecuteEx` would be the direct route, but the workspace sets
  `unsafe_code = "forbid"` and `forbid` cannot be locally overridden.
- **Linux** — `pkexec`, which prompts through the desktop and needs no terminal.

A dismissed prompt is reported as `NotAuthorised`, never as a failed install.
On Windows this takes deliberate work: dismissing the prompt makes
`Start-Process` throw, so the script catches it and exits 1223
(`ERROR_CANCELLED`) rather than a generic 1.

The application is not elevated because it hosts other people's code — remote
repositories, an editor writing to disk, host-mode processes. An elevated window
would make all of that machine-wide administrator.

## Stale PATH

**The trap this feature is most likely to hit.** A process inherits `PATH` at
launch and never sees a change to it. The moment after `winget` installs Node,
a probe using the inherited copy finds nothing.

So confirmation rebuilds the search path from `HKLM`/`HKCU` `Environment` rather
than trusting what the process holds. If the executable is still not found after
a step that exited zero, the message is _"installed — restart Panel Platform to
pick it up"_. Reporting a failure there would send the user to reinstall
software they already have.

## Limits

- **`POLYGLOT`** declares several languages and cannot be resolved to one
  toolchain. It is refused with that reason rather than half-provisioned.
- **`STATIC`** needs nothing and never sees the offer.
- **Bun and Deno** are in no Linux distribution's default repository. On Linux
  they are refused with the vendor's URL rather than an invented package name.
- **Versions are not handled.** This installs a language that is _absent_. A
  machine with Node 18 and a project wanting Node 22 is not fixed by it.
- **Uninstalling is not handled.** The package manager owns that.
- **Docker projects get a host toolchain too.** The container image already
  carries the runtime, so that copy is unused by the project itself. This is
  deliberate — see §2 of the design.

## Unverified

The machine this was developed on has **no Linux and no elevated session**.
Neither the `pkexec` path nor a real UAC prompt has ever been executed. The
decisions around them are tested; the elevation itself is not. The App Installer
bootstrap is written from Microsoft's documented behaviour rather than from
having watched it work.

Per this project's standing rule, none of that is described as working until
someone has watched it.
