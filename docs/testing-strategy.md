# Testing Strategy

The organising principle: **a test that cannot run must be reported as skipped,
never counted as passing.** This project targets two operating systems and
depends on Docker, and the development machine has neither Docker nor Linux. A
green summary that quietly omits two thirds of the suite would be worse than no
summary at all.

Every test therefore declares the host it needs, and the runner prints what it
skipped and why.

---

## 1. Layers

| Layer              | Tool                                   | What it covers                                               | Speed |
| ------------------ | -------------------------------------- | ------------------------------------------------------------ | ----- |
| Rust unit          | `cargo test`                           | Pure logic — path validation, specs, state machines, parsers | ms    |
| Rust integration   | `cargo test --test …`                  | Crate seams, real SQLite, fake platform adapters             | s     |
| Docker integration | `cargo test --features docker-tests`   | Real containers                                              | min   |
| Contract           | codegen + diff                         | Rust types match generated TS                                | s     |
| TS unit            | Vitest                                 | Frontend logic, hooks, formatting, state                     | ms    |
| Component          | Vitest + Testing Library               | React components with a stubbed IPC layer                    | s     |
| E2E                | Playwright (Tauri WebDriver)           | Critical UI flows against a real agent                       | min   |
| Platform           | `cargo test --features platform-tests` | Services, keychain, firewall, path escapes                   | s     |
| Installer          | scripted, in VMs                       | Install, upgrade, repair, uninstall                          | min   |

---

## 2. Host requirements

Rust tests are gated by feature flags; TypeScript tests by an environment check.
Neither silently passes when its dependency is absent.

```rust
#[cfg(feature = "docker-tests")]
#[tokio::test]
async fn container_starts_with_memory_limit() { … }
```

```
$ cargo test
running 412 tests
… ok

SKIPPED (feature "docker-tests" not enabled): 63 tests
  reason: no Docker daemon detected
SKIPPED (feature "platform-tests" not enabled): 18 tests
  reason: requires administrator rights
```

| Suite                            | This machine (Win, no Docker) | Windows + Docker | Linux + Docker | CI     |
| -------------------------------- | ----------------------------- | ---------------- | -------------- | ------ |
| Rust unit                        | ✅                            | ✅               | ✅             | ✅     |
| Rust integration (SQLite, fakes) | ✅                            | ✅               | ✅             | ✅     |
| Contract codegen diff            | ✅                            | ✅               | ✅             | ✅     |
| TS unit + component              | ✅                            | ✅               | ✅             | ✅     |
| Windows path-escape suite        | ✅                            | ✅               | —              | ✅     |
| Windows keychain                 | ✅                            | ✅               | —              | ✅     |
| Windows service lifecycle        | ✅ (admin)                    | ✅               | —              | ⚠️     |
| Linux path/permission suite      | ❌                            | ❌               | ✅             | ✅     |
| systemd lifecycle                | ❌                            | ❌               | ✅ (root)      | ⚠️     |
| Docker integration               | ❌                            | ✅               | ✅             | ✅     |
| E2E                              | ✅ (agent without Docker)     | ✅               | ✅             | ✅     |
| Installer                        | ❌                            | VM               | VM             | manual |

---

## 3. What gets tested where

### Rust unit — the largest and most valuable layer

Everything that can be pure, is. In particular:

- **`SafePath` corpus.** Traversal, absolute, UNC, drive-relative, encoded,
  reserved device names, trailing dot/space, NUL, deep nesting, case variants.
  This is a table test with the attack corpus as data, so adding a newly learned
  bypass is one line.
- **ZIP validation.** Slip entries, bombs, ratio violations, symlink and device
  entries, count and depth limits — over synthetic archives built in the test.
- **Container spec generation.** Assert every forbidden property is absent from
  every generated spec: no socket mount, no privileged, no host network, no
  added capabilities, non-root user, `no-new-privileges`, `memory_swap` equal to
  `memory_limit`. These catch the majority of container-security regressions
  **without Docker**, which matters given the development machine.
- **State machines.** Project status, backup operation state, connection
  lifecycle — including illegal transitions being refused.
- **Runtime detection.** `package.json` variants, lockfile precedence, missing
  start script, invalid JSON, unsupported versions.
- **Port allocation.** Pool exhaustion, conflict, release, range enforcement.
- **Redaction.** `Secret<T>` never prints; the log layer strips by key.

### Rust integration

Real SQLite in a temp directory, fake platform adapters, no Docker. Covers
migrations applying cleanly and being idempotent, foreign keys and `CHECK`
constraints actually rejecting bad rows (including the secret/plaintext
constraint), transaction rollback leaving no partial state, lock contention
being refused, and interrupted-operation recovery from a database deliberately
left in a transient state.

### Docker integration

Marked, gated, and honest about needing a daemon. Container lifecycle, limits
actually applied (start a container that allocates past its memory limit and
assert OOM), network isolation between two projects, log streaming including
detach on stop and reattach on restart, stats, event handling, build failure
surfaces, and reconciliation after the agent is killed and restarted.

### Contract tests

Regenerate TypeScript from Rust and `git diff --exit-code`. A Rust type change
without regeneration fails CI. This is the mechanism behind "documentation
matches the implementation" for the API surface.

### Frontend

Component tests stub the IPC layer, so the whole UI is testable without an
agent. Every screen is tested in its loading, empty, error, offline and
populated states — the specification lists those as requirements, and a
requirement without a test is an aspiration.

### E2E (Playwright over Tauri WebDriver)

Six flows, chosen because breaking them makes the product unusable: first-run
setup; create a project from a ZIP and start it; view live logs; edit an
environment variable and restart; create and restore a backup; connect to a
second agent. The first three run without Docker against an agent reporting
Docker unavailable, which verifies the failure presentation rather than skipping
it.

---

## 4. Fixtures

```
tests/fixtures/
  archives/       safe.zip, slip.zip, bomb.zip, symlink.zip, deep.zip, …
  projects/       node-pnpm/, node-no-lockfile/, node-invalid-json/,
                  python-requirements/, python-poetry/, static-site/,
                  discord-bot/
  databases/      interrupted-restore.db, stale-lock.db, older-schema.db
  certs/          agent cert + a deliberately different one for pin-mismatch
```

Malicious archives are generated by a script at test time rather than committed
as binaries — a committed ZIP bomb is a hazard in a repository, and a generator
is auditable in a way an opaque blob is not.

---

## 5. CI

GitHub Actions, three jobs:

| Job            | Runner         | Runs                                                                                         |
| -------------- | -------------- | -------------------------------------------------------------------------------------------- |
| `check`        | ubuntu-latest  | fmt, clippy `-D warnings`, ESLint, Prettier, tsc, contract diff, `cargo audit`, `pnpm audit` |
| `test-linux`   | ubuntu-latest  | Rust unit + integration + Docker (daemon present), TS unit, component, E2E                   |
| `test-windows` | windows-latest | Rust unit + integration, Windows platform suite, TS unit, Tauri build                        |

Docker integration runs on Linux CI, which is where a daemon is available
without extra setup. Installer tests are manual against VMs and are recorded in
the release checklist rather than pretended to be automated.

Coverage is measured (`cargo-llvm-cov`, Vitest v8) and reported, without a
blocking threshold. A percentage gate reliably produces tests written to move a
number. The review question is whether the security corpora and the state
machines are covered, which a percentage cannot answer.

---

## 6. Per-phase gates

No phase is called complete until, on the machine it is developed on:
`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `pnpm typecheck`,
`pnpm lint`, `pnpm test` and the contract diff all pass — and the skip list is
reported alongside, so what was _not_ run is visible at every step rather than
discovered at the end.

Phase 12 is where the skip list is finally emptied against real hardware:
a Windows machine with Docker Desktop, an Ubuntu machine with Docker Engine, and
the two of them on one network for the remote tests.
