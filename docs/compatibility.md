# Compatibility scan and tier

What the application learns about the machine at startup, what it concludes,
and what it refuses to conclude.

Design: `docs/superpowers/specs/2026-08-02-compatibility-setup-design.md`.
This document describes stages 1–4 of that design, which are the stages that
exist. Acquisition, hand-off and the setup wizard are designed and not built.

---

## 1. The scan

`crates/platform`'s `probe` module reads the machine into a `SystemSnapshot`.
Run it directly with:

```
cargo run -p project-host-platform --example scan
```

**The scan has no failure case.** Every field is optional, and a probe that
cannot answer yields `None`. A machine that will not report its GPU still gets
tiered and still gets Docker; a scan that could fail would be a scan that
blocks setup on something irrelevant to it.

| Group          | Read from                                                       |
| -------------- | --------------------------------------------------------------- |
| CPU, memory    | `sysinfo`                                                       |
| Volumes        | `sysinfo`, including removable and SSD/HDD                      |
| Architecture   | the compiled target — always known                              |
| OS identity    | `sysinfo`, plus `Win32_OperatingSystem` for the Windows edition |
| Virtualization | `Win32_Processor`/`Win32_ComputerSystem`; `/proc/cpuinfo` flags |
| WSL            | `wsl --status`                                                  |
| Distribution   | `/etc/os-release`                                               |
| GPU            | `Win32_VideoController`; `lspci`                                |

Platform reads go through a subprocess rather than FFI. The workspace sets
`unsafe_code = "forbid"` and `forbid` cannot be downgraded, so `WinVerifyTrust`
and friends are unavailable — the same reasoning that chose `taskkill` in the
host run mode design. Every `#[cfg]` in the scan lives in
`probe/platform_specific.rs` and nowhere else.

### 1.1 The build number

Two fields decide a great deal and neither has one format.

`sysinfo` reports `os_version` as `"11 (26200)"` on the development machine and
as the dotted `"10.0.26200"` elsewhere; the build also appears in
`kernel_version`. `parse_build` tries the parenthesised form, then the dotted
form, then the kernel field, and requires at least four digits so a marketing
version — the `11` in `"11 (26200)"` — is never mistaken for a build.

This was found by running the scan on real hardware. The first implementation
read only the dotted form and returned `None` on this machine, which would have
selected a Docker backend from a missing build number.

---

## 2. Tier

`crates/compatibility` turns the snapshot into a tier and the resource defaults
that follow. It is pure: no I/O, no process, no file. Every decision is tested
against constructed machines rather than the one running the tests.

| Tier          | Trigger                                        | `memory_limit_mb` | `cpu_limit_cores` | `process_limit` |
| ------------- | ---------------------------------------------- | ----------------- | ----------------- | --------------- |
| `MINIMAL`     | <4 logical cores, or <8 GB RAM, or <20 GB free | 512               | 0.5               | 128             |
| `STANDARD`    | <8 logical cores, or <16 GB RAM                | 1024              | 1.0               | 256             |
| `PERFORMANCE` | otherwise                                      | 2048              | 2.0               | 512             |

The tier is the **weakest** of the three axes, not an average. A 32-core machine
with 6 GB of RAM is `MINIMAL`; averaging would hand it defaults it cannot
honour. `None` on any axis counts as that axis's weakest value — a machine that
will not say how much memory it has is not assumed to have plenty.

**CPU age is not an input.** Release year is a weak predictor of throughput — a
2013 Xeon with 64 GB outruns a 2023 Celeron with 4 GB — and tiering on it would
misjudge precisely the low-end machines that most need correct limits. It would
also need a model-string-to-year table, which is large, fuzzy, and stale the day
it is written. CPU model and GPU are collected and reported; they decide
nothing.

### 2.1 Advertised versus reported memory

The memory thresholds sit 6% below the round number they stand for. A machine
sold as 16 GB never reports 16 GiB: firmware reserves some, and the advertised
figure is decimal GB against a binary GiB reading. The development machine
reports 15.1 GiB, so a literal `>= 16 GiB` threshold placed it — and
effectively every real 16 GB machine — one tier below where it belongs. Tests
pin both directions: 16 GB reaches `PERFORMANCE`, and 12 GB does not.

### 2.2 The invariant that outranks the table

**No default exceeds 12.5% of total RAM**, with a floor of 256 MB. The table is
a set of round numbers chosen for legibility; the invariant is what keeps them
safe on a machine the table did not anticipate. It is asserted over every
machine in the golden set, not spot-checked.

Every default is also asserted to satisfy the `projects` CHECK constraints in
`0001_initial.sql`. A default the schema rejects fails at the moment a user
presses Create, which is the worst place to discover it.

### 2.3 Where the defaults apply

To **newly created projects only**, through `AppState::resource_defaults()`.
Existing projects are never modified: a user who set a limit deliberately does
not have it overwritten by a scan.

`storage_limit_mb` is not tiered. No probe here measures what a project will
store, so tiering it would be a guess dressed as a measurement.

---

## 3. Selection

The catalogue declares each artefact beside its requirements. Selection returns
**exactly one artefact, or a non-empty list of blockers** — no fallback, no
closest match, no default artefact.

| Artefact                        | Host    | Requires                                              |
| ------------------------------- | ------- | ----------------------------------------------------- |
| `docker-desktop-windows-x86_64` | Windows | x86_64, build ≥ 19045, virtualization enabled, 20 GB  |
| `docker-engine-linux`           | Linux   | x86_64 or aarch64, a supported package manager, 10 GB |

The guarantee — never install an incompatible version — rests on one property
test: for every machine in the golden set, if selection returns an artefact,
that machine satisfies every requirement the artefact declares. The test
re-derives each check independently rather than calling `unmet`, because a
property test that called the function it verifies would be a tautology.

That independence earned its keep immediately. `Artifact.host` was declared and
never checked, so a Linux machine reporting a Windows-shaped build number
satisfied every remaining Docker Desktop requirement and would have been handed
a `.exe`. `unmet` now takes the whole artefact rather than its `Requirements`,
so the host cannot be overlooked again.

Windows on ARM has no Docker Desktop build and is therefore blocked, not served
the x64 installer.

---

## 4. Blockers

Every variant names concrete values. "Virtualization is disabled" without the
key to press is the failure mode this design exists to replace.

| Blocker                     | Fixable | Names                                            |
| --------------------------- | ------- | ------------------------------------------------ |
| `VirtualizationDisabled`    | yes     | the setting, and how to reach firmware setup     |
| `VirtualizationUnsupported` | no      | that the CPU lacks it, so no fix exists here     |
| `ArchitectureUnsupported`   | no      | the architecture found                           |
| `OsTooOld`                  | yes     | the build found and the build required           |
| `OsBuildUnknown`            | no      | that nothing is installed on a guess             |
| `EditionUnsupported`        | no      | the edition found and what was needed            |
| `InsufficientDisk`          | yes     | space free, space required                       |
| `NoPackageManager`          | yes     | which managers are supported                     |
| `HostUnrecognised`          | no      | that no installer can be matched to this machine |

**Firmware cannot be changed by any application.** Where virtualization is
supported but disabled, the blocker names the vendor's own setting — Intel VT-x
or AMD SVM, chosen from the CPU vendor, because naming the wrong one sends the
user hunting for a setting their firmware does not have — and says plainly that
no application can change it.

`is_fixable` distinguishes a machine the user can do something about from one
they cannot, so the interface never offers a retry for a missing CPU feature.

---

## 5. What is verified, and what is not

Verified on the development machine (Windows 11, no Docker, no WSL, no Linux):

- every tier, selection and blocker decision, against the golden set
- the selection property, the 12.5% invariant, and the schema-constraint check
- all parsing: build numbers, `/etc/os-release`, `/proc/cpuinfo` flags,
  virtualization CSV, `wsl --status`, GPU lines
- the Windows probes end to end, through the `scan` example

**Not verified, and to be treated as unproven until run elsewhere:**

- every Linux probe invocation — `/proc/cpuinfo`, `/etc/os-release`, `lspci`.
  The parsers are tested; the reads have never run.
- the Windows edition and GPU probes on any machine but this one
- `wsl --status` parsing against a machine that actually has WSL. This one
  reports WSL absent, so only the absent branch has executed.
- everything in stages 5–8 of the design, which is not built: no download, no
  hand-off, no test container, no wizard. **This code selects an artefact; it
  has never installed one.**
