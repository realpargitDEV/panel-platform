# Installers and Releases

One installer per platform installs a single desktop application. There is no
background service, no service user, and no admin CLI: the single-process
rewrite removed all three, and this document describes what is actually built.

Two rules govern every path through these installers:

1. **Project data is never destroyed without an explicit answer.** Upgrades
   always preserve it; uninstall leaves it in place.
2. **Every step is idempotent.** Repair and upgrade re-run the same steps.
   Installing over an install, or creating an existing directory, succeeds
   quietly.

---

## 1. Outputs

Built by `.github/workflows/release.yml` on a `v*` tag, and attached to a draft
GitHub release.

| Platform | Artefacts                                                            |
| -------- | -------------------------------------------------------------------- |
| Windows  | `Panel.Platform_<version>_x64-setup.exe` (NSIS), `..._x64_en-US.msi` |
| Linux    | `Panel.Platform_<version>_amd64.deb`, `..._amd64.AppImage`           |
| Updates  | `latest.json`, plus a `.sig` beside each updater artefact            |
| Both     | `SHA256SUMS.txt`                                                     |

The NSIS `.exe` is the primary Windows artefact: it handles the WebView2
bootstrap, which is the one dependency a Tauri application cannot supply
itself. The MSI exists for deployment tooling that requires one.

On Linux the `.deb` is the primary artefact. The AppImage is the fallback for
distributions that are not Debian-derived, and is the **only** Linux artefact
that can update itself.

`rpm` is deliberately not built. Nothing has tested it, and shipping an
untested package format is worse than not shipping it.

---

## 2. Release process

1. Bump the version in all four places — `Cargo.toml`, `package.json`,
   `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`.
   `scripts/check-version.sh` verifies they agree and runs in CI.
2. Tag `vX.Y.Z` and push it.
3. The workflow builds each platform, signs the updater artefacts, and opens a
   **draft** release.
4. Check the artefacts, then publish. Nothing reaches a user, and no client
   sees `latest.json`, until that press.

The draft step is the only gate between a green build and everyone's updater.

---

## 3. Windows

### Layout

```
C:\Users\<user>\AppData\Local\Panel Platform\    application
C:\ProgramData\ProjectHost\                      data — preserved across upgrade
    data\  config\  logs\  projects\  backups\  tmp\
```

The application installs per-user; the data directory is machine-wide because
projects outlive the account that created them.

### Install sequence

1. Install or repair the **WebView2 Evergreen runtime**.
2. Write program files, Start Menu entry, optional desktop shortcut.
3. Register the uninstall entry with publisher, version and icon.

The data directory is **not** created by the installer. The application creates
it and applies migrations on first launch, so that a user who has never opened
the application has nothing on disk to clean up.

Docker is not installed and not bundled. The application detects it at runtime
and explains its absence rather than refusing to start.

### Upgrade

Replace program files; leave `ProgramData` untouched; the application applies
pending migrations on next launch. **Running project containers are not
stopped** — Docker keeps them up, and the reconciler adopts them on start.

### Uninstall

Removes program files, shortcuts and the uninstall entry. **`ProgramData` is
left in place**, along with every project, backup and container. Removing a
user's running services during an uninstall of the manager would be
indefensible; the directory is named in the final uninstaller page so it can be
deleted by hand.

---

## 4. Linux

### Layout

```
/usr/bin/panel-platform                       binary
/usr/share/applications/panel-platform.desktop
~/.local/share/project-host/                  data — preserved
```

Data is per-user under `$XDG_DATA_HOME`, because the application runs as the
user rather than as a system service.

### Dependencies

`libwebkit2gtk-4.1-0` and `libgtk-3-0`, declared in the `.deb`. Docker is not a
dependency at all — not even `Recommends`. Forcing a particular Docker package
on a user who already runs one is presumptuous, and the application is useful
without it.

### AppImage

Ships the same application with no package manager involved. `chmod +x` and
run. This is the artefact the updater can replace in place.

---

## 5. Updates

`crates/updater` decides **whether** to offer a release: it rejects a version
older than or equal to the installed one, requires `https` from a host on
`ALLOWED_HOSTS`, and requires a signature. `tauri-plugin-updater` performs the
install, verifying against a minisign public key compiled into the binary from
`tauri.conf.json` — never one supplied by the feed.

The private key lives only in the `TAURI_SIGNING_PRIVATE_KEY` repository
secret and in the maintainer's offline copy. **Losing it means no existing
install can ever be updated again**, because clients trust exactly one key.

| Artefact | Can self-update            |
| -------- | -------------------------- |
| NSIS/MSI | Yes — installer takes over |
| AppImage | Yes — replaced in place    |
| `.deb`   | **No** — dpkg owns it      |

Nothing installs without the user pressing the button.

---

## 6. Signing

| Platform | Mechanism                           | State          |
| -------- | ----------------------------------- | -------------- |
| Windows  | Authenticode over the installers    | **Not signed** |
| Linux    | Detached signature over the `.deb`  | **Not signed** |
| Updates  | Minisign, verified before any write | Signed         |

Unsigned Windows binaries trip SmartScreen and train users to click through
warnings. Until a certificate exists, the release notes and the README say
plainly that builds are unsigned and how to verify checksums, rather than
leaving people to guess.

---

## 7. Testing

| Test                                               | Host                          |
| -------------------------------------------------- | ----------------------------- |
| Clean install → application opens → project starts | Windows 11 VM, Docker Desktop |
| Upgrade preserves data and running containers      | Windows VM                    |
| Uninstall leaves `ProgramData` intact              | Windows VM                    |
| `.deb` install → application opens                 | Ubuntu 22.04 + 24.04 VMs      |
| `apt upgrade` preserves data                       | Ubuntu VM                     |
| AppImage runs and self-updates                     | Ubuntu VM                     |
| Update offered, downloaded, verified, installed    | both, two published releases  |
| Install with Docker absent opens with guidance     | both                          |

**None of these have been run.** CI proves the artefacts _build_; installing
them needs virtual machines that do not exist, and the update path additionally
needs two published releases. Every row here is unverified.
