# Installer Design

One installer per platform installs both components — the desktop application
and the background service — and leaves a machine where projects start at boot
with nobody logged in.

Two rules govern every path through these installers:

1. **Project data is never destroyed without an explicit answer.** Upgrades
   always preserve it; uninstall asks, defaults to keeping it, and only removes
   it when the user actively chooses removal.
2. **Every step is idempotent.** Repair and upgrade re-run the same steps.
   Installing an installed service, creating an existing directory, or adding an
   existing firewall rule all succeed quietly.

---

## 1. Outputs

| Platform | Artefacts                                                                   |
| -------- | --------------------------------------------------------------------------- |
| Windows  | `ProjectHost-<version>-x64.msi`, `ProjectHost-<version>-x64-setup.exe`      |
| Linux    | `project-host_<version>_amd64.deb`, `ProjectHost-<version>-x86_64.AppImage` |

The MSI is the primary Windows artefact — it supports proper upgrade, repair and
uninstall semantics. The NSIS `.exe` wraps it for users who expect a setup
program and handles the WebView2 bootstrap.

The `.deb` is the primary Linux artefact, because it is the only one that can
install a systemd service and a service user. **The AppImage ships the desktop
client only** — an AppImage cannot register a system service. This is stated in
the download page and in `docs/linux-installation.md` rather than discovered:
an AppImage user managing a remote agent is a perfectly good arrangement, but an
AppImage user expecting local hosting would be confused.

---

## 2. Windows

### Layout

```
C:\Program Files\Project Host\
    project-host.exe            desktop client
    project-host-agent.exe      service binary
    project-host-ctl.exe        admin CLI
    resources\, templates\

C:\ProgramData\ProjectHost\     data — preserved across upgrade
    data\  config\  logs\  projects\  backups\  tmp\
```

### Install sequence

1. Check Windows 10 1809+ / 11, x64, administrator rights.
2. Install or repair the **WebView2 Evergreen runtime** — the one dependency a
   Tauri app cannot supply itself.
3. Write program files.
4. Create `ProgramData\ProjectHost` and subdirectories.
5. Apply ACLs: Full Control to `SYSTEM` and `Administrators`, inheritance
   disabled, nothing for `Users`.
6. Register `ProjectHostAgent` — `LocalSystem`, Automatic (Delayed Start),
   failure actions 5s/15s/60s.
7. Start the service; wait for it to report `Running`. The agent creates its
   database, applies migrations and generates its TLS certificate on that first
   start.
8. Start Menu and optional desktop shortcuts.
9. Register the uninstall entry with publisher, version and icon.
10. **Firewall rule: only if the user ticked "allow access from my local
    network", which is unticked by default.**

Docker is not installed and not bundled. The installer detects Docker Desktop
and, if absent, finishes successfully while showing a page explaining that
Docker is required to run projects, with a link. An installer that refuses to
complete because an optional-at-install-time dependency is missing is a worse
experience than one that explains.

### Upgrade

Detect the installed version by upgrade code; stop the service; replace
binaries; leave `ProgramData` untouched; start the service, which applies any
pending migrations; preserve firewall and autostart choices. **Running project
containers are not stopped** — Docker keeps them up while the agent is briefly
down, and the reconciler adopts them on start.

### Repair

Re-runs file installation, ACLs and service registration without touching data.
The direct answer to a deleted binary or a disabled service.

### Uninstall

1. Stop and delete the service.
2. Remove program files, shortcuts, firewall rule, uninstall entry.
3. **Ask about data**, with a dialog that names the directory and states its
   size:

```
Remove project data?

  ○ Keep projects, backups and settings  (default)
     C:\ProgramData\ProjectHost  —  4.2 GB

  ○ Delete everything permanently
     This cannot be undone.
```

4. Project containers, volumes and images are **left alone** by default and
   removed only under "delete everything". Silently deleting a user's running
   services during an uninstall of the manager would be indefensible.

Unattended uninstall defaults to keeping data; removal requires an explicit
`REMOVE_DATA=1`.

---

## 3. Linux

### Layout

```
/usr/bin/project-host                 desktop client
/usr/lib/project-host/
    project-host-agent
    project-host-ctl
    templates/
/lib/systemd/system/project-host-agent.service
/usr/share/applications/project-host.desktop
/etc/project-host/                    config, TLS cert  (conffiles)
/var/lib/project-host/                data — preserved
/var/log/project-host/
```

### Maintainer scripts

**`preinst`** — verify the distribution is supported; on upgrade, stop the
service.

**`postinst`** —

1. Create system user and group `project-host` (`--system`, no login shell, no
   home). Skipped if it exists.
2. Create data directories with the modes in `docs/platform-support.md` §3.1.
3. Add `project-host` to the `docker` group if that group exists; if not, warn
   that Docker appears absent and continue.
4. `systemctl daemon-reload`, `enable`, `start`.
5. Wait for `sd_notify` readiness, then report status.
6. Print next steps: create the administrator with `project-host-ctl`, and open
   the desktop app.

**`prerm`** — stop and disable the service.

**`postrm`** —

- `remove`: leave `/var/lib/project-host`, `/etc/project-host` and the service
  user in place. This is what makes reinstall-after-remove keep working.
- `purge`: prompt via debconf, defaulting to keep. Only on an explicit answer
  are data directories and the service user removed. Non-interactive purge
  preserves data unless `PROJECT_HOST_PURGE_DATA=1` is set.

Dependencies: `libwebkit2gtk-4.1-0`, `libayatana-appindicator3-1`, `adduser`.
Docker is `Recommends`, not `Depends` — the agent is useful without it, and
forcing a particular Docker package on a user who already runs one is
presumptuous.

---

## 4. First-run experience

Both platforms converge here. After install, no administrator exists and the
agent is in setup mode, serving only `/api/v1/setup/*`.

1. The user opens the desktop app.
2. It reads the bootstrap file (administrator rights required) and connects.
3. Setup wizard: create the administrator, show recovery codes **once**, check
   Docker, offer LAN access (default off).
4. The dashboard appears, with real data and no projects.

`project-host-ctl create-admin` does the same thing from a terminal, which is
the path for a headless Linux install.

---

## 5. Signing

| Platform | Mechanism                                                                   |
| -------- | --------------------------------------------------------------------------- |
| Windows  | Authenticode over the MSI, the EXE and both binaries                        |
| Linux    | Detached signature over the `.deb`; a signed repository is a later addition |
| Updates  | Minisign signature verified before any file is written                      |

Unsigned Windows binaries trip SmartScreen and train users to click through
warnings, which undoes more security than most controls add. Until a certificate
exists, the release notes say plainly that builds are unsigned and how to verify
checksums, rather than leaving people to guess.

---

## 6. Testing

| Test                                               | Host                          |
| -------------------------------------------------- | ----------------------------- |
| Clean install → service running → project starts   | Windows 11 VM, Docker Desktop |
| Upgrade preserves data and running containers      | Windows VM                    |
| Repair restores a deleted binary                   | Windows VM                    |
| Uninstall keep-data, then reinstall finds projects | Windows VM                    |
| Uninstall delete-data removes everything           | Windows VM                    |
| `.deb` install → `systemctl status` active         | Ubuntu 22.04 + 24.04 VMs      |
| `apt upgrade` preserves data                       | Ubuntu VM                     |
| `apt remove` keeps data; `apt purge` prompts       | Ubuntu VM                     |
| Reboot → projects auto-start                       | both                          |
| AppImage runs the client only                      | Ubuntu VM                     |
| Install with Docker absent completes with guidance | both                          |

**None of these can run on the current development machine.** The MSI can be
_built_ here; installing it, and everything Linux, needs virtual machines that do
not yet exist. Acceptance criteria 1, 2, 5, 27 and 28 depend entirely on this
table, and remain unverified until it has been executed.
