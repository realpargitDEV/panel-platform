# Platform Support Plan

Windows 10/11 and Ubuntu/Debian are supported from one codebase. The rule that
makes that sustainable: **operating-system differences live behind traits in the
`platform` crate, and nowhere else.** No `#[cfg(windows)]` appears in
`project-manager`, `backup-manager`, `docker-manager` or the API layer. If a
business rule needs to know which OS it is on, the design is wrong — it needs a
capability from an adapter instead.

---

## 1. Adapter traits

Seven traits, each small enough to fake in tests. `PlatformAdapter` is the
composition root handed to `agent-core` at startup.

```rust
pub trait PlatformAdapter: Send + Sync + 'static {
    fn paths(&self)         -> &dyn PathProvider;
    fn services(&self)      -> &dyn ServiceManager;
    fn docker(&self)        -> &dyn DockerProvider;
    fn metrics(&self)       -> &dyn MetricsProvider;
    fn secrets(&self)       -> &dyn SecureStorageProvider;
    fn notifications(&self) -> &dyn NotificationProvider;
    fn firewall(&self)      -> &dyn FirewallProvider;
    fn info(&self)          -> PlatformInfo;
}
```

### PathProvider

Every path the system uses comes from here. Nothing concatenates strings.

```rust
pub trait PathProvider: Send + Sync {
    fn data_dir(&self)     -> &Path;   // database, agent state
    fn config_dir(&self)   -> &Path;   // agent.toml, TLS cert
    fn log_dir(&self)      -> &Path;
    fn projects_dir(&self) -> &Path;
    fn backups_dir(&self)  -> &Path;
    fn temp_dir(&self)     -> &Path;   // same volume as projects: atomic renames
    fn project_dir(&self, id: ProjectId) -> PathBuf;
    fn ensure_all(&self) -> Result<(), PlatformError>;  // idempotent, sets modes
}
```

`temp_dir()` sitting on the same filesystem as `projects_dir()` is not cosmetic:
ZIP extraction and restore both stage into temp and then rename into place. A
cross-device rename is not atomic, and the whole partial-write protection
depends on that atomicity.

### ServiceManager

```rust
pub trait ServiceManager: Send + Sync {
    fn install(&self, spec: &ServiceSpec) -> Result<(), PlatformError>;
    fn uninstall(&self) -> Result<(), PlatformError>;
    fn start(&self) -> Result<(), PlatformError>;
    fn stop(&self) -> Result<(), PlatformError>;
    fn status(&self) -> Result<ServiceStatus, PlatformError>;
    fn set_autostart(&self, enabled: bool) -> Result<(), PlatformError>;
    fn logs_hint(&self) -> String;  // "journalctl -u …" / "Event Viewer → …"
}
```

All six operations are idempotent. Installing an installed service succeeds.
Stopping a stopped service succeeds. Installers get retried and repaired; an
adapter that throws on "already done" turns a repair install into a failure.

### DockerProvider

```rust
pub trait DockerProvider: Send + Sync {
    fn discover(&self) -> Result<DockerEndpoint, PlatformError>;
    fn connect(&self, ep: &DockerEndpoint) -> Result<Docker, PlatformError>;
    fn install_hint(&self) -> DockerInstallHint;   // shown when discovery fails
}
```

Discovery is ordered and explicit per platform (§2.3, §3.3). When it fails the
UI shows actionable instructions, never a raw connection error.

### MetricsProvider, SecureStorageProvider, NotificationProvider, FirewallProvider

- **MetricsProvider** — host CPU, RAM, swap, disk, disk I/O, network I/O,
  uptime, process count, CPU temperature where the OS exposes it. Backed by
  `sysinfo`, with platform-specific temperature and disk-I/O paths.
- **SecureStorageProvider** — stores the master encryption key and remote
  credentials. `store`, `retrieve`, `delete`, plus `backend()` reporting which
  mechanism is actually in use so the UI can tell the truth about it.
- **NotificationProvider** — native desktop notifications. Only meaningful in
  the desktop client; the agent's implementation is a no-op that records the
  notification in the database for the client to collect.
- **FirewallProvider** — `add_lan_rule`, `remove_lan_rule`, `rule_status`.
  Never invoked without explicit user consent, always audited.

---

## 2. Windows

Target: Windows 10 1809+ and Windows 11, x86_64. ARM64 is a later target.

### 2.1 Directories

| Purpose              | Path                                   |
| -------------------- | -------------------------------------- |
| Data, database       | `C:\ProgramData\ProjectHost\data\`     |
| Config, TLS cert     | `C:\ProgramData\ProjectHost\config\`   |
| Logs                 | `C:\ProgramData\ProjectHost\logs\`     |
| Projects             | `C:\ProgramData\ProjectHost\projects\` |
| Backups              | `C:\ProgramData\ProjectHost\backups\`  |
| Temp                 | `C:\ProgramData\ProjectHost\tmp\`      |
| Per-user UI settings | `%APPDATA%\ProjectHost\`               |

`ProgramData` is correct precisely because the service runs without a logged-in
user: a per-user directory would be unreadable in that state.

**ACLs**, applied by the installer and re-asserted on agent start:
`ProgramData\ProjectHost` grants Full Control to `SYSTEM` and `Administrators`,
and nothing to `Users`. Inheritance is disabled so a permissive parent cannot
widen it. The database, TLS key and agent config are additionally checked at
startup; the agent logs a loud warning and refuses LAN binding if they are
world-readable.

### 2.2 Service

Registered via the `windows-service` crate as `ProjectHostAgent`, running as
`LocalSystem`, start type `Automatic (Delayed Start)`. Delayed start avoids
racing Docker Desktop during boot.

- Implements the SCM control handler: `Stop`, `Shutdown`, `Interrogate`.
- Reports `StartPending` with a growing hint while migrations and reconciliation
  run, then `Running`. The SCM kills services that go quiet during startup.
- Failure actions: restart after 5s, then 15s, then 60s; reset counter daily.
- Graceful stop: refuse new requests, cancel background tasks, checkpoint the
  SQLite WAL, close the pool. Project containers are **not** stopped — the whole
  point is that they outlive the agent.
- Logs to `logs\agent.log` (rotated) and mirrors warnings and errors to the
  Windows Event Log.

`LocalSystem` is required to talk to the Docker named pipe and manage service
state. It is a broad privilege, which is exactly why the agent runs no
user-supplied code in-process — everything user-supplied executes inside a
container as a non-root user.

### 2.3 Docker discovery, in order

1. Named pipe `\\.\pipe\docker_engine` — Docker Desktop and Engine both expose it.
2. `DOCKER_HOST` environment variable, if set.
3. TCP `127.0.0.1:2375`, only if explicitly enabled in agent config.

If all fail, the UI states whether Docker Desktop is installed-but-stopped
(detected via the service/registry) or absent, and links to the install guide.
WSL is never invoked directly; Docker Desktop's use of WSL2 internally is its
own business.

### 2.4 Paths, permissions and the traps

- Paths are `PathBuf` throughout, never strings. Canonicalisation uses
  `dunce::canonicalize` to avoid `\\?\` prefixes leaking into display and
  comparison.
- Reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
  `LPT1`–`LPT9`), trailing dots and trailing spaces are rejected in the file
  manager and in ZIP entries — Windows resolves them in surprising ways.
- Case-insensitivity means containment checks compare case-folded canonical
  paths. A check that is case-sensitive on Windows is a bypass.
- **Junctions and symlinks** are the Windows equivalent of symlink escape and
  get the same treatment: after canonicalisation, the resolved path must still
  be inside the project root, re-checked at open time (see `docs/security.md`
  on TOCTOU).
- Alternate Data Streams (`file.txt:evil`) are rejected: any `:` past the drive
  prefix is invalid.
- Long paths: the manifest enables long-path awareness, but generated paths stay
  short by construction — UUID directories, not nested user-supplied names.

### 2.5 Firewall

`Windows Defender Firewall` rules are added only when the user enables LAN
access, via `netsh advfirewall` with a named rule
`Project Host Agent (LAN)` scoped to the agent port and to private profiles
only. Uninstall removes it. Public-profile rules are never created.

---

## 3. Linux

Target: Ubuntu 22.04+ and Debian 12+, x86_64. Other distributions may work; only
these are tested and claimed.

### 3.1 Directories

| Purpose              | Path                              | Owner                       | Mode   |
| -------------------- | --------------------------------- | --------------------------- | ------ |
| Data, database       | `/var/lib/project-host/`          | `project-host:project-host` | `0750` |
| Projects             | `/var/lib/project-host/projects/` | `project-host:project-host` | `0750` |
| Backups              | `/var/lib/project-host/backups/`  | `project-host:project-host` | `0750` |
| Temp                 | `/var/lib/project-host/tmp/`      | `project-host:project-host` | `0700` |
| Config               | `/etc/project-host/`              | `root:project-host`         | `0750` |
| TLS key              | `/etc/project-host/agent.key`     | `root:project-host`         | `0640` |
| Logs                 | `/var/log/project-host/`          | `project-host:project-host` | `0750` |
| Per-user UI settings | `~/.config/project-host/`         | user                        | `0700` |

### 3.2 Service user and unit

A dedicated system user `project-host` (no login shell, no home) owns the data.
It is added to the `docker` group — which is equivalent to root on the host, and
is stated plainly in `docs/security.md` rather than glossed over. The mitigation
is that the agent is the only thing running as that user and it executes no
user-supplied code in-process.

```ini
[Unit]
Description=Project Host Agent
Documentation=https://github.com/…/docs/architecture.md
After=network-online.target docker.service
Wants=network-online.target
# Not Requires=: the agent must start and report Docker-unavailable
# rather than fail, so the UI can explain the problem.

[Service]
Type=notify
NotifyAccess=main
User=project-host
Group=project-host
SupplementaryGroups=docker
ExecStart=/usr/lib/project-host/project-host-agent --service
Restart=on-failure
RestartSec=5s
WatchdogSec=60s
TimeoutStopSec=30s

StateDirectory=project-host
LogsDirectory=project-host
ConfigurationDirectory=project-host

NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
ReadWritePaths=/var/lib/project-host /var/log/project-host
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM

[Install]
WantedBy=multi-user.target
```

`Type=notify` with `sd_notify` means systemd learns the agent is ready only
after migrations and reconciliation finish, so dependent units and the UI do not
race a half-started agent. `WatchdogSec` turns a wedged agent into a restart
instead of a silent hang; the main loop pings it.

### 3.3 Docker discovery, in order

1. `/var/run/docker.sock`
2. `DOCKER_HOST`
3. `$XDG_RUNTIME_DIR/docker.sock` (rootless Docker)

Rootless Docker is detected and reported, since it changes what resource limits
and port bindings are possible. Version one supports it in a degraded mode and
says so in the UI rather than failing obscurely.

### 3.4 Paths and permissions

- `openat2` with `RESOLVE_BENEATH` where the kernel supports it (5.6+), giving
  kernel-enforced containment for file operations. Fallback is canonicalise-then-
  verify-then-open with an `O_NOFOLLOW` final component.
- `umask` is set explicitly at startup; created files are `0640`, directories
  `0750`.
- Project bind-mount sources are always canonical absolute paths under
  `projects_dir()`, verified before every container create.

### 3.5 Firewall

UFW rules added only on explicit LAN opt-in:
`ufw allow from <lan-cidr> to any port 8787 proto tcp comment 'Project Host Agent'`.
The CIDR is derived from the host's own interface, never `any`. If UFW is not
installed or inactive, the UI says so instead of pretending a rule was applied.

---

## 4. Desktop client per platform

| Concern            | Windows                            | Linux                                                                |
| ------------------ | ---------------------------------- | -------------------------------------------------------------------- |
| Webview            | WebView2 (Evergreen)               | WebKitGTK 2.36+                                                      |
| Runtime dependency | WebView2 bootstrapper in installer | `libwebkit2gtk-4.1-0` via `.deb` deps                                |
| Tray               | Shell_NotifyIcon via Tauri         | StatusNotifierItem; falls back gracefully on desktops without a tray |
| Notifications      | Toast                              | `org.freedesktop.Notifications`                                      |
| Autostart          | `HKCU\…\Run`                       | XDG autostart `.desktop`                                             |
| Keychain           | Credential Manager                 | Secret Service (libsecret)                                           |

Where a Linux desktop has no Secret Service — a headless or minimal install —
the fallback is an encrypted key file at `~/.config/project-host/keys` with mode
`0600`. The UI reports which backend is in use. Silently degrading from a
keychain to a file would be a lie about the security posture.

---

## 5. Capability degradation

Not every platform can do everything. The design names the gaps rather than
hiding them; `PlatformInfo` carries a capability set the UI reads.

| Capability                | Windows         | Linux                      | If unavailable                 |
| ------------------------- | --------------- | -------------------------- | ------------------------------ |
| CPU temperature           | often absent    | usually present            | hide the tile                  |
| Per-container disk I/O    | limited         | cgroup v2                  | show "unavailable"             |
| Storage quota per project | not enforceable | cgroup/quota where present | soft accounting + warning      |
| Read-only root filesystem | supported       | supported                  | —                              |
| Linux capability dropping | n/a             | full                       | Windows uses its own isolation |
| Rootless Docker           | n/a             | detected                   | degraded mode, explained       |

"Storage controls where technically possible" from the specification resolves
to: enforce where the platform allows, otherwise measure and warn, and never
claim a limit is enforced when it is not.

---

## 6. Testing the adapters

Each trait gets an in-memory fake, so the 90% of logic that is platform-neutral
runs on any developer machine. Real adapters get integration tests marked with
required-host attributes and skipped with a printed reason elsewhere — never
silently.

| Test group                                        | Runs where           |
| ------------------------------------------------- | -------------------- |
| Fake adapters, all business logic                 | any host             |
| Windows service lifecycle                         | Windows, admin       |
| systemd unit lifecycle                            | Linux, root          |
| Docker container lifecycle                        | any host with Docker |
| Path-escape suites (symlink, junction, traversal) | matching OS          |
| Keychain round-trip                               | matching OS          |

On the current development machine — Windows, no Docker, no WSL — the first,
second, and Windows path-escape and keychain suites run. The rest are skipped
with an explicit "requires Docker" or "requires Linux" message, and are not
counted as passing.
