# Download manager

Date: 2026-07-31
Status: approved, not yet implemented

A small binary a user downloads and runs, which fetches the real installer for
their platform, verifies it, and hands off to it. Shipped for Windows and Linux
and attached to the same GitHub release as everything else.

It exists because the download page cannot know which of five artefacts a
visitor needs, and because the Linux AppImage is 89 MB — a stub that downloads
only what applies is a smaller ask than a page of choices, four of which are
wrong for any given reader.

It is also a new unsigned binary whose entire purpose is to download and execute
code. That is the reason signature verification is not a feature of this design
but the spine of it: there is no flag to skip it, and an artefact that fails it
is deleted rather than run.

---

## 1. Outputs

Built by `.github/workflows/release.yml` on a `v*` tag and attached to the same
draft release as the installers.

| Platform | Artefact                        | Approx. size |
| -------- | ------------------------------- | ------------ |
| Windows  | `PanelPlatformSetup.exe`        | ~5 MB        |
| Linux    | `panel-platform-setup-x86_64`   | ~5 MB        |

Both are listed in `SHA256SUMS.txt` with every other asset.

### 1.1 Why eframe/egui

The stub runs on a machine that has **not yet installed the application**, so it
may not link anything the application's packages provide. That rules out
webkit2gtk, which is what a Tauri window would need on Linux and what the `.deb`
exists to pull in.

`eframe` on the `glow` backend links only libGL and X11/Wayland, which any Linux
desktop already has, needs no C++ toolchain, and is pure Rust. `native-windows-gui`
is Windows-only; `iced` and `slint` are larger for no gain at this size; `fltk`
needs a C++ toolchain in CI.

---

## 2. Sequence

```
check latest release → select asset → confirm → download → verify → hand off → exit
```

Each step is a module with one job, and the two that decide anything are pure
functions over data rather than code that touches the network.

| Module        | Responsibility                                                  |
| ------------- | --------------------------------------------------------------- |
| `release.rs`  | Parse the GitHub API response into `Release { version, assets }` |
| `target.rs`   | `select_asset(&Release, Platform, LinuxTools) -> Result<&Asset>` |
| `download.rs` | Streaming GET to a private temp file, progress, cap, timeout     |
| `verify.rs`   | `verify(bytes, sig, sums) -> Result<()>`                         |
| `handoff.rs`  | Build and spawn the install command                              |
| `ui.rs`       | The state machine below, drawn                                   |

```
Checking → Confirm → Downloading → Verifying → Launching → Done
    ↓         ↓           ↓            ↓           ↓
                       Failed(reason)
```

`Failed` carries a reason that names what went wrong and what the user can do
about it, not a status code.

### 2.1 Check

`GET https://api.github.com/repos/realpargitDEV/panel-platform/releases/latest`,
anonymous, no token.

That endpoint **excludes drafts**, which is the correct behaviour for this
purpose: a draft is by definition not fit to install, and the stub must not be
able to reach one. While no release is published it returns 404, and the stub
says so in those words — *no published release yet* — rather than reporting a
network error for a server that answered correctly.

Anonymous GitHub API calls are rate-limited by IP. A 403 with
`x-ratelimit-remaining: 0` is reported as a rate limit with the reset time, not
as a failure to reach GitHub.

### 2.2 Select

Pure, total, and tested for every platform on every platform — not behind
`#[cfg]`, for the reason §5 gives.

| Host                                  | Asset                       |
| ------------------------------------- | --------------------------- |
| Windows x64                           | `*_x64-setup.exe` (NSIS)    |
| Linux, `dpkg` **and** `pkexec` present | `*_amd64.deb`               |
| Linux, either missing                 | `*_amd64.AppImage`          |
| Anything else                         | Refuse, naming the platform |

The NSIS `.exe` is chosen over the `.msi` for the reason `docs/installers.md`
§1 already gives: it bootstraps WebView2, the one dependency the application
cannot supply itself. The `.deb` is preferred on Linux when it can actually be
installed, and the AppImage is the answer when it cannot — a stub that demands
`dpkg` on Fedora would be choosing the artefact for its own convenience.

`select_asset` takes `LinuxTools` as a parameter rather than probing `PATH`
itself, so every branch is reachable in a test on any host.

### 2.3 Confirm

Nothing downloads before the user presses a button. The confirm screen names the
version, the artefact, its size, and that the build is unsigned per
`docs/installers.md` §6 — the same thing the release notes say, at the moment it
matters, rather than in a document nobody opened.

### 2.4 Download

To a directory created private to the user (`0700` on Linux), never to a
world-writable temp path where another process could swap the file between
verification and execution.

- Transport rules are `crates/updater`'s: `https` only, host on the allowlist
  `api.github.com`, `github.com`, `objects.githubusercontent.com`. Redirects are
  re-validated against the allowlist and capped.
- Size is capped at 256 MB and compared against the length the API reported.
- The whole transfer has a timeout, so a stalled connection fails rather than
  showing a progress bar forever.
- Cancel deletes the partial file.

### 2.5 Verify

Two independent checks, both required:

| Check     | Against                                   | Defeats                          |
| --------- | ----------------------------------------- | -------------------------------- |
| minisign  | Public key compiled into the binary       | A forged or substituted artefact |
| SHA-256   | `SHA256SUMS.txt` from the same release     | Corruption in transit            |

The signature is the one that matters. `SHA256SUMS.txt` is fetched over the same
channel as the artefact, so anyone able to substitute one can substitute the
other; it is an integrity check, not an authenticity check, and the design does
not pretend otherwise.

The public key is emitted by `build.rs` reading
`apps/desktop/src-tauri/tauri.conf.json`, so the stub and the in-app updater
trust the same key by construction and cannot drift apart. A key supplied by the
feed is never used, for the same reason `docs/installers.md` §5 gives.

A file that fails either check is deleted and the failure is reported as a
failed verification, in those words. There is no override, no `--insecure`, and
no "continue anyway" button.

### 2.6 Hand off

| Host                  | Action                                                                     |
| --------------------- | -------------------------------------------------------------------------- |
| Windows               | Spawn the NSIS installer, let its UI take over, exit                        |
| Linux, `.deb`         | `pkexec dpkg -i` — one password prompt — then `apt-get -f install` if needed |
| Linux, AppImage       | `chmod +x`, move to `~/.local/bin`, write a `.desktop` entry                 |

The stub adds no install logic of its own on Windows or for the `.deb`: those
paths are already built and already smoke-tested in CI, and a second
implementation of them would be a second thing to get wrong. The AppImage path
is the exception because no packager owns it.

`pkexec` returning 126/127 (dismissed or unavailable) is reported as *not
authorised*, distinctly from a failed install.

---

## 3. `--silent`

The same pipeline with text output and no window. It exists so the stub works
over SSH and in CI, and it is the mode §4's smoke test runs.

`--dry-run` stops after verification without installing anything.

---

## 4. Release integration

A `bootstrap` job in `release.yml` with `needs: check-version`, building both
stubs and attaching them to the draft.

Two existing jobs change, because the current graph is
`check-version → build → {checksums, smoke-linux, smoke-windows} → release-gate`
and `checksums` today needs only `build`:

| Job            | Was                                          | Becomes                                                   |
| -------------- | -------------------------------------------- | --------------------------------------------------------- |
| `checksums`    | `needs: build`                                | `needs: [build, bootstrap]`                                |
| `release-gate` | `needs: [build, checksums, smoke-*]`          | `needs: [build, bootstrap, checksums, smoke-*]`             |

Without the first, `checksums` races `bootstrap` and `SHA256SUMS.txt` silently
omits the two artefacts most in need of a checksum. Without the second, a broken
stub cannot fail the gate.

The stubs do not depend on `build` — nothing in them is produced by it — so they
compile in parallel with the Tauri builds rather than after them.

Its smoke step runs `--silent --dry-run` on both runners: resolve the latest
release, download, verify, stop. This proves the real path against real
artefacts over the real network.

**This test cannot pass until a release is published.** Until then the endpoint
in §2.1 returns 404 by design, and the only branch CI can exercise is the error
path. That is stated here rather than discovered later, and the job asserts the
404 path explicitly so it is a test that ran rather than a test that was skipped.

`site/index.html`'s download button points at the stub. `docs/installers.md`
gains a section describing it alongside the other artefacts.

---

## 5. Testing

| Test                                                    | Runs                  |
| ------------------------------------------------------- | --------------------- |
| `select_asset` for every platform × tool combination    | Every host            |
| Release JSON parsed from fixtures, including 404 and 403 | Every host            |
| Verification accepts a good artefact                     | Every host            |
| Verification rejects a tampered byte, a wrong signature, and a missing one | Every host |
| Download honours the size cap and the timeout            | Local server          |
| Handoff builds the right command per platform, without spawning | Every host    |
| `--silent --dry-run` against the published release       | Both release runners  |

No test is behind `#[cfg(unix)]` or `#[cfg(windows)]`. The Linux paths defect —
an application that had never once started for a non-root user — survived
because the only test that would have caught it could not run on the machine the
project is developed on. `select_asset` and `handoff` take their platform as a
parameter for exactly that reason.

**What these prove:** the right artefact is chosen, a bad one is refused, and
the install command is correct.

**What they do not prove:** that the handoff installs anything. That needs the
VMs `docs/installers.md` §7.2 already lists and still does not have.
