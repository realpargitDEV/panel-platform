# Architecture

Project Host is a desktop application for running and managing containerised
projects on a machine you own. It is not a web panel with a window around it:
the process that runs your projects is an operating-system service that has no
knowledge of whether a user is logged in, and the window is an ordinary native
program that can be closed at any time without consequence.

This document defines the components, the boundaries between them, and the
rules that keep those boundaries honest. Every other Phase 1 document elaborates
one part of what is stated here.

---

## 1. Naming

One name is fixed here and used verbatim everywhere else — installers, service
registration, directory layout and container labels all derive from it.

| Thing                     | Value                                                    |
| ------------------------- | -------------------------------------------------------- |
| Product name              | Project Host                                             |
| Slug                      | `project-host`                                           |
| Desktop binary            | `project-host` (Windows: `project-host.exe`)             |
| Agent binary              | `project-host-agent` (Windows: `project-host-agent.exe`) |
| Admin CLI                 | `project-host-ctl`                                       |
| Linux service unit        | `project-host-agent.service`                             |
| Linux service user        | `project-host`                                           |
| Windows service key       | `ProjectHostAgent`                                       |
| Windows service display   | `Project Host Agent`                                     |
| Container label namespace | `io.projecthost.*`                                       |
| Default agent port        | `8787`                                                   |

The slug is an implementation detail of the filesystem and Docker. It is never
derived from anything the user types.

---

## 2. The two components

```
┌──────────────────────────────────────────────────────────────────┐
│  DESKTOP CLIENT — runs as the logged-in user, closable           │
│                                                                  │
│   ┌────────────────────────┐      ┌──────────────────────────┐   │
│   │  Webview (React + TS)  │◄────►│  Tauri Rust core         │   │
│   │  no network access     │ IPC  │  holds credentials       │   │
│   │  no tokens             │      │  speaks to agent         │   │
│   └────────────────────────┘      └───────────┬──────────────┘   │
└───────────────────────────────────────────────┼──────────────────┘
                                                │  HTTPS + WSS
                                                │  127.0.0.1:8787
                                                │  (or LAN, opt-in)
┌───────────────────────────────────────────────┼──────────────────┐
│  BACKGROUND AGENT — OS service, no user session required         │
│                                               ▼                  │
│   ┌──────────────────────────────────────────────────────────┐   │
│   │  Local API  (axum: REST + WebSocket)                     │   │
│   ├──────────────────────────────────────────────────────────┤   │
│   │  agent-core — lifecycle, scheduler, reconciler, locks    │   │
│   ├───────────┬───────────┬───────────┬──────────┬───────────┤   │
│   │ project-  │ docker-   │ file-     │ backup-  │ metrics   │   │
│   │ manager   │ manager   │ manager   │ manager  │           │   │
│   ├───────────┴───────────┴───────────┴──────────┴───────────┤   │
│   │  security  │  database (SQLite/SQLx)  │  platform        │   │
│   └──────────────────────┬───────────────────────┬───────────┘   │
└──────────────────────────┼───────────────────────┼───────────────┘
                           │                       │
                    ┌──────▼──────┐         ┌──────▼──────┐
                    │   Docker    │         │  Host OS    │
                    │   Engine    │         │  services,  │
                    │             │         │  keychain,  │
                    └──────┬──────┘         │  metrics    │
                           │                └─────────────┘
              ┌────────────┼────────────┐
        ┌─────▼────┐ ┌─────▼────┐ ┌─────▼────┐
        │ project  │ │ project  │ │ project  │   one container each,
        │ container│ │ container│ │ container│   isolated network,
        └──────────┘ └──────────┘ └──────────┘   no socket access
```

### 2.1 Background agent

A single long-lived Rust binary registered as a native OS service. It owns
everything stateful: the database, project files, backups, log files, the
connection to Docker, and the lifecycle of every project container.

It must survive the things that kill ordinary programs:

- It starts at boot, before and independent of any user login.
- It keeps running when the desktop client exits, crashes, or was never opened.
- It is restarted by the OS service manager if it dies (`Restart=on-failure` /
  Windows SCM failure actions), and reconciles state on the way back up.
- It never requires an interactive session, a console window, or a tray icon.
- It never requires the internet.

The agent is the **only** component that talks to Docker. This is a hard
boundary, not a convention — see §5.

### 2.2 Desktop client

A Tauri 2 application: a native window hosting a React webview, plus a Rust
core compiled into the same binary. It holds no authoritative state. Everything
it displays is fetched from an agent; everything it changes is a request to an
agent. Closing it stops nothing.

The split inside the client matters and is enforced:

- **The webview renders. It does not authenticate and does not reach the
  network.** Its Content-Security-Policy forbids outbound connections. It cannot
  see the agent's address, session token, or device key.
- **The Tauri Rust core is the client's trust boundary.** It stores credentials
  in the OS keychain, opens the TLS connection, pins the agent certificate, and
  exposes a narrow set of typed IPC commands upward to the webview.

This is why the requirement "no authentication tokens in localStorage" is
satisfied structurally rather than by discipline: there is no code path by which
a token can reach the webview.

---

## 3. Why two processes, and why this split

A single process would be simpler, and it would fail the central requirement.
Projects must keep running when the window closes and after a reboot with
nobody logged in. That forces a service. Once there is a service, the window
becomes a client, and the only remaining question is where the trust boundary
sits. Putting it between the webview and the client's Rust core — rather than
between the client and the agent alone — means a compromised or buggy frontend
cannot reach Docker, the filesystem, or the network on its own.

The cost is a serialisation boundary and a contract to keep in sync. §4 and
`docs/agent-desktop-communication.md` address that.

---

## 4. Crate and package layout

Rust does the work; TypeScript renders it. Business rules live in Rust and are
not reimplemented in TypeScript.

### Rust crates (`crates/`)

| Crate             | Responsibility                                                                     | Depends on                                   |
| ----------------- | ---------------------------------------------------------------------------------- | -------------------------------------------- |
| `agent-core`      | Service lifecycle, task scheduler, reconciler, operation locks, event bus          | all below                                    |
| `docker-manager`  | Docker API client (bollard), container specs, image builds, log streams, stats     | `security`, `platform`                       |
| `project-manager` | Project lifecycle, runtime detection, template rendering, port allocation          | `docker-manager`, `database`, `file-manager` |
| `backup-manager`  | Backup create/restore/verify, retention, interrupted-operation recovery            | `database`, `file-manager`                   |
| `file-manager`    | Sandboxed filesystem ops, ZIP import/export, path canonicalisation                 | `security`, `platform`                       |
| `metrics`         | Host and container sampling, ring buffers, rollups                                 | `platform`, `docker-manager`                 |
| `security`        | Argon2id, session tokens, Ed25519 device keys, secret encryption, validators       | `platform`                                   |
| `platform`        | OS adapters — services, paths, keychain, notifications, firewall, Docker discovery | —                                            |
| `database`        | SQLx pool, migrations, typed queries, transactions                                 | —                                            |
| `api-types`       | Request/response types, error codes, JSON Schema emission                          | `database` (enums only)                      |

`api-types` is deliberately thin and dependency-light: it is the crate that
generates the TypeScript contract, so it must not drag the world into codegen.

### TypeScript packages (`packages/`)

| Package         | Responsibility                                                                     |
| --------------- | ---------------------------------------------------------------------------------- |
| `shared-types`  | **Generated.** TS interfaces emitted from `api-types`. Never hand-edited.          |
| `api-contracts` | **Generated.** Zod schemas + typed client method signatures.                       |
| `validation`    | Hand-written UI-only validation (form ergonomics), composed with generated schemas |
| `ui`            | Design-system components — buttons, panels, tables, terminal, dialogs              |
| `config`        | Shared ESLint, TS, Prettier, Vitest configuration                                  |

### Applications (`apps/`)

| App       | Contents                                                                                          |
| --------- | ------------------------------------------------------------------------------------------------- |
| `agent`   | Thin binary: parses args, resolves platform, starts `agent-core`. Service entry points live here. |
| `desktop` | Tauri app — `src/` React frontend, `src-tauri/` Rust core                                         |

---

## 5. Boundaries that are enforced, not merely intended

These five rules are the architecture. Everything else is arrangement.

1. **Only the agent talks to Docker.** The desktop client has no Docker client
   dependency compiled into it. Not "does not call Docker" — cannot.
2. **The webview holds no secrets and opens no sockets.** Enforced by CSP and by
   the IPC surface, which returns rendered data and never credentials.
3. **No project container can reach Docker.** The socket is never mounted, the
   Docker API is never proxied, and no user-supplied string reaches a Docker
   call unvalidated. See `docs/docker.md`.
4. **User input never becomes a path or an identifier.** Project names are
   display strings. Filesystem directories and container names derive from a
   server-generated UUID. See `docs/security.md`.
5. **The agent binds to loopback unless explicitly told otherwise.** LAN
   exposure is a deliberate, audited setting, never a default.

---

## 6. Data flow: creating and starting a project

The path below exercises every component and is the reference for how work
moves through the system.

```
 1. Webview          wizard collects config, validates with generated Zod schema
 2. Tauri core       IPC command → typed HTTPS request, attaches session token
 3. Agent API        authenticates, validates against the same JSON Schema,
                     assigns request_id, writes AuditLog entry
 4. project-manager  generates UUID → derives slug, dir, container name, volume
 5. file-manager     creates project dir; if ZIP: streams extraction with
                     Zip-Slip and bomb protections into a UUID temp dir
 6. project-manager  detects runtime (package.json / requirements.txt /
                     index.html), selects approved template, renders Dockerfile
 7. database         transaction: Project + ProjectRuntime + ProjectPort +
                     EnvironmentVariable rows, Deployment row PENDING
 8. docker-manager   builds image, streams build log over WebSocket to client
 9. docker-manager   creates network, volumes, container from a structured spec
                     (never a shell string), applies limits and hardening
10. docker-manager   starts container; Deployment → SUCCEEDED
11. metrics          begins sampling the container
12. agent-core       emits event → WebSocket → client updates without polling
```

Failure at any step rolls back: the deployment is marked FAILED with an error
code, partial files are removed, allocated ports released, and the audit entry
records the outcome. Nothing is left half-created.

---

## 7. Concurrency, locking and recovery

The agent is async (Tokio). Long operations — builds, backups, restores, ZIP
imports — run as cancellable background tasks with progress reported over the
event bus.

**Operation locks.** Each project has a lock with a declared held-operation.
Start, stop, rebuild, restore, delete and import take it. This is what makes
"do not restore a project while it is running" and "no two simultaneous
restores" true by construction rather than by timing luck.

**Recovery on startup.** The agent cannot assume it shut down cleanly. On every
boot it:

1. Opens the database, applies pending migrations.
2. Marks any operation still in a transient state (`CREATING`, `RESTORING`,
   `BUILDING`) as `INTERRUPTED`, with a recovery action recorded.
3. Removes orphaned temp directories and partial archives.
4. Reconciles Docker: enumerates containers labelled `io.projecthost.*`,
   compares actual state against `desired_state`, and converges — starting what
   should run, adopting what already runs, flagging what vanished.
5. Releases stale port allocations for containers that no longer exist.

Step 4 is what satisfies "projects restart after a reboot" alongside Docker's
own `unless-stopped` policy. The two are complementary: Docker restores
containers it knows about, the reconciler catches everything else — a container
removed while the agent was down, a project whose image is gone, a port now
taken by something else.

---

## 8. Connectivity model

The system distinguishes five independent states and never conflates them. A
single "offline" flag would be wrong: an unplugged network cable and a stopped
Docker daemon are different problems with different remedies.

| State              | Meaning                                   | Consequence                             |
| ------------------ | ----------------------------------------- | --------------------------------------- |
| Agent reachable    | Client ↔ agent transport is up            | UI is live vs. showing last-known state |
| Docker reachable   | Agent ↔ Docker daemon is up               | Containers manageable vs. read-only     |
| LAN available      | Host has a local network address          | Remote clients can connect              |
| Internet available | Outbound route to the public internet     | Discord bots can reach Discord          |
| External service   | A given project's dependency is reachable | Per-project health display              |

All five are surfaced separately in the UI. Full treatment in
`docs/offline-mode.md`.

---

## 9. What Phase 1 fixes and what it leaves open

**Fixed by this document:** the two-component split, the trust boundaries, the
crate layout, the direction of contract generation (Rust → TypeScript), SQLite
as the store, and the five enforced boundaries in §5.

**Deliberately deferred:** Git-based deployment, PostgreSQL, cloud backups,
multi-user roles, automatic updates, and any template not in
`docker/templates/`. Version one is single-administrator and local-first. The
schema in `docs/database-schema.md` carries a `User` table with a role column so
that multi-user is a later addition rather than a migration of everything.

---

## 10. Verification status of this design

This machine has Node 24.16, pnpm 11.1.1, Rust 1.96 (msvc, verified compiling),
git 2.53 and WebView2. It has **no Docker, no WSL, and no Linux host.**

| Area                                                 | Can be verified here | Needs other hardware |
| ---------------------------------------------------- | -------------------- | -------------------- |
| Rust build, unit tests, schema codegen               | ✅                   |                      |
| SQLite migrations and queries                        | ✅                   |                      |
| Tauri desktop build (Windows)                        | ✅                   |                      |
| React UI, Vitest, Playwright                         | ✅                   |                      |
| Windows Service install/start/stop                   | ✅ (admin rights)    |                      |
| Docker builds, container runtime, stats, log streams | ❌                   | Docker Desktop       |
| systemd unit, `.deb`, Linux paths, UFW               | ❌                   | Ubuntu/Debian host   |
| Windows↔Linux remote pairing                         | ❌                   | both machines        |
| Reboot recovery end-to-end                           | ❌                   | Docker + reboot      |

No claim of "works" will be made for anything in the right-hand column until it
runs on real hardware. Phase 12 exists for that, and
`docs/testing-strategy.md` states which tests are gated on which host.
