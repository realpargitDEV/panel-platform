# Automatic compatibility and Docker setup

Date: 2026-08-02
Status: designed, not implemented

On first launch, look at the machine, decide what Docker it can run, fetch that,
hand it to its own installer, prove it works, and set project defaults the
hardware can actually sustain.

Today a machine without a daemon reaches `install_hint()` in
`crates/platform/src/docker.rs` — one paragraph and a documentation URL. That
paragraph is correct and it is the entire onboarding story. It does not say
whether _this_ machine can run Docker at all, which of several installers is the
right one, or why the daemon that is installed still is not answering.

This design replaces that with a scan, a decision, an acquisition and a proof.

---

## 1. What this does not do, said out loud

**It does not enable virtualization.** No API toggles VT-x or SVM; the setting
lives in firmware and is reachable only by rebooting into it. Where
virtualization is supported but disabled, the only honest output is an
instruction naming the vendor's setup key and the setting's real name. Any
design that implies otherwise is lying to the user.

**It does not run the installer silently.** The application downloads the
correct artefact, proves who published it, and hands off to the vendor's own
installer. The user sees Docker's licence and the elevation prompt. Docker
Desktop requires a paid subscription above a size threshold, and an application
that installs it unattended has accepted those terms on someone else's behalf.
On Linux the equivalent consent is joining the `docker` group, which is
root-equivalent and is therefore also asked rather than assumed.

**It does not write machine-global configuration.** The tier sets defaults for
projects this application creates. The Docker Desktop VM cap — `.wslconfig` —
is _displayed_ as a recommended block to copy. That file is shared by every
WSL2 consumer on the machine, and rewriting software we do not own is not ours
to do quietly.

**It does not block.** A machine that genuinely cannot run Docker keeps
everything that works without one: creating projects, editing files, settings.
The wizard is dismissible, and says what was wrong on the way out.

---

## 2. Shape

Four units. The split exists so that every decision is a pure function of a
value, and can be tested on a machine with no Docker, no WSL and no Linux —
which is the machine this is written on.

| Unit                         | Responsibility                                        | Depends on                                    |
| ---------------------------- | ----------------------------------------------------- | --------------------------------------------- |
| `crates/platform` (extended) | Probe the OS, produce a `SystemSnapshot`              | the OS                                        |
| `crates/compatibility` (new) | `SystemSnapshot` → tier, plan, or blockers. No I/O.   | nothing                                       |
| `crates/docker-setup` (new)  | Catalogue, download, verify, hand off, test container | `platform`, `compatibility`, `docker-manager` |
| `app-core/src/setup_flow.rs` | The retryable step machine the interface drives       | all three                                     |

`crates/platform` remains the only place carrying `#[cfg(windows)]` or
`#[cfg(unix)]`, as `docs/platform-support.md` requires.

`crates/compatibility` depends on nothing — not `docker-manager`, not
`api-types` — for the reason `detection` and `host-runner` depend on nothing:
it should be possible to reason about whether a machine can run Docker without
also holding the container model, or the wire format, in mind. It takes a
`SystemSnapshot` by value, so every test constructs the machine it wants to
test against rather than describing the one it is running on.

---

## 3. The scan

`SystemSnapshot` is a plain data value. **Every field degrades rather than
fails**: a probe that cannot answer yields `Unknown`, and the scan as a whole
has no failure case. A machine that will not report its GPU still gets Docker
installed, and a scan that could fail would be a scan that blocks setup on
something irrelevant to it.

| Group            | Fields                                                                 |
| ---------------- | ---------------------------------------------------------------------- |
| CPU              | vendor, model string, physical cores, logical cores                    |
| Memory           | total bytes, available bytes                                           |
| Storage          | per volume: mount point, total, free, removable, `SSD`/`HDD`/`Unknown` |
| Architecture     | `x86_64`, `aarch64`, other                                             |
| Operating system | name, edition, version, **build number**, kernel version               |
| Virtualization   | supported, enabled, hypervisor already present                         |
| Windows          | WSL present, WSL version, default WSL distro                           |
| Linux            | distro id, version id, package manager (`apt`/`dnf`/`pacman`/`zypper`) |
| GPU              | vendor, model                                                          |

`probe/` holds one module per group, and each is the only place that knows how
its group is read on a given OS. The submodules return their own small types;
assembling them into a `SystemSnapshot` is the crate's single public entry
point.

**Build number is load-bearing**, not decoration. It is the field that decides
whether the WSL2 backend is available on a given Windows install, and a
selection made without it is a guess.

GPU and CPU model are collected and reported, and feed no decision. They are
there because a setup failure reported without them is a setup failure nobody
can diagnose remotely. This is stated so that a later reader does not "fix" the
tier by wiring them into it — see §4.

---

## 4. Tier

A pure function, `SystemSnapshot` → `PerformanceTier` → `ResourceDefaults`.

| Tier          | Trigger                                                             | `memory_limit_mb` | `cpu_limit_cores` | `process_limit` |
| ------------- | ------------------------------------------------------------------- | ----------------- | ----------------- | --------------- |
| `Minimal`     | <4 logical cores, or <8 GB RAM, or <20 GB free on the target volume | 512               | 0.5               | 128             |
| `Standard`    | <8 logical cores, or <16 GB RAM                                     | 1024              | 1.0               | 256             |
| `Performance` | otherwise                                                           | 2048              | 2.0               | 512             |

The tier is the **weakest** of the three axes, not an average. A 32-core machine
with 6 GB of RAM is a `Minimal` machine, and averaging would hand it defaults it
cannot honour.

**CPU age is deliberately not an input.** Release year is a weak predictor of
throughput — a 2013 Xeon with 64 GB outruns a 2023 Celeron with 4 GB — and
tiering on it would misjudge precisely the low-end machines that most need
correct limits. It would also require a model-string-to-year table, which is
large, fuzzy, and stale the day it is written. Cores, memory and free disk are
measured facts.

`Unknown` on any axis is treated as its weakest value. A machine that will not
say how much memory it has is not assumed to have plenty.

These become the defaults for **newly created** projects, written into the
existing `memory_limit_mb`, `cpu_limit_cores` and `process_limit` columns that
`app-core/src/lifecycle.rs:147` already feeds to
`ResourceLimits::from_user_values`. No existing project is altered: a user who
set a limit deliberately does not have it overwritten by a wizard.

One invariant outranks the table: **no default exceeds 12.5% of total RAM.** The
table is a set of round numbers chosen for legibility; the invariant is what
makes it safe on a machine the table did not anticipate. It is asserted as a
property over every synthetic machine in the golden set, not spot-checked.

---

## 5. Selection

The catalogue lists each installer artefact beside its _requirements_:
architecture, minimum OS build or kernel, required edition, whether
virtualization must be enabled, and free disk needed.

Selection is a total function returning **exactly one artefact, or a non-empty
list of blockers**. There is no fallback, no "closest match" and no default
artefact. This is the whole of "never install an incompatible version", and it
is enforced by one property test:

> For every machine in the golden set, if selection returns an artefact, that
> machine satisfies every requirement that artefact declares.

A property over the whole set, rather than a case per artefact, is what keeps
the guarantee true when a future artefact is added.

| Host    | Target                                                                |
| ------- | --------------------------------------------------------------------- |
| Windows | Docker Desktop; WSL2 or Hyper-V backend chosen from edition and build |
| Linux   | Docker Engine, for the detected package manager                       |

Windows on ARM has no Docker Desktop artefact, and is therefore a blocker rather
than a wrong download.

### 5.1 Authenticity

`crates/setup` verifies our own artefacts with minisign against a key compiled
into the binary. That is unavailable here: these artefacts are Docker's, and we
hold no key over them.

Pinning a SHA-256 per release is the obvious substitute and is wrong. Docker
publishes new installers continuously; a pinned digest goes stale in weeks and
converts every Docker release into a setup that refuses to proceed. The pin
would be removed under pressure, which is worse than not having relied on one.

Publisher identity is verified instead:

- **Windows** — `Get-AuthenticodeSignature` through a PowerShell subprocess,
  requiring a valid signature naming Docker Inc. The workspace sets
  `unsafe_code = "forbid"` for every crate and `forbid` cannot be downgraded, so
  `WinVerifyTrust` through FFI is not available. A subprocess is safe and
  sufficient — the same reasoning that chose `taskkill` in the host run mode
  design.
- **Linux** — install Docker's official repository key, pinned by fingerprint,
  and let `apt` or `dnf` verify every package against it natively. This is the
  mechanism the distribution already trusts, rather than one invented here.

> **The fingerprint is to be read from Docker's published installation
> documentation at implementation time and cited in a comment beside it.** It is
> deliberately absent from this document. A fingerprint written from memory into
> a security check is worse than no check, because it looks like one.

An artefact failing verification is deleted rather than run, and nothing is
written to its final location until it has passed. There is no override flag,
matching `docs/installers.md` §8.

---

## 6. The step machine

Eight steps. Each carries
`Pending | Running | Ok | Failed { reason, retryable, manual_instructions }`,
and all eight are visible from the start, so the user sees the shape of what is
about to happen rather than a spinner that occasionally changes its caption.

| #   | Step           | Notes                                                     |
| --- | -------------- | --------------------------------------------------------- |
| 1   | Scan           | Cannot fail; unavailable probes yield `Unknown`           |
| 2   | Assess         | Tier, plan or blockers. Pure.                             |
| 3   | Prerequisites  | Virtualization, OS support, free disk. The firmware gate. |
| 4   | Acquire        | Download, then verify publisher                           |
| 5   | Hand off       | Vendor installer; the one privileged prompt               |
| 6   | Await daemon   | Poll the endpoints `platform::docker` already knows       |
| 7   | Test container | Pull, run, assert exit 0, remove                          |
| 8   | Apply defaults | Tier defaults stored; `.wslconfig` block displayed        |

Steps 4 to 6 are skipped when a healthy daemon already answers. A machine that
already has Docker gets a scan, a tier and a proof — not an install it does not
need.

**Retry is per step.** A user who reboots into firmware, enables VT-x and
returns resumes at step 3. Restarting the whole wizard after a reboot would
throw away the only expensive part, and would ask them to sit through the scan
to reach the step they just fixed.

**Manual setup is not a separate mode.** Every step carries its manual
equivalent — the exact command, or the exact download URL — and a failed step
renders it. This is why there is no parallel "manual path" to maintain and no
way for the two to disagree.

Step 7 is what licenses the word "working". `hello-world` is roughly 20 KB and
exercises the entire chain: registry reachability, pull, create, run, exit.
Failure is reported as failure. **The wizard never reports a success it did not
observe** — the same rule `lifecycle.rs` states about writing status from what
is observed rather than what was intended.

Completion is recorded so the wizard runs once, and it is re-runnable from
Settings, because both the hardware and the Docker installation can change after
first launch.

---

## 7. Parity

Windows and Linux get the same eight steps, the same scan, the same tiering and
the same proof. Only the mechanism inside steps 4, 5 and 7 differs.

This mirrors `crates/setup`, which already hands off to the NSIS installer's own
interface on Windows and to `pkexec dpkg -i` on Linux: one shape, one set of
trust checks, two mechanisms. The precedent is followed rather than a second
pattern invented.

The licence asymmetry is real and is handled in content, not in structure:
Docker Engine is Apache-2.0, so Linux has no subscription question, but joining
the `docker` group is root-equivalent and is presented as its own consent step.
Each platform ends up with exactly one privileged prompt.

---

## 8. Errors

`SetupBlocker` carries what failed, and what the user can do:

| Blocker                     | Names                                                             |
| --------------------------- | ----------------------------------------------------------------- |
| `VirtualizationDisabled`    | That it is supported but off, and how to reach firmware           |
| `VirtualizationUnsupported` | That the CPU lacks it, so no fix exists on this machine           |
| `ArchitectureUnsupported`   | The architecture, and that no artefact is published for it        |
| `OsTooOld`                  | The build found, and the build required                           |
| `EditionUnsupported`        | The edition, and which backend it would have needed               |
| `InsufficientDisk`          | Free space, space required, and on which volume                   |
| `VerificationFailed`        | What was expected of the signature, and that the file was deleted |
| `HandoffFailed`             | The installer's own exit code, and the manual command             |
| `DaemonNeverAnswered`       | The endpoints tried, and how long was waited                      |
| `TestContainerFailed`       | The stage — pull, create or run — and the output                  |

Every variant names concrete values. "Virtualization is disabled" without the
key to press is the failure mode this whole design exists to replace.

---

## 9. Testing

The machine this is written on has no Docker, no WSL and no Linux. What can and
cannot be verified is stated rather than assumed.

Verifiable here, and the bulk of the value:

- tiering, selection and blocker classification against a **golden set of
  synthetic machines** — a 2015 dual-core 4 GB laptop, Windows on ARM, VT-x
  present but disabled, 4 GB free disk, a 64 GB workstation, and a machine that
  answers `Unknown` to everything — each asserting an exact tier, plan and
  blocker list
- the selection property in §5, over that whole set
- the 12.5% invariant in §4, over that whole set
- probes behind an injected trait, so results never depend on the test machine
- `SystemSnapshot` serialization round-trip, as the other wire types have
- the step machine's transitions, retry and skip logic, against a fake acquirer

Not verifiable here, and to be treated as unproven until run elsewhere:

- any real install, on either platform
- Authenticode verification against a real signed installer
- the daemon interaction and the test container
- every Linux path, including the repository key and `pkexec`

Modules covering the unverifiable paths carry an explicit note saying so, as
`crates/host-runner/src/lib.rs` and `app-core/src/lifecycle.rs` already do.

---

## 10. Staging

Ordered so each stage is useful alone, and the least verifiable work is last.

1. `SystemSnapshot` and `crates/platform` probes, per group, degrading to
   `Unknown`
2. `crates/compatibility`: tier, defaults, the 12.5% invariant, the golden set
3. Catalogue and selection, with the §5 property test
4. Tier defaults applied to newly created projects — useful on its own, with no
   wizard at all
5. The step machine in `app-core`, against a fake acquirer
6. Download and publisher verification
7. Hand off, await daemon, test container
8. The interface: the wizard, the system report, the `.wslconfig` block, the
   Settings entry point

Stage 4 is worth landing on its own: it improves defaults for every user
immediately, needs none of the acquisition machinery, and is entirely testable
here.
