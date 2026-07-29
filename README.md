# Project Host

A cross-platform **desktop application** for running and managing projects on a
machine you own — Discord bots, Node.js and Python applications, websites,
static sites, REST APIs and background workers, each in its own Docker
container.

It is a desktop program, not a web panel. Everything runs in one process on your
own machine: a Tauri 2 window on top of a Rust core that owns the database,
the project files and the Docker connection.

Projects keep running when the window is closed, because Docker's own daemon
keeps them running under their restart policy. What pauses with the application
is the part that watches them — scheduled backups, log capture and metrics.

Optionally, projects can also be watched and controlled from **Discord**: each
project gets a log channel and a control panel, with role-based permissions.

---

## Status: it launches

`cargo build -p project-host-desktop` produces `project-host-desktop.exe`, and
running it opens a window. On first launch it created
`C:\ProgramData\ProjectHost\` with its `data`, `config`, `logs`, `projects`,
`backups` and `tmp` directories, migrated the database to schema version 2 (28
tables) and recorded its instance in `agent_state`. That is observed, not
inferred.

The window shows the Docker status, the project list, and the update banner.
It is a first screen, not the thirty-item interface in the specification.

Verified on the development machine: **533 Rust tests, 31 TypeScript tests**,
clippy clean under `-D warnings`, `cargo fmt`, ESLint, Prettier, `tsc`, and the
generated TypeScript matching its Rust source.

**Not verified:** the clean-shutdown path — the launch above was force-killed,
so `last_clean_shutdown` stayed `0` and the window's own exit handler has never
run. Also unverified: anything needing a Docker daemon, a Linux host, or a
Discord bot token. See [Verification status](#verification-status).

**Cannot create a project yet.** There is no creation wizard and no command
behind one, so the project list is correct and empty.

### A note on the architecture

Earlier revisions split this into a background OS service plus a thin desktop
client that talked to it over authenticated HTTPS on loopback. That has been
removed in favour of a single process. Gone with it: the HTTP API, the TLS
identity, password login, session tokens, login rate limiting, and the
Windows Service and systemd adapters — roughly 2,400 lines whose only purpose
was protecting a network listener that no longer exists. Nothing authenticates
to the application now, because the process runs as you, on your machine, and
can already do anything you can.

The domain crates were untouched by that change, which is the point of them
having had no idea how they were being called.

| Phase                                                    | State                 |
| -------------------------------------------------------- | --------------------- |
| 1 — Architecture                                         | ✅ Complete           |
| 2 — Foundation: workspaces, database, contracts, logging | ✅ Complete           |
| 3 — Application core: lifecycle, Docker, platform        | ✅ Complete           |
| 4 — Project management: templates, lifecycle, limits     | ◑ Partial (see below) |
| 5 — Files and environment variables                      | ◑ Partial (see below) |
| 6 — Logs and metrics                                     | Not started           |
| 7 — Backups                                              | Not started           |
| 8 — Desktop UI                                           | ◑ Partial (see below) |
| 9 — Discord integration                                  | ◑ Partial (see below) |
| 10 — Installers and updates                              | ◑ Partial (see below) |
| 11 — Security review                                     | Not started           |
| 12 — Final verification                                  | Not started           |

Phase 9 was remote management. With the service gone there is no remote to
manage, and the slot is now the Discord integration — which is what a user
actually wanted remote management for.

---

## The design

Read in this order:

| Document                                                              | Covers                                                              |
| --------------------------------------------------------------------- | ------------------------------------------------------------------- |
| [development.md](docs/development.md)                                 | Bootstrap, commands, the contract pipeline, adding a migration      |
| [architecture.md](docs/architecture.md)                               | Components, trust boundaries, crate layout, data flow, recovery     |
| [platform-support.md](docs/platform-support.md)                       | Adapter traits, Windows and Linux specifics, capability degradation |
| [agent-desktop-communication.md](docs/agent-desktop-communication.md) | Tauri IPC, TLS transport, auth, streaming, contract generation      |
| [database-schema.md](docs/database-schema.md)                         | Full SQLite schema, constraints, transactions, retention            |
| [api-design.md](docs/api-design.md)                                   | Every route, error codes, pagination, idempotency                   |
| [security.md](docs/security.md)                                       | Threat model, attackers, controls, verification plan                |
| [docker.md](docs/docker.md)                                           | Container hardening, templates, networking, runtimes                |
| [offline-mode.md](docs/offline-mode.md)                               | Five connectivity states, Discord-bot reconnection                  |
| [remote-management.md](docs/remote-management.md)                     | Pairing, device keys, certificate pinning, addressing               |
| [installers.md](docs/installers.md)                                   | MSI, `.deb`, AppImage, upgrade and uninstall behaviour              |
| [testing-strategy.md](docs/testing-strategy.md)                       | Layers, host gating, what is skipped and why                        |
| [file-tree.md](docs/file-tree.md)                                     | Complete target layout                                              |

---

## Stack

| Layer     | Choice                                                            |
| --------- | ----------------------------------------------------------------- |
| Desktop   | Tauri 2, Rust, React 19, TypeScript, Vite, Tailwind               |
| Core      | Rust — tokio, bollard, sqlx                                       |
| Database  | SQLite (WAL) via SQLx; PostgreSQL is not required and not used    |
| Transport | Tauri IPC, in-process. No sockets, no ports, no certificates      |
| Secrets   | XChaCha20-Poly1305 at rest, for environment variables and the bot |
| Discord   | serenity (gateway not yet wired)                                  |
| Contracts | Rust types → JSON Schema → generated TypeScript and Zod           |
| Testing   | `cargo test`, Vitest, Playwright, Docker integration tests        |

Node.js is a build-time dependency of the frontend only. **The application does
not require Node.js on the host**, and nothing the product installs runs on the
host outside a container.

> The documents listed above still describe the client/server split and its
> HTTPS API. They are **out of date** as of the single-process change and have
> not yet been rewritten.

---

## Requirements

|         | Minimum                                                       |
| ------- | ------------------------------------------------------------- |
| Windows | 10 1809+ or 11, x64, WebView2, Docker Desktop                 |
| Linux   | Ubuntu 22.04+ / Debian 12+, x64, Docker Engine, WebKitGTK 4.1 |

Docker is required to run projects. The application installs and starts without
it, and explains what is missing rather than failing obscurely.

---

## Development environment

Verified on the machine this was designed on:

| Tool       | Version       | State       |
| ---------- | ------------- | ----------- |
| Node.js    | 24.16.0       | ✅          |
| pnpm       | 11.1.1        | ✅          |
| Rust       | 1.96.0 (msvc) | ✅ compiles |
| git        | 2.53.0        | ✅          |
| WebView2   | installed     | ✅          |
| Docker     | —             | ❌ absent   |
| WSL        | —             | ❌ absent   |
| Linux host | —             | ❌ none     |

### What "partial" means for Phases 4 and 5

Both phases split cleanly into work that can be verified on a machine with no
Docker daemon and work that cannot. The verifiable half is **done and tested**;
the rest is not started, and is not pretended to be.

| Phase 4                                                 | State                      |
| ------------------------------------------------------- | -------------------------- |
| Three approved templates with manifests and Dockerfiles | ✅ shipped                 |
| Manifest validation (versions, managers, health checks) | ✅ 51 tests                |
| Runtime detection: Node, Python, static                 | ✅ tested incl. edge cases |
| Slug / container / network / volume naming              | ✅ tested                  |
| Host port allocation and conflict detection             | ✅ tested                  |
| Container spec with full hardening + violation checker  | ✅ 31 tests                |
| Actually building an image or starting a container      | ❌ needs Docker            |
| Health checks observed, restart behaviour, reconciler   | ❌ needs Docker            |

| Phase 5                                                          | State          |
| ---------------------------------------------------------------- | -------------- |
| `SafePath`: traversal, absolute, UNC, reserved names, ADS        | ✅ 20 tests    |
| Symlink escape and the TOCTOU recheck                            | ✅ tested      |
| ZIP entry validation: Zip Slip, bombs, ratios, devices           | ✅ 14 tests    |
| Self-cleaning staging with atomic promote                        | ✅ tested      |
| Extraction driven through a real ZIP reader, staged and promoted | ✅ 12 tests    |
| File explorer operations: list, read, write, move, copy, search  | ✅ 27 tests    |
| Environment variable manager: validation, `.env` import/export   | ✅ 28 tests    |
| Project, environment and audit storage in SQLite                 | ✅ 33 tests    |
| Exposing all of the above through the desktop app                | ❌ not started |

| Phase 9 — Discord                                             | State          |
| ------------------------------------------------------------- | -------------- |
| Permission model: roles, users, blocks, lockout safety        | ✅ 16 tests    |
| Control panel and its `custom_id` encoding                    | ✅ 13 tests    |
| Channel naming: templates, sanitising, hostile names          | ✅ 15 tests    |
| Event routing, mention and code-block safety, secret masking  | ✅ 21 tests    |
| Discord ids that survive JavaScript                           | ✅ 7 tests     |
| Storage: bot token, servers, grants, channels, event settings | ✅ 27 tests    |
| Connecting to Discord and actually sending anything           | ❌ needs a bot |

### What Phase 9 does and does not reach

The whole rule set is built and tested; the gateway connection is not written.
Concretely, the crate answers "may this person press this button", "what does
this message say", and "is it safe to send" — and nothing yet holds a websocket
open to Discord, so no message has ever been sent.

What is worth stating plainly, because it is what tests were written for:

- **A project's log output cannot ping your server.** A Discord bot that logs
  the messages it receives would otherwise let any stranger trigger
  `@everyone`. Mentions are neutralised before sending.
- **A project's log output cannot escape its code block.** The fence is chosen
  longer than the longest backtick run in the content, so no output can close
  it and inject Markdown or links.
- **Secret environment variable values are masked** before a log line leaves
  for Discord — the case being a bot crashing on startup and printing its own
  token.
- **Authorisation happens when the button is pressed, not when it is drawn.**
  A panel sits in a channel for months; the person who eventually clicks it may
  have gained or lost roles since.
- **The bot token has nowhere to be stored unencrypted.** The table has a
  ciphertext column and a nonce column and nothing else.

Phases 6, 7, 8, 10, 11 and 12 are **not started**.

### What Phase 5 added, and what it does not yet reach

The security-critical half of the file and environment work is now real code
with real tests rather than validation helpers:

- **Archive import** opens an actual ZIP, applies the entry rules one at a time
  while extracting into a UUID-named staging directory, and renames it into
  place only on complete success. A Zip Slip entry or a bomb aborts the import
  and the staging directory removes itself — verified by asserting that neither
  the project nor the staging directory exists afterwards.
- **File operations** never accept a path; they accept a project root and a
  request string, and construct a `SafePath` themselves. Symbolic links (and
  Windows junctions, which `symlink_metadata` reports the same way) are listed
  so they can be deleted, and refused as the target of every other operation.
- **Environment variables** are validated, parsed from `.env` with the syntax
  people really have, and exported to `.env.example` with secret values omitted
  entirely rather than masked. A round-trip test parses the exported file back
  and compares values, because the quoting rules are where this goes wrong.
- **Storage** for projects spans three tables written in one transaction, and
  the environment-variable repository never sees an encryption key — a secret
  arrives as ciphertext and the schema `CHECK` refuses any row that pairs
  `is_secret = 1` with a plaintext value. That refusal is asserted directly
  against SQLite with raw SQL.

What is **not** built: none of this is reachable over the API yet. There are no
`/projects`, `/files` or `/env` routes, and the agent does not yet encrypt a
secret on its way to storage — the key management exists in `security`, the
storage exists in `database`, and the layer that joins them does not.

### Verification status

What Phase 3 shipped, split by whether it has actually been run:

| Behaviour                                                         | Status                               |
| ----------------------------------------------------------------- | ------------------------------------ |
| Agent starts as a standalone process and serves HTTPS             | ✅ run by hand and in tests          |
| Loopback-only bind (`netstat` shows `127.0.0.1:8787`)             | ✅ verified, LAN address refused     |
| TLS with a pinned self-signed certificate, stable across restarts | ✅ verified                          |
| Database creation, migrations, schema-version guard               | ✅ verified                          |
| Crash detection and startup recovery                              | ✅ verified (hard-killed, restarted) |
| Graceful shutdown: clean flag, WAL checkpoint, token removal      | ✅ verified in an integration test   |
| Administrator setup, bootstrap token, recovery codes              | ✅ verified over HTTPS               |
| Login, sessions, logout, revocation                               | ✅ verified over HTTPS               |
| Login rate limiting with exponential lockout                      | ✅ verified over HTTPS               |
| Docker detection reporting a real absence with a hint             | ✅ verified (no daemon on this host) |
| Operation locking, expiry, per-project isolation                  | ✅ verified in integration tests     |
| Docker **connected** behaviour — version, containers, events      | ❌ needs a daemon                    |
| Windows Service registration and SCM lifecycle                    | ❌ needs an elevated Windows session |
| systemd unit installation, `Type=notify` readiness                | ❌ needs an Ubuntu/Debian host       |
| Linux file modes and the dedicated service user                   | ❌ needs a Linux host                |

The unverified rows are implemented, not stubbed. Each is marked in its own
source file with why it could not be run here. No claim is made that they work.

**What that means for claims of completeness.** Roughly two thirds of the
planned test suite runs here: all Rust unit and integration tests, the container
_specification_ security assertions, SQLite migrations, contract generation, the
whole frontend, and the Windows platform suite. Anything requiring a Docker
daemon, systemd, a `.deb`, or a second machine cannot run, and will be reported
as **skipped and unverified** — never as passing. `docs/testing-strategy.md` §2
has the exact table.

Reaching all 30 acceptance criteria needs a Windows machine with Docker Desktop,
an Ubuntu machine with Docker Engine, and both on one network.

---

## Licence

Not yet chosen.
