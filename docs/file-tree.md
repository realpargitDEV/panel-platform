# Complete File Tree

The target layout for the finished project. Phase 1 has created only the `docs/`
entries marked ✅; everything else is the plan that Phases 2–10 build. Nothing
below is a placeholder for something undecided — each path has an owner
documented elsewhere in `docs/`.

```
project-host/
├── Cargo.toml                       # Cargo workspace root
├── Cargo.lock
├── package.json                     # pnpm workspace root
├── pnpm-workspace.yaml
├── pnpm-lock.yaml
├── rust-toolchain.toml              # pinned Rust version
├── .editorconfig  .gitignore  .gitattributes
├── .prettierrc  .prettierignore  eslint.config.js
├── rustfmt.toml  clippy.toml
├── vitest.workspace.ts
├── README.md
│
├── apps/
│   ├── agent/                                  # the background service binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                         # arg parsing, mode selection
│   │       ├── service/
│   │       │   ├── mod.rs
│   │       │   ├── windows.rs                  # SCM entry point, control handler
│   │       │   └── linux.rs                    # sd_notify, watchdog, signals
│   │       ├── console.rs                      # foreground mode for development
│   │       └── shutdown.rs                     # graceful stop, WAL checkpoint
│   │
│   └── desktop/                                # Tauri 2 application
│       ├── package.json  vite.config.ts  tailwind.config.ts  index.html
│       ├── src/                                # React frontend
│       │   ├── main.tsx  App.tsx  router.tsx
│       │   ├── ipc/                            # typed wrappers over Tauri invoke
│       │   │   ├── client.ts  commands.ts  events.ts  errors.ts
│       │   ├── features/
│       │   │   ├── setup/                      # first-run administrator wizard
│       │   │   ├── auth/                       # unlock screen, session state
│       │   │   ├── dashboard/                  # agent, Docker, host, counters
│       │   │   ├── projects/
│       │   │   │   ├── list/  detail/  wizard/ # wizard = 6 steps, one file each
│       │   │   │   └── actions/                # start/stop/rebuild/delete dialogs
│       │   │   ├── logs/                       # terminal panel, follow, search
│       │   │   ├── files/                      # explorer, editor, upload
│       │   │   ├── env/                        # variable manager
│       │   │   ├── backups/                    # create, restore, verify
│       │   │   ├── history/                    # deployments, container events
│       │   │   ├── audit/                      # audit log viewer
│       │   │   ├── system/                     # host info, Docker status
│       │   │   ├── servers/                    # remote connection manager
│       │   │   ├── notifications/
│       │   │   └── settings/
│       │   ├── layout/                         # shell, sidebar, tabs, panels
│       │   │   ├── AppShell.tsx  Sidebar.tsx  TabBar.tsx
│       │   │   ├── ResizablePanels.tsx  DockableTerminal.tsx
│       │   │   ├── CommandPalette.tsx  StatusBar.tsx
│       │   ├── providers/                      # theme, connection, query, toast
│       │   ├── hooks/                          # useStream, useProject, useShortcuts
│       │   ├── lib/                            # formatting, keyboard map, storage
│       │   ├── styles/
│       │   └── test/                           # setup, IPC stubs, fixtures
│       │
│       └── src-tauri/                          # the desktop client's Rust core
│           ├── Cargo.toml  tauri.conf.json  build.rs
│           ├── icons/
│           └── src/
│               ├── main.rs  lib.rs
│               ├── commands/                   # one module per feature area
│               │   ├── auth.rs  projects.rs  files.rs  env.rs
│               │   ├── backups.rs  logs.rs  metrics.rs  servers.rs
│               │   ├── settings.rs  audit.rs  system.rs
│               ├── agent_client/               # HTTPS + WSS, pinning, retry
│               │   ├── mod.rs  transport.rs  pinning.rs  stream.rs  errors.rs
│               ├── connections/                # saved servers, active sessions
│               ├── keychain.rs                 # device keys, session tokens
│               ├── local_db.rs                 # client-side cache + settings
│               ├── tray.rs  notifications.rs  autostart.rs
│               └── state.rs
│
├── crates/
│   ├── agent-core/                             # orchestration; owns nothing OS-specific
│   │   └── src/
│   │       ├── lib.rs  agent.rs                # startup, readiness, shutdown
│   │       ├── api/                            # axum router
│   │       │   ├── mod.rs  router.rs
│   │       │   ├── middleware/                 # auth, rate limit, request id,
│   │       │   │                               # idempotency, audit, errors
│   │       │   ├── routes/                     # one module per API section
│   │       │   └── stream/                     # WebSocket, topics, subscriptions
│   │       ├── auth/                           # sessions, challenges, lockout
│   │       ├── reconciler.rs                   # desired vs actual convergence
│   │       ├── recovery.rs                     # interrupted operations, stale locks
│   │       ├── locks.rs                        # project operation locks
│   │       ├── scheduler.rs                    # retention, sampling, health
│   │       ├── events.rs                       # internal bus
│   │       ├── operations.rs                   # long-running task registry
│   │       └── config.rs                       # agent.toml + env, validated
│   │
│   ├── api-types/                              # the contract source of truth
│   │   └── src/
│   │       ├── lib.rs  requests.rs  responses.rs  enums.rs
│   │       ├── errors.rs                       # stable error codes
│   │       └── bin/emit-schema.rs              # → contracts/
│   │
│   ├── database/
│   │   ├── migrations/0001_initial.sql
│   │   └── src/
│   │       ├── lib.rs  pool.rs                 # WAL, foreign_keys per connection
│   │       ├── models/                         # one module per table
│   │       ├── queries/                        # typed query functions
│   │       └── retention.rs
│   │
│   ├── docker-manager/
│   │   └── src/
│   │       ├── lib.rs  client.rs               # bollard, reconnect
│   │       ├── container_spec.rs               # the typed spec; hardening lives here
│   │       ├── security.rs                     # forbidden-property assertions
│   │       ├── images.rs  build.rs             # render + build, streamed
│   │       ├── networks.rs  volumes.rs
│   │       ├── logs.rs                         # ref-counted followers
│   │       ├── stats.rs  events.rs  health.rs
│   │       └── names.rs                        # generated identifiers only
│   │
│   ├── project-manager/
│   │   └── src/
│   │       ├── lib.rs  lifecycle.rs            # create/start/stop/rebuild/delete
│   │       ├── detection/                      # nodejs.rs  python.rs  static.rs
│   │       ├── templates.rs                    # manifest validation + rendering
│   │       ├── ports.rs                        # allocation, conflicts, release
│   │       ├── env.rs                          # variables, .env import/export
│   │       ├── import.rs  export.rs  duplicate.rs
│   │       └── archive.rs
│   │
│   ├── file-manager/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── safe_path.rs                    # the only way to name a file
│   │       ├── operations.rs                   # atomic write, move, copy, delete
│   │       ├── listing.rs  search.rs
│   │       ├── zip_import.rs                   # streaming, limits, slip protection
│   │       ├── zip_export.rs
│   │       └── detect.rs                       # binary vs text, size limits
│   │
│   ├── backup-manager/
│   │   └── src/
│   │       ├── lib.rs  create.rs  restore.rs
│   │       ├── verify.rs                       # checksum + archive integrity
│   │       ├── retention.rs  export.rs  import.rs
│   │       └── recovery.rs                     # interrupted operation handling
│   │
│   ├── metrics/
│   │   └── src/
│   │       ├── lib.rs  host.rs                 # sysinfo
│   │       ├── container.rs                    # docker stats stream
│   │       ├── buffers.rs                      # bounded ring buffers
│   │       └── rollup.rs                       # downsampling for history
│   │
│   ├── security/
│   │   └── src/
│   │       ├── lib.rs  password.rs             # Argon2id
│   │       ├── tokens.rs                       # opaque sessions, hashing
│   │       ├── device_keys.rs                  # Ed25519 pairing + challenge
│   │       ├── encryption.rs                   # XChaCha20-Poly1305
│   │       ├── secret.rs                       # Secret<T>: redacting, zeroising
│   │       ├── tls.rs                          # self-signed cert, fingerprints
│   │       ├── rate_limit.rs
│   │       └── validation.rs                   # shared validators
│   │
│   └── platform/
│       └── src/
│           ├── lib.rs  traits.rs               # the seven adapter traits
│           ├── windows/                        # service, paths, credman, netsh,
│           │                                   # docker pipe, metrics
│           ├── linux/                          # systemd, paths, secret service,
│           │                                   # ufw, docker socket, metrics
│           └── fake/                           # in-memory adapters for tests
│
├── packages/
│   ├── shared-types/    src/generated.ts       # GENERATED — do not edit
│   ├── api-contracts/   src/generated.ts       # GENERATED — Zod + client types
│   ├── validation/      src/                   # hand-written UI-only validation
│   ├── ui/              src/components/        # design system
│   │                    src/tokens/            # colour, spacing, typography
│   └── config/          eslint/ tsconfig/ vitest/
│
├── contracts/                                  # GENERATED
│   ├── openapi.json
│   └── schemas/*.json
│
├── docker/
│   └── templates/
│       ├── nodejs/       Dockerfile.hbs  manifest.toml  README.md
│       ├── python/       Dockerfile.hbs  manifest.toml  README.md
│       └── static-site/  Dockerfile.hbs  manifest.toml  nginx.conf  README.md
│
├── installers/
│   ├── windows/          product.wxs  service.wxs  ui/  build-msi.ps1
│   │                     nsis/installer.nsi
│   └── linux/            debian/{control,postinst,prerm,postrm,preinst,conffiles}
│                         systemd/project-host-agent.service
│                         desktop/project-host.desktop
│                         appimage/AppRun  build-deb.sh  build-appimage.sh
│
├── scripts/
│   ├── setup.sh / setup.ps1                    # developer bootstrap
│   ├── generate-contracts.sh                   # Rust → TS, used by CI diff
│   ├── dev-agent.sh / dev-agent.ps1            # foreground agent, dev database
│   ├── dev-desktop.sh / dev-desktop.ps1
│   ├── test-all.sh / test-all.ps1              # prints the skip list
│   ├── build-desktop.sh / build-desktop.ps1
│   ├── build-agent.sh / build-agent.ps1
│   ├── build-installers.sh / build-installers.ps1
│   ├── install-service.ps1 / uninstall-service.ps1
│   ├── install-service.sh  / uninstall-service.sh
│   ├── create-admin.sh / create-admin.ps1
│   ├── backup-system.sh / restore-system.sh
│   ├── health-check.sh / health-check.ps1
│   └── clean.sh / clean.ps1
│
├── tests/
│   ├── e2e/              specs/  fixtures/  playwright.config.ts
│   ├── integration/      docker/  database/  service/
│   └── fixtures/         archives/  projects/  databases/  certs/
│
├── docs/
│   ├── architecture.md                    ✅ Phase 1
│   ├── platform-support.md                ✅ Phase 1
│   ├── agent-desktop-communication.md     ✅ Phase 1
│   ├── database-schema.md                 ✅ Phase 1
│   ├── api-design.md                      ✅ Phase 1
│   ├── security.md                        ✅ Phase 1  (threat model)
│   ├── docker.md                          ✅ Phase 1  (container security model)
│   ├── offline-mode.md                    ✅ Phase 1
│   ├── remote-management.md               ✅ Phase 1
│   ├── installers.md                      ✅ Phase 1
│   ├── testing-strategy.md                ✅ Phase 1
│   ├── file-tree.md                       ✅ Phase 1  (this file)
│   ├── api-reference.md                      GENERATED, Phase 2
│   ├── development.md                        Phase 2
│   ├── windows-installation.md               Phase 10
│   ├── linux-installation.md                 Phase 10
│   ├── deployment.md                         Phase 10
│   ├── updating.md                           Phase 10
│   ├── uninstall.md                          Phase 10
│   ├── backups.md                            Phase 7
│   ├── project-templates.md                  Phase 4
│   ├── troubleshooting.md                    Phase 12
│   └── security-review.md                    Phase 11 (verified vs unverified)
│
└── .github/workflows/
    ├── check.yml  test-linux.yml  test-windows.yml  release.yml
```

---

## Notes on the shape

**`crates/` is where the product lives.** `apps/agent` is a thin binary that
selects a service mode and starts `agent-core`; almost nothing of consequence is
in it. That keeps the service-hosting logic — the least testable part — as small
as possible.

**`platform/fake/` is not a test helper afterthought.** It is what allows the
majority of the system to be developed and tested on a machine without Docker or
Linux, which is the situation this project actually starts from.

**Generated directories are committed.** `contracts/`, `packages/shared-types`
and `packages/api-contracts` are checked in so a fresh clone builds without
running Rust codegen first; CI regenerates and fails on any diff.

**`docker/templates/` is an allow-list, not a starting point.** Adding a
template is a code change with review, not a user action.
