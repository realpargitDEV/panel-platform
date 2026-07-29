# Development

## Requirements

| Tool    | Version        | Needed for                                         |
| ------- | -------------- | -------------------------------------------------- |
| Node.js | 22+            | Frontend build and tests                           |
| pnpm    | 11+            | JavaScript workspace                               |
| Rust    | stable (1.82+) | Everything in `crates/` and `apps/agent`           |
| git     | any            | —                                                  |
| Docker  | 24+            | Running projects, and the Docker integration tests |

Docker is **optional for development**. The workspace builds and the large
majority of tests run without it; the suites that need a daemon are skipped with
a printed reason rather than silently passing.

Windows additionally needs the MSVC build tools (for the Rust linker) and the
WebView2 runtime (for the desktop app, from Phase 8). Linux needs
`libwebkit2gtk-4.1-dev` and `build-essential`.

## Bootstrap

```bash
./scripts/setup.sh          # or  .\scripts\setup.ps1
```

Checks the toolchain, installs JavaScript dependencies, generates the API
contracts from Rust, and builds the workspace. Safe to re-run.

## Layout

```
apps/
  agent/         the runnable background service binary
crates/          Rust: the agent and its libraries
  api-types/     wire types — the contract source of truth
  database/      SQLite, migrations, constraints, locks, recovery
  platform/      OS differences, isolated behind adapters
  security/      Argon2id, tokens, encryption, TLS, rate limiting
  docker-manager/ daemon detection and status (lifecycle in Phase 4)
  agent-core/    config, logging, state, auth, HTTP API, lifecycle
packages/        TypeScript
  shared-types/  GENERATED interfaces
  api-contracts/ GENERATED Zod schemas
  validation/    hand-written form validation
contracts/       GENERATED JSON Schema
```

`docs/file-tree.md` has the full target layout, including the parts later
phases add.

## The contract

Rust is the single source of truth. TypeScript is generated:

```
crates/api-types/src/*.rs
  → schemars JSON Schema
    → contracts/schema.json
    → packages/shared-types/src/generated.ts     (interfaces)
    → packages/api-contracts/src/generated.ts    (Zod schemas)
```

```bash
pnpm contracts          # regenerate
pnpm contracts:check    # fail if the committed output is stale — what CI runs
```

**After changing anything in `crates/api-types`, run `pnpm contracts`.** The
generated files are committed so a fresh clone builds without a Rust toolchain
step, and CI fails on any diff, so a type change that skips regeneration cannot
merge.

Generated files carry a do-not-edit banner and are excluded from ESLint and
Prettier. Formatting them would fight the generator and make the check fail on
whitespace.

## Everyday commands

```bash
pnpm contracts          # Rust types → TypeScript
cargo test --workspace  # Rust tests
pnpm test               # TypeScript tests (Vitest)
pnpm typecheck          # tsc across every package
pnpm lint               # ESLint
pnpm format             # Prettier, write
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

./scripts/test-all.sh   # everything above, plus a report of what was skipped
pnpm verify             # the same gate, without the skip report
```

## What clippy enforces

The workspace denies `unwrap`, `expect`, `panic!`, `todo!` and `unimplemented!`
in non-test code. The agent runs as a privileged service: a panic in a request
handler stops managed projects. Tests opt out per crate — a panic in a test is
a failed test, which is the point of one.

Handle errors with `Result` and the crate's typed error enum. If a value truly
cannot be absent, restructure so the type says so rather than reaching for
`expect`.

## Development mode versus production

Production is the default in code: `AgentConfig::default()` is production, with
JSON logging and loopback binding. Development must be asked for explicitly:

```bash
PROJECT_HOST_MODE=development
PROJECT_HOST_LOG_LEVEL=debug
PROJECT_HOST_DOCKER_ENABLED=false
```

Two guards make the specification's "production must never accidentally run
with development configuration" structural rather than a convention:

- Mode comes from an explicit value, never from `cfg!(debug_assertions)`. A
  release build cannot fall into development mode.
- `validate()` refuses trace logging in production, refuses a privileged port,
  and refuses a non-loopback bind unless `lan_enabled` is also true. Setting an
  address alone is not enough to put the management API on the network.

An unknown value for `PROJECT_HOST_MODE` is an error rather than a default —
defaulting to production would be safe and to development would not, and
refusing avoids having to be right.

## Adding a migration

1. Add `crates/database/migrations/000N_description.sql`. Forward only.
2. If it changes an enum's `CHECK` list, update the matching Rust enum in
   `crates/api-types/src/enums.rs`. The parity test in
   `crates/database/tests/schema.rs` compares the two and fails on drift.
3. Bump `SUPPORTED_SCHEMA_VERSION` in `crates/database/src/pool.rs` when the
   change is not backward compatible.
4. `cargo test -p project-host-database`.

## Testing on a machine without Docker

This is the normal case during development, and the split is deliberate:

| Runs anywhere                          | Needs Docker or Linux              |
| -------------------------------------- | ---------------------------------- |
| Contract generation and codegen        | Image builds, container lifecycle  |
| SQLite migrations and constraints      | Log streaming, stats, events       |
| Container **spec** security assertions | Network isolation between projects |
| Path, ZIP and redaction logic          | systemd, `.deb`, UFW               |
| Frontend unit and component tests      | Installers, reboot recovery        |

The left column catches most container-security regressions, because those bugs
live in spec construction rather than in Docker. But no claim that a container
_runs_ correctly is made until it has run. `docs/testing-strategy.md` §2 has the
full matrix.

## Running the agent

The agent is a standalone process. Nothing else needs to be running, and it
keeps running when you close whatever started it.

```bash
cargo build -p project-host-agent

# A throwaway data directory keeps development away from a real install.
export PROJECT_HOST_DATA_DIR=/tmp/project-host-dev
export PROJECT_HOST_CONFIG=$PROJECT_HOST_DATA_DIR/agent.toml
export PROJECT_HOST_MODE=development

./target/debug/project-host-agent
```

PowerShell:

```powershell
cargo build -p project-host-agent

$env:PROJECT_HOST_DATA_DIR = "$env:TEMP\project-host-dev"
$env:PROJECT_HOST_CONFIG   = "$env:PROJECT_HOST_DATA_DIR\agent.toml"
$env:PROJECT_HOST_MODE     = "development"

.\target\debug\project-host-agent.exe
```

It prints its address and certificate fingerprint, then serves until Ctrl-C:

```text
Project Host agent 0.1.0
  listening on https://127.0.0.1:8787
  certificate  92:d7:5c:40:b9:ed:…
  press Ctrl-C to stop
```

### Talking to it

The certificate is self-signed and pinned by fingerprint, so `curl` needs `-k`.
That is expected — there is no CA, by design.

```bash
# Liveness. No authentication, no configuration disclosed.
curl -sk https://127.0.0.1:8787/api/v1/server/health

# Has an administrator been created yet?
curl -sk https://127.0.0.1:8787/api/v1/setup/status

# Create the first administrator. Requires the rotating bootstrap token from
# $PROJECT_HOST_DATA_DIR/config/local-bootstrap.json — a file only an
# administrator can read.
TOKEN=$(node -p "require('$PROJECT_HOST_DATA_DIR/config/local-bootstrap.json').local_token")
curl -sk -X POST https://127.0.0.1:8787/api/v1/setup/administrator \
  -H 'Content-Type: application/json' \
  -d "{\"local_token\":\"$TOKEN\",\"email\":\"you@example.com\",
       \"display_name\":\"You\",\"password\":\"choose-a-long-password\"}"

# Exchange the bootstrap token for a session, or log in with the password.
SESSION=$(curl -sk -X POST https://127.0.0.1:8787/api/v1/auth/local-token \
  -H 'Content-Type: application/json' \
  -d "{\"local_token\":\"$TOKEN\"}" | node -p "JSON.parse(require('fs').readFileSync(0)).data.token")

curl -sk -H "Authorization: Bearer $SESSION" https://127.0.0.1:8787/api/v1/server/info
curl -sk -H "Authorization: Bearer $SESSION" https://127.0.0.1:8787/api/v1/docker/status
curl -sk -H "Authorization: Bearer $SESSION" https://127.0.0.1:8787/api/v1/server/health/detail
```

The recovery codes in the setup response are shown once. No route returns them
again, because storing them retrievably would defeat them.

### Routes available in Phase 3

| Method | Path                           | Auth            |
| ------ | ------------------------------ | --------------- |
| GET    | `/api/v1/server/health`        | none            |
| GET    | `/api/v1/setup/status`         | none            |
| POST   | `/api/v1/setup/administrator`  | bootstrap token |
| POST   | `/api/v1/auth/local-token`     | bootstrap token |
| POST   | `/api/v1/auth/login`           | none            |
| GET    | `/api/v1/auth/session`         | session         |
| POST   | `/api/v1/auth/logout`          | session         |
| POST   | `/api/v1/auth/logout-all`      | session         |
| GET    | `/api/v1/server/info`          | session         |
| GET    | `/api/v1/server/health/detail` | session         |
| GET    | `/api/v1/server/connectivity`  | session         |
| GET    | `/api/v1/docker/status`        | session         |

Project, file, backup and streaming routes arrive in Phases 4–7.

### Without Docker

The agent starts anyway and reports Docker as unavailable with an install hint.
That is deliberate: an agent that refused to start without a daemon could never
explain why the daemon was missing. Everything except container operations works.

### Service mode

`--service` is the entry point a service manager calls. On Linux it also sends
`READY=1` to systemd once migrations and recovery have finished.

**Neither service path has been run under a real service manager.** Registration
helpers exist in `crates/platform/src/service.rs` (systemd unit rendering,
`sc.exe` arguments) and their pure parts are unit-tested, but installing and
starting a real service needs an Ubuntu host or an elevated Windows session.
Both are marked untested in the source.

## Current state

Phase 3 is complete: the agent runs, serves an authenticated HTTPS API on
loopback, and recovers from a crash. It cannot yet run projects — the container
lifecycle, templates, files, logs, metrics and backups are Phases 4 to 7, and
the desktop client is Phase 8.
