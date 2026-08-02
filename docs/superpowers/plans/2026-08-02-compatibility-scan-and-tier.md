# Compatibility Scan and Tier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scan the machine, classify it into a performance tier, decide which Docker artefact it can run (or why it cannot), and give newly created projects resource defaults the hardware can sustain.

**Architecture:** `crates/platform` gains a `probe/` module that reads the OS and produces a plain `SystemSnapshot` value. A new dependency-free crate `crates/compatibility` turns that value into a tier, resource defaults, and either one Docker artefact or a list of blockers — all pure functions, tested against synthetic machines. `apps/desktop/src-tauri` uses the defaults when creating a project.

**Tech Stack:** Rust 2021 (MSRV 1.82), `sysinfo` for hardware counts, PowerShell/`/proc` subprocess reads for OS specifics, `serde`, `tokio`, SQLite via sqlx.

Spec: `docs/superpowers/specs/2026-08-02-compatibility-setup-design.md`

**Scope:** This plan covers spec stages 1–4 only. Stages 5–7 (step machine, download, hand off, test container) and stage 8 (wizard interface) are separate plans. This plan ships working software on its own: every user gets project defaults matched to their machine, with no wizard involved.

## Global Constraints

- The workspace sets `unsafe_code = "forbid"` for every crate and `forbid` cannot be downgraded. No FFI, no raw Win32. Use `sysinfo` or a subprocess.
- Clippy runs as `cargo clippy --workspace --all-targets -- -D warnings` with `unwrap_used`, `expect_used`, `panic`, `todo` and `unimplemented` all set to `deny`. Every new crate's `lib.rs` MUST open with the workspace's standard test allowance block (given verbatim in Task 1).
- No `#[cfg(windows)]` or `#[cfg(unix)]` outside `crates/platform` and that crate's own tests.
- `crates/compatibility` depends on `project-host-platform` and nothing else — not `docker-manager`, not `api-types`.
- A probe that cannot answer yields `None`. The scan itself has no failure case.
- `None` on any tier axis is treated as that axis's weakest value.
- No resource default may exceed 12.5% of total RAM.
- Every default must satisfy the existing schema CHECKs in `crates/database/migrations/0001_initial.sql:123-126`: `memory_limit_mb BETWEEN 64 AND 65536`, `cpu_limit_cores > 0 AND <= 64`, `process_limit BETWEEN 8 AND 4096`.
- Existing projects are never modified. Defaults apply to newly created projects only.
- CPU model and GPU are collected and reported but MUST NOT feed the tier.
- Run `pnpm verify` before every commit.

---

### Task 1: `SystemSnapshot` data model

Pure data. No probing, no OS calls. Everything later in this plan is a function of this type, so it lands first and alone.

**Files:**

- Create: `crates/platform/src/snapshot.rs`
- Modify: `crates/platform/src/lib.rs` (add `pub mod snapshot;` and re-export)

**Interfaces:**

- Produces: `SystemSnapshot`, `CpuInfo`, `MemoryInfo`, `VolumeInfo`, `StorageKind`, `Architecture`, `OsInfo`, `VirtualizationInfo`, `WindowsInfo`, `LinuxInfo`, `PackageManager`, `GpuInfo`, and `SystemSnapshot::unknown()`.

- [ ] **Step 1: Write the failing test**

Append to `crates/platform/src/snapshot.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_snapshot_claims_nothing() {
        // The scan cannot fail. A machine that answers no probe at all must
        // still produce a valid snapshot, because every decision downstream is
        // a function of this value.
        let snapshot = SystemSnapshot::unknown();
        assert_eq!(snapshot.cpu.logical_cores, None);
        assert_eq!(snapshot.memory.total_bytes, None);
        assert!(snapshot.volumes.is_empty());
        assert_eq!(snapshot.virtualization.enabled, None);
        assert!(snapshot.gpus.is_empty());
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let mut snapshot = SystemSnapshot::unknown();
        snapshot.cpu.logical_cores = Some(8);
        snapshot.memory.total_bytes = Some(16 * 1024 * 1024 * 1024);
        snapshot.arch = Architecture::X86_64;
        snapshot.os.build = Some(26200);
        snapshot.volumes.push(VolumeInfo {
            mount_point: "C:\\".to_string(),
            total_bytes: 500_000_000_000,
            free_bytes: 200_000_000_000,
            removable: false,
            kind: StorageKind::Ssd,
        });

        let json = serde_json::to_string(&snapshot).expect("serialise");
        let back: SystemSnapshot = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn an_unrecognised_architecture_keeps_its_name() {
        // Reporting "other" without saying which is a bug report nobody can act
        // on.
        let arch = Architecture::from_target("riscv64");
        assert_eq!(arch, Architecture::Other("riscv64".to_string()));
        assert_eq!(Architecture::from_target("x86_64"), Architecture::X86_64);
        assert_eq!(Architecture::from_target("aarch64"), Architecture::Aarch64);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-platform snapshot`
Expected: FAIL — `snapshot` module does not exist.

- [ ] **Step 3: Implement**

Create `crates/platform/src/snapshot.rs`, above the test module:

```rust
//! What this machine is.
//!
//! A plain value, produced by `crate::probe` and consumed by
//! `project-host-compatibility`. Splitting the data from the reading is what
//! lets every compatibility decision be tested against a machine that was
//! constructed rather than one that happens to be running the test.
//!
//! **Every field is optional, and the scan has no failure case.** A probe that
//! cannot answer yields `None`. A machine that will not report its GPU still
//! gets Docker installed, and a scan that could fail would be a scan that
//! blocks setup on something irrelevant to it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CpuInfo {
    pub vendor: Option<String>,
    /// Reported, never used to decide anything. See the tier rules in
    /// `project-host-compatibility`: release year is a weak predictor of
    /// throughput, and tiering on it would misjudge the low-end machines that
    /// most need correct limits.
    pub model: Option<String>,
    pub physical_cores: Option<u32>,
    pub logical_cores: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoryInfo {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Removable volumes are excluded from every capacity decision: a USB stick
    /// with 400 GB free must not make a machine look roomy.
    pub removable: bool,
    pub kind: StorageKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
    Aarch64,
    Other(String),
}

impl Architecture {
    /// From a Rust target arch string, e.g. `std::env::consts::ARCH`.
    pub fn from_target(arch: &str) -> Self {
        match arch {
            "x86_64" => Architecture::X86_64,
            "aarch64" => Architecture::Aarch64,
            other => Architecture::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OsInfo {
    /// e.g. "Windows 11 Pro", "Ubuntu"
    pub name: Option<String>,
    /// e.g. "Pro", "Home". Decides whether the Hyper-V backend is available.
    pub edition: Option<String>,
    /// e.g. "10.0.26200"
    pub version: Option<String>,
    /// The number that decides whether the WSL2 backend exists on a given
    /// Windows install. A selection made without it is a guess.
    pub build: Option<u32>,
    pub kernel: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VirtualizationInfo {
    /// The CPU has VT-x or SVM.
    pub supported: Option<bool>,
    /// It is switched on in firmware. No API can change this; only a reboot
    /// into firmware setup can.
    pub enabled: Option<bool>,
    /// Something is already running as a hypervisor — Hyper-V, WSL2, or this
    /// machine is itself a guest.
    pub hypervisor_present: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WindowsInfo {
    pub wsl_present: bool,
    pub wsl_version: Option<u32>,
    pub default_distro: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LinuxInfo {
    pub distro_id: Option<String>,
    pub version_id: Option<String>,
    pub package_manager: Option<PackageManager>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: Option<String>,
    pub model: Option<String>,
}

/// Everything the scan learned about this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub volumes: Vec<VolumeInfo>,
    pub arch: Architecture,
    pub os: OsInfo,
    pub virtualization: VirtualizationInfo,
    /// Present only on Windows.
    pub windows: Option<WindowsInfo>,
    /// Present only on Linux.
    pub linux: Option<LinuxInfo>,
    /// Reported, never used to decide anything: no container here requests GPU
    /// access. Collected because a setup failure reported without it is a
    /// failure nobody can diagnose remotely.
    pub gpus: Vec<GpuInfo>,
}

impl SystemSnapshot {
    /// A snapshot that knows nothing. The starting point for the real scan, and
    /// the base for constructing test machines.
    pub fn unknown() -> Self {
        Self {
            cpu: CpuInfo::default(),
            memory: MemoryInfo::default(),
            volumes: Vec::new(),
            arch: Architecture::Other("unknown".to_string()),
            os: OsInfo::default(),
            virtualization: VirtualizationInfo::default(),
            windows: None,
            linux: None,
            gpus: Vec::new(),
        }
    }

    /// Free bytes on the roomiest fixed volume.
    ///
    /// Docker's images and containers land on one volume, and which one is a
    /// setting we do not read. The roomiest fixed volume is the right question
    /// for tiering: if any of them has space, disk is not what constrains this
    /// machine. Removable volumes are excluded.
    pub fn largest_fixed_free_bytes(&self) -> Option<u64> {
        self.volumes
            .iter()
            .filter(|volume| !volume.removable)
            .map(|volume| volume.free_bytes)
            .max()
    }
}
```

In `crates/platform/src/lib.rs`, add beside the existing module declarations:

```rust
pub mod snapshot;

pub use snapshot::{
    Architecture, CpuInfo, GpuInfo, LinuxInfo, MemoryInfo, OsInfo, PackageManager, StorageKind,
    SystemSnapshot, VirtualizationInfo, VolumeInfo, WindowsInfo,
};
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p project-host-platform snapshot`
Expected: PASS, 3 tests.

- [ ] **Step 5: Verify and commit**

```bash
pnpm verify
git add crates/platform
git commit -m "Add SystemSnapshot data model"
```

---

### Task 2: The probe seam and a fake

The trait and the assembly point, with no real probing behind them yet. This exists before any OS call so that every consumer can be written and tested against a constructed machine.

**Files:**

- Create: `crates/platform/src/probe/mod.rs`
- Modify: `crates/platform/src/lib.rs`

**Interfaces:**

- Consumes: `SystemSnapshot` and its field types from Task 1.
- Produces: `trait SystemProbe { fn snapshot(&self) -> SystemSnapshot; }`, `SystemProbe` implementations `SystemScanner` (real, stubbed in this task) and `FixedProbe` (test double wrapping a `SystemSnapshot`).

- [ ] **Step 1: Write the failing test**

Append to `crates/platform/src/probe/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_probe_returns_exactly_what_it_was_given() {
        // The seam that makes every downstream decision testable: results must
        // never depend on the machine running the test.
        let mut machine = SystemSnapshot::unknown();
        machine.cpu.logical_cores = Some(2);
        machine.memory.total_bytes = Some(4 * 1024 * 1024 * 1024);

        let probe = FixedProbe::new(machine.clone());
        assert_eq!(probe.snapshot(), machine);
    }

    #[test]
    fn the_real_scanner_always_produces_a_snapshot() {
        // No failure case. Whatever this machine is, and whatever refuses to
        // answer, a snapshot comes back.
        let snapshot = SystemScanner.snapshot();
        assert!(
            !matches!(snapshot.arch, Architecture::Other(ref name) if name == "unknown"),
            "the architecture is always knowable from the compiled target"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-platform probe`
Expected: FAIL — `probe` module does not exist.

- [ ] **Step 3: Implement**

Create `crates/platform/src/probe/mod.rs`, above the test module:

```rust
//! Reading this machine.
//!
//! One submodule per group of facts, each the only place that knows how its
//! group is read on a given OS. This module assembles them into a
//! [`SystemSnapshot`] and is the crate's only public entry point for scanning.
//!
//! Probing goes through [`SystemProbe`] so that consumers depend on the seam
//! rather than on the machine. `crates/compatibility` never calls a probe at
//! all — it takes the resulting value.
//!
//! **The workspace forbids `unsafe`**, so nothing here uses FFI. Facts come
//! from `sysinfo` or from a subprocess, which is safe and sufficient.

use crate::snapshot::{Architecture, SystemSnapshot};

/// Something that can describe this machine.
pub trait SystemProbe: Send + Sync + std::fmt::Debug {
    fn snapshot(&self) -> SystemSnapshot;
}

/// Reads the real machine. The only implementation used in production.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemScanner;

impl SystemProbe for SystemScanner {
    fn snapshot(&self) -> SystemSnapshot {
        let mut snapshot = SystemSnapshot::unknown();
        // Always knowable: it is the target this binary was compiled for.
        snapshot.arch = Architecture::from_target(std::env::consts::ARCH);
        // Later tasks fill the remaining groups in.
        snapshot
    }
}

/// Returns a snapshot decided by the test, so results never depend on the
/// machine running them.
#[derive(Debug, Clone)]
pub struct FixedProbe(SystemSnapshot);

impl FixedProbe {
    pub fn new(snapshot: SystemSnapshot) -> Self {
        Self(snapshot)
    }
}

impl SystemProbe for FixedProbe {
    fn snapshot(&self) -> SystemSnapshot {
        self.0.clone()
    }
}
```

In `crates/platform/src/lib.rs` add:

```rust
pub mod probe;

pub use probe::{FixedProbe, SystemProbe, SystemScanner};
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p project-host-platform probe`
Expected: PASS, 2 tests.

- [ ] **Step 5: Verify and commit**

```bash
pnpm verify
git add crates/platform
git commit -m "Add the system probe seam"
```

---

### Task 3: CPU and memory probe

**Files:**

- Create: `crates/platform/src/probe/hardware.rs`
- Modify: `crates/platform/src/probe/mod.rs`
- Modify: `crates/platform/Cargo.toml`

**Interfaces:**

- Consumes: `CpuInfo`, `MemoryInfo` from Task 1.
- Produces: `pub(crate) fn read_cpu(system: &sysinfo::System) -> CpuInfo`, `pub(crate) fn read_memory(system: &sysinfo::System) -> MemoryInfo`.

- [ ] **Step 1: Add the dependency**

Run: `cargo add sysinfo -p project-host-platform`

`sysinfo` is chosen because it is pure safe Rust with no FFI of our own, which is the only option compatible with `unsafe_code = "forbid"`, and it covers CPU counts, memory and disks on both target platforms behind one API. Let `cargo add` pick the current version rather than pinning one by hand; `Cargo.lock` records it.

- [ ] **Step 2: Write the failing test**

Append to `crates/platform/src/probe/hardware.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reports_cores_and_memory() {
        // Any machine that can run this test has at least one core and some
        // memory. Asserting specific values would assert the test runner's
        // hardware, which is why every *decision* is tested against a
        // constructed snapshot instead.
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        system.refresh_cpu_all();

        let cpu = read_cpu(&system);
        let memory = read_memory(&system);

        assert!(cpu.logical_cores.is_some_and(|cores| cores >= 1));
        assert!(memory.total_bytes.is_some_and(|bytes| bytes > 0));
        assert!(
            memory.available_bytes.is_some_and(|free| free
                <= memory.total_bytes.unwrap_or(u64::MAX)),
            "available memory cannot exceed total"
        );
    }

    #[test]
    fn a_zero_core_count_is_reported_as_unknown() {
        // sysinfo returns 0 rather than an error when it cannot tell. A 0 that
        // reached the tier would read as the weakest possible machine for a
        // reason that is a measurement failure, not a hardware fact.
        assert_eq!(non_zero(0), None);
        assert_eq!(non_zero(8), Some(8));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p project-host-platform hardware`
Expected: FAIL — `hardware` module does not exist.

- [ ] **Step 4: Implement**

Create `crates/platform/src/probe/hardware.rs`, above the test module:

```rust
//! CPU and memory counts, from `sysinfo`.

use crate::snapshot::{CpuInfo, MemoryInfo};

/// `sysinfo` reports 0 for a count it could not determine. Zero is not a
/// hardware fact, and letting it through would classify a measurement failure
/// as the weakest possible machine.
pub(crate) fn non_zero(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

pub(crate) fn read_cpu(system: &sysinfo::System) -> CpuInfo {
    let first = system.cpus().first();
    CpuInfo {
        vendor: first.map(|cpu| cpu.vendor_id().to_string()),
        model: first.map(|cpu| cpu.brand().trim().to_string()),
        physical_cores: system
            .physical_core_count()
            .and_then(|count| u32::try_from(count).ok())
            .and_then(non_zero),
        logical_cores: u32::try_from(system.cpus().len())
            .ok()
            .and_then(non_zero),
    }
}

pub(crate) fn read_memory(system: &sysinfo::System) -> MemoryInfo {
    MemoryInfo {
        total_bytes: (system.total_memory() > 0).then(|| system.total_memory()),
        available_bytes: (system.total_memory() > 0).then(|| system.available_memory()),
    }
}
```

In `crates/platform/src/probe/mod.rs`, add `mod hardware;` at the top and replace the body of `SystemScanner::snapshot`:

```rust
    fn snapshot(&self) -> SystemSnapshot {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        system.refresh_cpu_all();

        let mut snapshot = SystemSnapshot::unknown();
        snapshot.arch = Architecture::from_target(std::env::consts::ARCH);
        snapshot.cpu = hardware::read_cpu(&system);
        snapshot.memory = hardware::read_memory(&system);
        snapshot
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p project-host-platform`
Expected: PASS.

- [ ] **Step 6: Verify and commit**

```bash
pnpm verify
git add crates/platform Cargo.lock
git commit -m "Probe CPU and memory"
```

---

### Task 4: Storage probe

**Files:**

- Create: `crates/platform/src/probe/storage.rs`
- Modify: `crates/platform/src/probe/mod.rs`

**Interfaces:**

- Consumes: `VolumeInfo`, `StorageKind` from Task 1.
- Produces: `pub(crate) fn read_volumes(disks: &sysinfo::Disks) -> Vec<VolumeInfo>`.

- [ ] **Step 1: Write the failing test**

Append to `crates/platform/src/probe/storage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SystemSnapshot;

    #[test]
    fn this_machine_reports_at_least_one_volume() {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let volumes = read_volumes(&disks);
        assert!(!volumes.is_empty(), "a running machine has a filesystem");
        for volume in &volumes {
            assert!(
                volume.free_bytes <= volume.total_bytes,
                "{} reports more free than total",
                volume.mount_point
            );
        }
    }

    #[test]
    fn removable_volumes_are_excluded_from_capacity() {
        // A USB stick with 400 GB free must not make a machine look roomy.
        let mut snapshot = SystemSnapshot::unknown();
        snapshot.volumes = vec![
            VolumeInfo {
                mount_point: "C:\\".to_string(),
                total_bytes: 250_000_000_000,
                free_bytes: 10_000_000_000,
                removable: false,
                kind: StorageKind::Ssd,
            },
            VolumeInfo {
                mount_point: "E:\\".to_string(),
                total_bytes: 500_000_000_000,
                free_bytes: 400_000_000_000,
                removable: true,
                kind: StorageKind::Unknown,
            },
        ];
        assert_eq!(
            snapshot.largest_fixed_free_bytes(),
            Some(10_000_000_000),
            "the removable volume must not count"
        );
    }

    #[test]
    fn a_machine_with_no_volumes_reports_unknown_capacity() {
        assert_eq!(SystemSnapshot::unknown().largest_fixed_free_bytes(), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-platform storage`
Expected: FAIL — `storage` module does not exist.

- [ ] **Step 3: Implement**

Create `crates/platform/src/probe/storage.rs`, above the test module:

```rust
//! Volumes, their capacity and their kind.

use crate::snapshot::{StorageKind, VolumeInfo};

pub(crate) fn read_volumes(disks: &sysinfo::Disks) -> Vec<VolumeInfo> {
    disks
        .list()
        .iter()
        .map(|disk| VolumeInfo {
            mount_point: disk.mount_point().display().to_string(),
            total_bytes: disk.total_space(),
            free_bytes: disk.available_space(),
            removable: disk.is_removable(),
            kind: match disk.kind() {
                sysinfo::DiskKind::SSD => StorageKind::Ssd,
                sysinfo::DiskKind::HDD => StorageKind::Hdd,
                // Reported rather than guessed. Storage kind informs nothing
                // that must be decided, so an unknown one is not worth a probe
                // that could be wrong.
                _ => StorageKind::Unknown,
            },
        })
        .collect()
}
```

In `crates/platform/src/probe/mod.rs`, add `mod storage;` and add to `SystemScanner::snapshot` before the final `snapshot`:

```rust
        let disks = sysinfo::Disks::new_with_refreshed_list();
        snapshot.volumes = storage::read_volumes(&disks);
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p project-host-platform`
Expected: PASS.

- [ ] **Step 5: Verify and commit**

```bash
pnpm verify
git add crates/platform
git commit -m "Probe storage volumes"
```

---

### Task 5: Operating system identity

The build number lands here. It is the field that decides which Docker backend is available, so it gets its own task and its own parsing tests.

**Files:**

- Create: `crates/platform/src/probe/os.rs`
- Modify: `crates/platform/src/probe/mod.rs`

**Interfaces:**

- Consumes: `OsInfo`, `LinuxInfo`, `PackageManager` from Task 1.
- Produces: `pub(crate) fn read_os(system_name: Option<String>, kernel: Option<String>, version: Option<String>) -> OsInfo`, `pub(crate) fn parse_build(version: &str) -> Option<u32>`, `pub(crate) fn parse_os_release(contents: &str) -> LinuxInfo`, `pub(crate) fn package_manager_for(distro_id: &str) -> Option<PackageManager>`.

- [ ] **Step 1: Write the failing test**

Append to `crates/platform/src/probe/os.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_version_yields_its_build_number() {
        // The number that decides whether the WSL2 backend exists.
        assert_eq!(parse_build("10.0.26200"), Some(26200));
        assert_eq!(parse_build("10.0.19045"), Some(19045));
    }

    #[test]
    fn an_unparseable_version_yields_no_build_rather_than_a_guess() {
        // A wrong build number selects a wrong Docker backend, which is the
        // exact failure this whole design exists to prevent.
        assert_eq!(parse_build(""), None);
        assert_eq!(parse_build("10.0"), None);
        assert_eq!(parse_build("Windows"), None);
        assert_eq!(parse_build("10.0.notanumber"), None);
    }

    #[test]
    fn os_release_is_parsed_into_a_distro_and_manager() {
        let contents = "\
NAME=\"Ubuntu\"
VERSION_ID=\"24.04\"
ID=ubuntu
PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"
";
        let info = parse_os_release(contents);
        assert_eq!(info.distro_id.as_deref(), Some("ubuntu"));
        assert_eq!(info.version_id.as_deref(), Some("24.04"));
        assert_eq!(info.package_manager, Some(PackageManager::Apt));
    }

    #[test]
    fn os_release_quotes_are_stripped_and_unknown_keys_ignored() {
        let info = parse_os_release("ID=\"fedora\"\nVERSION_ID=41\nUNRELATED=x\n");
        assert_eq!(info.distro_id.as_deref(), Some("fedora"));
        assert_eq!(info.version_id.as_deref(), Some("41"));
        assert_eq!(info.package_manager, Some(PackageManager::Dnf));
    }

    #[test]
    fn an_unknown_distro_gets_no_package_manager() {
        // Guessing apt for an unknown distro would produce a command that fails
        // in a way the user cannot interpret.
        assert_eq!(package_manager_for("plan9"), None);
        assert_eq!(parse_os_release("").distro_id, None);
    }

    #[test]
    fn known_distros_map_to_their_managers() {
        assert_eq!(package_manager_for("debian"), Some(PackageManager::Apt));
        assert_eq!(package_manager_for("rhel"), Some(PackageManager::Dnf));
        assert_eq!(package_manager_for("arch"), Some(PackageManager::Pacman));
        assert_eq!(package_manager_for("opensuse"), Some(PackageManager::Zypper));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-platform probe::os`
Expected: FAIL — `os` module does not exist.

- [ ] **Step 3: Implement**

Create `crates/platform/src/probe/os.rs`, above the test module:

```rust
//! Which operating system this is, and precisely which version.

use crate::snapshot::{LinuxInfo, OsInfo, PackageManager};

/// The third dotted component of a Windows version string.
///
/// Returns `None` rather than a default. A wrong build number selects a wrong
/// Docker backend, so "unknown" must stay distinguishable from "old".
pub(crate) fn parse_build(version: &str) -> Option<u32> {
    version.split('.').nth(2)?.trim().parse().ok()
}

pub(crate) fn package_manager_for(distro_id: &str) -> Option<PackageManager> {
    match distro_id.trim().to_ascii_lowercase().as_str() {
        "ubuntu" | "debian" | "linuxmint" | "pop" | "raspbian" => Some(PackageManager::Apt),
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" => Some(PackageManager::Dnf),
        "arch" | "manjaro" | "endeavouros" => Some(PackageManager::Pacman),
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" => {
            Some(PackageManager::Zypper)
        }
        // Guessing a manager for an unrecognised distro would produce a command
        // that fails in a way the user cannot interpret.
        _ => None,
    }
}

/// Parse `/etc/os-release`, whose values may or may not be quoted.
pub(crate) fn parse_os_release(contents: &str) -> LinuxInfo {
    let value_of = |wanted: &str| -> Option<String> {
        contents.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == wanted)
                .then(|| value.trim().trim_matches('"').to_string())
                .filter(|value| !value.is_empty())
        })
    };

    let distro_id = value_of("ID");
    LinuxInfo {
        package_manager: distro_id.as_deref().and_then(package_manager_for),
        version_id: value_of("VERSION_ID"),
        distro_id,
    }
}

pub(crate) fn read_os(
    system_name: Option<String>,
    kernel: Option<String>,
    version: Option<String>,
) -> OsInfo {
    OsInfo {
        build: version.as_deref().and_then(parse_build),
        name: system_name,
        // Edition needs a platform-specific read and is filled in by
        // `probe::windows`. On Linux there is no such concept.
        edition: None,
        version,
        kernel,
    }
}
```

In `crates/platform/src/probe/mod.rs` add `mod os;` and, in `SystemScanner::snapshot`:

```rust
        snapshot.os = os::read_os(
            sysinfo::System::name(),
            sysinfo::System::kernel_version(),
            sysinfo::System::os_version(),
        );

        #[cfg(unix)]
        {
            snapshot.linux = Some(
                std::fs::read_to_string("/etc/os-release")
                    .map(|contents| os::parse_os_release(&contents))
                    .unwrap_or_default(),
            );
        }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p project-host-platform`
Expected: PASS.

- [ ] **Step 5: Verify and commit**

```bash
pnpm verify
git add crates/platform
git commit -m "Probe operating system identity"
```

---

### Task 6: Virtualization, WSL, edition and GPU

The platform-specific reads. All `#[cfg]` in this plan lives in this one file. Every parser is separated from every subprocess call, so the parsing is tested on any machine and only the invocation is unverifiable.

**Files:**

- Create: `crates/platform/src/probe/platform_specific.rs`
- Modify: `crates/platform/src/probe/mod.rs`

**Interfaces:**

- Consumes: `VirtualizationInfo`, `WindowsInfo`, `GpuInfo` from Task 1.
- Produces: `pub(crate) fn parse_virtualization_csv(stdout: &str) -> VirtualizationInfo`, `pub(crate) fn parse_gpu_lines(stdout: &str) -> Vec<GpuInfo>`, `pub(crate) fn parse_cpuinfo_flags(contents: &str) -> VirtualizationInfo`, `pub(crate) fn parse_wsl_status(stdout: &str) -> WindowsInfo`, `pub(crate) fn enrich(snapshot: &mut SystemSnapshot)`.

- [ ] **Step 1: Write the failing test**

Append to `crates/platform/src/probe/platform_specific.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_virtualization_output_is_parsed() {
        // `VirtualizationFirmwareEnabled,HypervisorPresent` from Win32_ComputerSystem
        // and Win32_Processor, emitted as one CSV line by the PowerShell command.
        let info = parse_virtualization_csv("True,False");
        assert_eq!(info.supported, Some(true));
        assert_eq!(info.enabled, Some(true));
        assert_eq!(info.hypervisor_present, Some(false));
    }

    #[test]
    fn virtualization_supported_but_disabled_is_distinguished_from_absent() {
        // These two produce completely different advice: one is a reboot into
        // firmware, the other has no fix on this machine at all.
        let disabled = parse_virtualization_csv("False,False");
        assert_eq!(disabled.enabled, Some(false));

        // When a hypervisor is already running, Windows reports the firmware
        // flag as False even though virtualization plainly works. Trusting it
        // would tell a working machine to reboot into its BIOS.
        let running = parse_virtualization_csv("False,True");
        assert_eq!(running.enabled, Some(true));
        assert_eq!(running.hypervisor_present, Some(true));
    }

    #[test]
    fn unreadable_virtualization_output_is_unknown_rather_than_false() {
        let info = parse_virtualization_csv("");
        assert_eq!(info.enabled, None);
        assert_eq!(parse_virtualization_csv("nonsense").enabled, None);
    }

    #[test]
    fn cpuinfo_flags_reveal_virtualization_support() {
        let intel = parse_cpuinfo_flags("flags\t: fpu vme de pse vmx est tm2\n");
        assert_eq!(intel.supported, Some(true));

        let amd = parse_cpuinfo_flags("flags\t: fpu vme de pse svm nx\n");
        assert_eq!(amd.supported, Some(true));

        let neither = parse_cpuinfo_flags("flags\t: fpu vme de pse\n");
        assert_eq!(neither.supported, Some(false));

        assert_eq!(parse_cpuinfo_flags("").supported, None);
    }

    #[test]
    fn wsl_status_reports_its_default_version() {
        let status = parse_wsl_status("Default Version: 2\nDefault Distribution: Ubuntu\n");
        assert!(status.wsl_present);
        assert_eq!(status.wsl_version, Some(2));
        assert_eq!(status.default_distro.as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn absent_wsl_is_reported_as_absent() {
        let status = parse_wsl_status("");
        assert!(!status.wsl_present);
        assert_eq!(status.wsl_version, None);
    }

    #[test]
    fn gpu_lines_become_entries_and_blanks_are_dropped() {
        let gpus = parse_gpu_lines("NVIDIA GeForce RTX 4070\n\nIntel(R) UHD Graphics\n");
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].vendor.as_deref(), Some("NVIDIA"));
        assert_eq!(gpus[0].model.as_deref(), Some("NVIDIA GeForce RTX 4070"));
        assert_eq!(gpus[1].vendor.as_deref(), Some("Intel"));
        assert!(parse_gpu_lines("").is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-platform platform_specific`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement**

Create `crates/platform/src/probe/platform_specific.rs`, above the test module:

```rust
//! The reads that differ per operating system.
//!
//! **Every `#[cfg]` in the scan lives in this file.** Each parser is separated
//! from the subprocess that feeds it, so the parsing is tested on any machine
//! and only the invocation is platform-dependent.
//!
//! **Verified on Windows only.** The machine this was written on has no Linux
//! and no macOS; the `#[cfg(unix)]` invocations below have never been run.

use crate::snapshot::{GpuInfo, SystemSnapshot, VirtualizationInfo, WindowsInfo};

fn parse_bool(field: &str) -> Option<bool> {
    match field.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn vendor_of(model: &str) -> Option<String> {
    let lowered = model.to_ascii_lowercase();
    for (needle, vendor) in [
        ("nvidia", "NVIDIA"),
        ("amd", "AMD"),
        ("radeon", "AMD"),
        ("intel", "Intel"),
        ("apple", "Apple"),
    ] {
        if lowered.contains(needle) {
            return Some(vendor.to_string());
        }
    }
    None
}

/// `<VirtualizationFirmwareEnabled>,<HypervisorPresent>`.
///
/// Windows reports the firmware flag as `False` whenever a hypervisor is
/// already running, because the flag describes what firmware exposes to the
/// host rather than whether virtualization works. Trusting it unconditionally
/// would tell a machine already running Hyper-V to reboot into its BIOS, so a
/// present hypervisor is itself proof that virtualization is enabled.
pub(crate) fn parse_virtualization_csv(stdout: &str) -> VirtualizationInfo {
    let mut fields = stdout.trim().split(',');
    let firmware = fields.next().and_then(parse_bool);
    let hypervisor = fields.next().and_then(parse_bool);

    let enabled = match (firmware, hypervisor) {
        (_, Some(true)) => Some(true),
        (Some(flag), _) => Some(flag),
        (None, _) => None,
    };

    VirtualizationInfo {
        // On Windows the firmware flag only exists on a CPU that supports it,
        // and a running hypervisor proves support outright.
        supported: enabled.or(firmware),
        enabled,
        hypervisor_present: hypervisor,
    }
}

/// `vmx` (Intel) or `svm` (AMD) in `/proc/cpuinfo` flags.
pub(crate) fn parse_cpuinfo_flags(contents: &str) -> VirtualizationInfo {
    let Some(line) = contents
        .lines()
        .find(|line| line.trim_start().starts_with("flags"))
    else {
        return VirtualizationInfo::default();
    };
    let supported = line
        .split_whitespace()
        .any(|flag| flag == "vmx" || flag == "svm");

    VirtualizationInfo {
        supported: Some(supported),
        // A flag present in `/proc/cpuinfo` means the kernel can see the
        // feature, which on Linux means firmware has already enabled it.
        enabled: Some(supported),
        hypervisor_present: None,
    }
}

pub(crate) fn parse_wsl_status(stdout: &str) -> WindowsInfo {
    let value_after = |label: &str| -> Option<String> {
        stdout.lines().find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.trim().eq_ignore_ascii_case(label))
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    };

    let version = value_after("Default Version").and_then(|value| value.parse().ok());
    WindowsInfo {
        wsl_present: !stdout.trim().is_empty(),
        wsl_version: version,
        default_distro: value_after("Default Distribution"),
    }
}

pub(crate) fn parse_gpu_lines(stdout: &str) -> Vec<GpuInfo> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|model| GpuInfo {
            vendor: vendor_of(model),
            model: Some(model.to_string()),
        })
        .collect()
}

/// Run a command and return its stdout, or `None` if it could not be run.
///
/// Never propagates a failure: a machine where PowerShell is unavailable still
/// gets a snapshot, with these fields left unknown.
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// Fill in the platform-specific groups. Failures leave fields unknown.
pub(crate) fn enrich(snapshot: &mut SystemSnapshot) {
    #[cfg(windows)]
    {
        // A subprocess rather than FFI: the workspace forbids `unsafe`, so
        // `WinVerifyTrust`-style direct calls are unavailable. This is the same
        // reasoning that chose `taskkill` in the host run mode design.
        if let Some(stdout) = output_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$c = Get-CimInstance Win32_ComputerSystem; \
                 $p = Get-CimInstance Win32_Processor | Select-Object -First 1; \
                 \"$($p.VirtualizationFirmwareEnabled),$($c.HypervisorPresent)\"",
            ],
        ) {
            snapshot.virtualization = parse_virtualization_csv(&stdout);
        }

        if let Some(stdout) = output_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_OperatingSystem).Caption",
            ],
        ) {
            snapshot.os.edition = Some(stdout.trim().to_string()).filter(|s| !s.is_empty());
        }

        if let Some(stdout) = output_of(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_VideoController | \
                 Select-Object -ExpandProperty Name",
            ],
        ) {
            snapshot.gpus = parse_gpu_lines(&stdout);
        }

        snapshot.windows = Some(
            output_of("wsl", &["--status"])
                .map(|stdout| parse_wsl_status(&stdout))
                .unwrap_or_default(),
        );
    }

    #[cfg(unix)]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/cpuinfo") {
            snapshot.virtualization = parse_cpuinfo_flags(&contents);
        }
        if let Some(stdout) = output_of("sh", &["-c", "lspci | grep -i vga"]) {
            snapshot.gpus = parse_gpu_lines(&stdout);
        }
    }
}
```

In `crates/platform/src/probe/mod.rs` add `mod platform_specific;` and, as the last line before returning from `SystemScanner::snapshot`:

```rust
        platform_specific::enrich(&mut snapshot);
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p project-host-platform`
Expected: PASS.

- [ ] **Step 5: Verify and commit**

```bash
pnpm verify
git add crates/platform
git commit -m "Probe virtualization, WSL, edition and GPU"
```

---

### Task 7: The `compatibility` crate and the golden set

The synthetic machines every later task tests against. They land before the logic that consumes them.

**Files:**

- Create: `crates/compatibility/Cargo.toml`
- Create: `crates/compatibility/src/lib.rs`
- Create: `crates/compatibility/src/machines.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**

- Consumes: `SystemSnapshot` and its field types from Task 1.
- Produces: `machines::GOLDEN_SET` as `fn golden_set() -> Vec<(&'static str, SystemSnapshot)>`, plus named constructors `windows_11_workstation()`, `windows_11_low_end()`, `windows_on_arm()`, `windows_virtualization_disabled()`, `windows_full_disk()`, `ubuntu_desktop()`, `knows_nothing()`.

- [ ] **Step 1: Create the crate**

`crates/compatibility/Cargo.toml`:

```toml
[package]
name = "project-host-compatibility"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
project-host-platform = { path = "../platform" }
serde = { version = "1.0.229", features = ["derive"] }
thiserror = "2.0.19"

[lints]
workspace = true

[dev-dependencies]
serde_json = "1.0.151"
```

Add `"crates/compatibility",` to the `members` list in the workspace `Cargo.toml`, after `"crates/platform",`.

`crates/compatibility/src/lib.rs`:

```rust
//! Whether this machine can run Docker, how well, and which one.
//!
//! Everything here is a pure function of a [`SystemSnapshot`]. This crate does
//! no I/O, spawns no process and reads no file, which is what lets every
//! decision be tested against a machine that was constructed rather than one
//! that happens to be running the test.
//!
//! It depends on `project-host-platform` for the snapshot type and on nothing
//! else — not `docker-manager`, not `api-types` — for the same reason
//! `detection` and `host-runner` do: it should be possible to reason about
//! whether a machine can run Docker without also holding the container model,
//! or the wire format, in mind.
//!
//! [`SystemSnapshot`]: project_host_platform::SystemSnapshot

// Tests are allowed to unwrap and slice; production paths in this workspace are
// not. A panic in a test is a failed test — in the agent it is a stopped service.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod machines;
```

- [ ] **Step 2: Write the failing test**

Append to `crates/compatibility/src/machines.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_golden_set_covers_every_case_the_design_names() {
        let names: Vec<&str> = golden_set().iter().map(|(name, _)| *name).collect();
        for expected in [
            "windows-11-workstation",
            "windows-11-low-end",
            "windows-on-arm",
            "windows-virtualization-disabled",
            "windows-full-disk",
            "ubuntu-desktop",
            "knows-nothing",
        ] {
            assert!(names.contains(&expected), "{expected} is missing");
        }
    }

    #[test]
    fn the_low_end_machine_is_actually_low_end() {
        // If this drifts upward the tier tests stop testing what they claim to.
        let machine = windows_11_low_end();
        assert!(machine.cpu.logical_cores.unwrap() < 4);
        assert!(machine.memory.total_bytes.unwrap() < 8 * GB);
    }

    #[test]
    fn the_workstation_is_actually_capable() {
        let machine = windows_11_workstation();
        assert!(machine.cpu.logical_cores.unwrap() > 8);
        assert!(machine.memory.total_bytes.unwrap() > 16 * GB);
        assert!(machine.largest_fixed_free_bytes().unwrap() > 20 * GB);
    }

    #[test]
    fn the_disabled_machine_supports_virtualization_but_has_it_off() {
        // The distinction the whole firmware-blocker path depends on.
        let machine = windows_virtualization_disabled();
        assert_eq!(machine.virtualization.supported, Some(true));
        assert_eq!(machine.virtualization.enabled, Some(false));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p project-host-compatibility machines`
Expected: FAIL — `machines` module does not exist.

- [ ] **Step 4: Implement**

Create `crates/compatibility/src/machines.rs`, above the test module:

```rust
//! Synthetic machines.
//!
//! Every decision in this crate is tested against these rather than against the
//! machine running the tests. They are `pub` because the tier and selection
//! tests in sibling modules use them, and because a new artefact added to the
//! catalogue must be checked against the whole set.

use project_host_platform::{
    Architecture, CpuInfo, LinuxInfo, MemoryInfo, OsInfo, PackageManager, StorageKind,
    SystemSnapshot, VirtualizationInfo, VolumeInfo, WindowsInfo,
};

pub const GB: u64 = 1024 * 1024 * 1024;

fn volume(mount: &str, total_gb: u64, free_gb: u64) -> VolumeInfo {
    VolumeInfo {
        mount_point: mount.to_string(),
        total_bytes: total_gb * GB,
        free_bytes: free_gb * GB,
        removable: false,
        kind: StorageKind::Ssd,
    }
}

fn windows_base() -> SystemSnapshot {
    SystemSnapshot {
        arch: Architecture::X86_64,
        os: OsInfo {
            name: Some("Windows".to_string()),
            edition: Some("Windows 11 Pro".to_string()),
            version: Some("10.0.26200".to_string()),
            build: Some(26200),
            kernel: None,
        },
        virtualization: VirtualizationInfo {
            supported: Some(true),
            enabled: Some(true),
            hypervisor_present: Some(false),
        },
        windows: Some(WindowsInfo {
            wsl_present: true,
            wsl_version: Some(2),
            default_distro: Some("Ubuntu".to_string()),
        }),
        ..SystemSnapshot::unknown()
    }
}

/// 16 logical cores, 32 GB, plenty of disk.
pub fn windows_11_workstation() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("GenuineIntel".to_string()),
            model: Some("Intel(R) Core(TM) i9-13900K".to_string()),
            physical_cores: Some(8),
            logical_cores: Some(16),
        },
        memory: MemoryInfo {
            total_bytes: Some(32 * GB),
            available_bytes: Some(20 * GB),
        },
        volumes: vec![volume("C:\\", 2000, 1200)],
        ..windows_base()
    }
}

/// A 2015-era laptop: 2 logical cores, 4 GB.
pub fn windows_11_low_end() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("GenuineIntel".to_string()),
            model: Some("Intel(R) Core(TM) i3-5005U".to_string()),
            physical_cores: Some(2),
            logical_cores: Some(2),
        },
        memory: MemoryInfo {
            total_bytes: Some(4 * GB),
            available_bytes: Some(1 * GB),
        },
        volumes: vec![VolumeInfo {
            kind: StorageKind::Hdd,
            ..volume("C:\\", 250, 60)
        }],
        ..windows_base()
    }
}

/// Capable in every respect, on an architecture with no Docker Desktop build.
pub fn windows_on_arm() -> SystemSnapshot {
    SystemSnapshot {
        arch: Architecture::Aarch64,
        ..windows_11_workstation()
    }
}

/// Supported by the CPU, switched off in firmware. The reboot-into-BIOS case.
pub fn windows_virtualization_disabled() -> SystemSnapshot {
    SystemSnapshot {
        virtualization: VirtualizationInfo {
            supported: Some(true),
            enabled: Some(false),
            hypervisor_present: Some(false),
        },
        ..windows_11_workstation()
    }
}

/// Capable, but with 4 GB free.
pub fn windows_full_disk() -> SystemSnapshot {
    SystemSnapshot {
        volumes: vec![volume("C:\\", 500, 4)],
        ..windows_11_workstation()
    }
}

/// 8 logical cores, 16 GB, apt.
pub fn ubuntu_desktop() -> SystemSnapshot {
    SystemSnapshot {
        cpu: CpuInfo {
            vendor: Some("AuthenticAMD".to_string()),
            model: Some("AMD Ryzen 7 5800X".to_string()),
            physical_cores: Some(8),
            logical_cores: Some(8),
        },
        memory: MemoryInfo {
            total_bytes: Some(16 * GB),
            available_bytes: Some(9 * GB),
        },
        volumes: vec![volume("/", 1000, 400)],
        arch: Architecture::X86_64,
        os: OsInfo {
            name: Some("Ubuntu".to_string()),
            version: Some("24.04".to_string()),
            kernel: Some("6.8.0-40-generic".to_string()),
            ..OsInfo::default()
        },
        virtualization: VirtualizationInfo {
            supported: Some(true),
            enabled: Some(true),
            hypervisor_present: Some(false),
        },
        linux: Some(LinuxInfo {
            distro_id: Some("ubuntu".to_string()),
            version_id: Some("24.04".to_string()),
            package_manager: Some(PackageManager::Apt),
        }),
        ..SystemSnapshot::unknown()
    }
}

/// Answers no probe at all. Every decision must still produce a defensible
/// result rather than a panic or an optimistic guess.
pub fn knows_nothing() -> SystemSnapshot {
    SystemSnapshot::unknown()
}

/// Every machine, by name. New artefacts are checked against all of them.
pub fn golden_set() -> Vec<(&'static str, SystemSnapshot)> {
    vec![
        ("windows-11-workstation", windows_11_workstation()),
        ("windows-11-low-end", windows_11_low_end()),
        ("windows-on-arm", windows_on_arm()),
        (
            "windows-virtualization-disabled",
            windows_virtualization_disabled(),
        ),
        ("windows-full-disk", windows_full_disk()),
        ("ubuntu-desktop", ubuntu_desktop()),
        ("knows-nothing", knows_nothing()),
    ]
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p project-host-compatibility`
Expected: PASS, 4 tests.

- [ ] **Step 6: Verify and commit**

```bash
pnpm verify
git add crates/compatibility Cargo.toml Cargo.lock
git commit -m "Add the compatibility crate and its golden machines"
```

---

### Task 8: Tier and resource defaults

**Files:**

- Create: `crates/compatibility/src/tier.rs`
- Modify: `crates/compatibility/src/lib.rs`

**Interfaces:**

- Consumes: `SystemSnapshot` (Task 1), `machines::golden_set` and `GB` (Task 7).
- Produces: `PerformanceTier::{Minimal, Standard, Performance}`, `ResourceDefaults { memory_limit_mb: i64, cpu_limit_cores: f64, process_limit: i64 }`, `Assessment { tier, defaults }`, `pub fn assess(snapshot: &SystemSnapshot) -> Assessment`.

- [ ] **Step 1: Write the failing test**

Append to `crates/compatibility/src/tier.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::machines::{self, GB};

    #[test]
    fn each_golden_machine_gets_the_tier_it_deserves() {
        assert_eq!(
            assess(&machines::windows_11_workstation()).tier,
            PerformanceTier::Performance
        );
        assert_eq!(
            assess(&machines::ubuntu_desktop()).tier,
            PerformanceTier::Standard
        );
        assert_eq!(
            assess(&machines::windows_11_low_end()).tier,
            PerformanceTier::Minimal
        );
    }

    #[test]
    fn the_tier_is_the_weakest_axis_not_an_average() {
        // A 32-core machine with 6 GB of RAM is a Minimal machine. Averaging
        // would hand it defaults it cannot honour.
        let mut machine = machines::windows_11_workstation();
        machine.memory.total_bytes = Some(6 * GB);
        assert_eq!(assess(&machine).tier, PerformanceTier::Minimal);
    }

    #[test]
    fn a_full_disk_drops_a_capable_machine_to_minimal() {
        assert_eq!(
            assess(&machines::windows_full_disk()).tier,
            PerformanceTier::Minimal
        );
    }

    #[test]
    fn an_unknown_axis_is_treated_as_its_weakest_value() {
        // A machine that will not say how much memory it has is not assumed to
        // have plenty.
        assert_eq!(
            assess(&machines::knows_nothing()).tier,
            PerformanceTier::Minimal
        );
    }

    #[test]
    fn no_default_exceeds_an_eighth_of_total_memory() {
        // The invariant that outranks the table: it is what makes the round
        // numbers safe on a machine the table did not anticipate.
        for (name, machine) in machines::golden_set() {
            let assessment = assess(&machine);
            if let Some(total) = machine.memory.total_bytes {
                let cap_mb = (total / 8 / 1024 / 1024) as i64;
                assert!(
                    assessment.defaults.memory_limit_mb <= cap_mb.max(MIN_MEMORY_MB),
                    "{name}: {} MB exceeds the 12.5% cap of {cap_mb} MB",
                    assessment.defaults.memory_limit_mb
                );
            }
        }
    }

    #[test]
    fn every_default_satisfies_the_schema_constraints() {
        // 0001_initial.sql lines 123-126. A default the CHECK rejects fails at
        // the moment a user presses Create.
        for (name, machine) in machines::golden_set() {
            let defaults = assess(&machine).defaults;
            assert!(
                (64..=65536).contains(&defaults.memory_limit_mb),
                "{name}: memory {}",
                defaults.memory_limit_mb
            );
            assert!(
                defaults.cpu_limit_cores > 0.0 && defaults.cpu_limit_cores <= 64.0,
                "{name}: cpu {}",
                defaults.cpu_limit_cores
            );
            assert!(
                (8..=4096).contains(&defaults.process_limit),
                "{name}: pids {}",
                defaults.process_limit
            );
        }
    }

    #[test]
    fn a_tiny_machine_still_gets_a_usable_floor() {
        // The 12.5% cap must never drive a default below what the schema allows
        // or below what any container could start with.
        let mut machine = machines::windows_11_low_end();
        machine.memory.total_bytes = Some(GB / 2);
        let defaults = assess(&machine).defaults;
        assert_eq!(defaults.memory_limit_mb, MIN_MEMORY_MB);
    }

    #[test]
    fn cpu_model_does_not_change_the_tier() {
        // Release year is not an input, deliberately. A 2013 Xeon with 64 GB
        // outruns a 2023 Celeron with 4 GB.
        let mut old = machines::windows_11_workstation();
        old.cpu.model = Some("Intel(R) Xeon(R) CPU E5-2670 0 @ 2.60GHz".to_string());
        assert_eq!(
            assess(&old).tier,
            assess(&machines::windows_11_workstation()).tier
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-compatibility tier`
Expected: FAIL — `tier` module does not exist.

- [ ] **Step 3: Implement**

Create `crates/compatibility/src/tier.rs`, above the test module:

```rust
//! How much this machine can be asked to do.
//!
//! A pure function of the snapshot, over three measured axes: logical cores,
//! total memory, and free space on the roomiest fixed volume.
//!
//! **CPU age is deliberately not an input.** Release year is a weak predictor
//! of throughput — a 2013 Xeon with 64 GB outruns a 2023 Celeron with 4 GB —
//! and tiering on it would misjudge precisely the low-end machines that most
//! need correct limits. It would also require a model-string-to-year table,
//! which is large, fuzzy, and stale the day it is written.

use project_host_platform::SystemSnapshot;
use serde::{Deserialize, Serialize};

/// The floor for a memory default, in MB. The schema's own minimum is 64
/// (`0001_initial.sql`), and the 12.5% cap must never drive a default below it.
pub const MIN_MEMORY_MB: i64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceTier {
    Minimal,
    Standard,
    Performance,
}

impl PerformanceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            PerformanceTier::Minimal => "MINIMAL",
            PerformanceTier::Standard => "STANDARD",
            PerformanceTier::Performance => "PERFORMANCE",
        }
    }
}

/// What a newly created project starts with. The column names match
/// `projects` exactly, so the call site cannot transpose two of them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResourceDefaults {
    pub memory_limit_mb: i64,
    pub cpu_limit_cores: f64,
    pub process_limit: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub tier: PerformanceTier,
    pub defaults: ResourceDefaults,
}

const GB: u64 = 1024 * 1024 * 1024;

/// The tier of one axis, then the whole machine takes the weakest.
fn tier_of_cores(cores: Option<u32>) -> PerformanceTier {
    match cores {
        Some(cores) if cores >= 8 => PerformanceTier::Performance,
        Some(cores) if cores >= 4 => PerformanceTier::Standard,
        // Unknown is the weakest value: a machine that will not say how many
        // cores it has is not assumed to have many.
        _ => PerformanceTier::Minimal,
    }
}

fn tier_of_memory(total: Option<u64>) -> PerformanceTier {
    match total {
        Some(bytes) if bytes >= 16 * GB => PerformanceTier::Performance,
        Some(bytes) if bytes >= 8 * GB => PerformanceTier::Standard,
        _ => PerformanceTier::Minimal,
    }
}

fn tier_of_disk(free: Option<u64>) -> PerformanceTier {
    match free {
        Some(bytes) if bytes >= 20 * GB => PerformanceTier::Performance,
        _ => PerformanceTier::Minimal,
    }
}

fn table_defaults(tier: PerformanceTier) -> ResourceDefaults {
    match tier {
        PerformanceTier::Minimal => ResourceDefaults {
            memory_limit_mb: 512,
            cpu_limit_cores: 0.5,
            process_limit: 128,
        },
        PerformanceTier::Standard => ResourceDefaults {
            memory_limit_mb: 1024,
            cpu_limit_cores: 1.0,
            process_limit: 256,
        },
        PerformanceTier::Performance => ResourceDefaults {
            memory_limit_mb: 2048,
            cpu_limit_cores: 2.0,
            process_limit: 512,
        },
    }
}

/// Tier this machine and produce the defaults a new project should start with.
pub fn assess(snapshot: &SystemSnapshot) -> Assessment {
    let tier = tier_of_cores(snapshot.cpu.logical_cores)
        .min(tier_of_memory(snapshot.memory.total_bytes))
        .min(tier_of_disk(snapshot.largest_fixed_free_bytes()));

    let mut defaults = table_defaults(tier);

    // The invariant that outranks the table. The table is a set of round
    // numbers chosen for legibility; this is what keeps them safe on a machine
    // the table did not anticipate.
    if let Some(total) = snapshot.memory.total_bytes {
        let cap_mb = i64::try_from(total / 8 / 1024 / 1024).unwrap_or(i64::MAX);
        defaults.memory_limit_mb = defaults.memory_limit_mb.min(cap_mb).max(MIN_MEMORY_MB);
    }

    Assessment { tier, defaults }
}
```

Add to `crates/compatibility/src/lib.rs`:

```rust
pub mod tier;

pub use tier::{assess, Assessment, PerformanceTier, ResourceDefaults};
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p project-host-compatibility tier`
Expected: PASS, 8 tests.

- [ ] **Step 5: Verify and commit**

```bash
pnpm verify
git add crates/compatibility
git commit -m "Tier a machine and derive its resource defaults"
```

---

### Task 9: Catalogue, blockers and selection

**Files:**

- Create: `crates/compatibility/src/blocker.rs`
- Create: `crates/compatibility/src/catalog.rs`
- Create: `crates/compatibility/src/select.rs`
- Modify: `crates/compatibility/src/lib.rs`

**Interfaces:**

- Consumes: `SystemSnapshot`, `Architecture`, `PackageManager` (Task 1); `machines::golden_set` (Task 7).
- Produces: `Blocker` enum; `DockerProduct::{DockerDesktop, DockerEngine}`; `Requirements`; `Artifact`; `pub fn catalog() -> &'static [Artifact]`; `Selection::{Artifact(&'static Artifact), Blocked(Vec<Blocker>)}`; `pub fn select(snapshot: &SystemSnapshot) -> Selection`; `pub fn unmet(requirements: &Requirements, snapshot: &SystemSnapshot) -> Vec<Blocker>`.

- [ ] **Step 1: Write the failing test for blockers**

Create `crates/compatibility/src/blocker.rs` with:

```rust
//! Why this machine cannot have the Docker it would otherwise get.
//!
//! Every variant names concrete values. "Virtualization is disabled" without
//! the key to press is the failure mode this design exists to replace.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Blocker {
    #[error(
        "Virtualization is supported by this CPU but switched off in firmware. \
         Restart, open firmware setup (usually F2, F10, Del or Esc during \
         startup — the key is shown on the first screen), and enable \
         {setting}. No application can change this setting."
    )]
    VirtualizationDisabled { setting: &'static str },

    #[error(
        "This CPU does not support hardware virtualization, which Docker \
         requires. There is no setting that changes this."
    )]
    VirtualizationUnsupported,

    #[error("No Docker build is published for {found} processors.")]
    ArchitectureUnsupported { found: String },

    #[error("This is {product}, which needs {name} build {required} or newer; this machine reports build {found}.")]
    OsTooOld {
        product: &'static str,
        name: &'static str,
        required: u32,
        found: u32,
    },

    #[error("Could not determine the operating system build, which decides which Docker backend applies. Nothing is installed on a guess.")]
    OsBuildUnknown,

    #[error("{found} does not provide {needed}.")]
    EditionUnsupported { found: String, needed: String },

    #[error("{needed_gb} GB of free space is required; the roomiest fixed volume has {found_gb} GB.")]
    InsufficientDisk { needed_gb: u64, found_gb: u64 },

    #[error("No supported package manager was found. Docker Engine is installed through apt, dnf, pacman or zypper.")]
    NoPackageManager,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_firmware_blocker_names_the_setting_and_how_to_reach_it() {
        // The distinction between this and VirtualizationUnsupported is the
        // difference between a fixable machine and an unfixable one.
        let message = Blocker::VirtualizationDisabled {
            setting: "Intel VT-x",
        }
        .to_string();
        assert!(message.contains("Intel VT-x"));
        assert!(message.contains("firmware"));
        assert!(
            message.contains("F2"),
            "naming a key is the difference between advice and a complaint"
        );
    }

    #[test]
    fn a_disk_blocker_names_both_numbers() {
        let message = Blocker::InsufficientDisk {
            needed_gb: 20,
            found_gb: 4,
        }
        .to_string();
        assert!(message.contains("20"));
        assert!(message.contains('4'));
    }

    #[test]
    fn an_os_blocker_names_the_build_found_and_the_build_required() {
        let message = Blocker::OsTooOld {
            product: "Docker Desktop",
            name: "Windows",
            required: 19045,
            found: 17763,
        }
        .to_string();
        assert!(message.contains("19045"));
        assert!(message.contains("17763"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p project-host-compatibility blocker`
Expected: FAIL — `blocker` module is not declared.

- [ ] **Step 3: Declare the module and confirm it passes**

Add `pub mod blocker;` and `pub use blocker::Blocker;` to `crates/compatibility/src/lib.rs`.

Run: `cargo test -p project-host-compatibility blocker`
Expected: PASS, 3 tests.

- [ ] **Step 4: Write the failing test for the catalogue and selection**

Create `crates/compatibility/src/select.rs` with only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::machines::{self, GB};
    use project_host_platform::{Architecture, SystemSnapshot};

    /// Re-checks each requirement directly, written separately from `unmet` on
    /// purpose. A property test that called `unmet` would be a tautology; this
    /// is an independent derivation, so the two disagreeing is a real signal.
    fn independently_satisfied(
        requirements: &Requirements,
        snapshot: &SystemSnapshot,
    ) -> bool {
        if !requirements.arch.contains(&snapshot.arch) {
            return false;
        }
        if requirements.requires_virtualization && snapshot.virtualization.enabled != Some(true) {
            return false;
        }
        if let Some(required) = requirements.min_os_build {
            match snapshot.os.build {
                Some(found) if found >= required => {}
                _ => return false,
            }
        }
        if let Some(editions) = requirements.required_editions {
            let Some(found) = snapshot.os.edition.as_deref() else {
                return false;
            };
            if !editions.iter().any(|wanted| found.contains(wanted)) {
                return false;
            }
        }
        if let Some(managers) = requirements.package_managers {
            let found = snapshot
                .linux
                .as_ref()
                .and_then(|linux| linux.package_manager);
            match found {
                Some(manager) if managers.contains(&manager) => {}
                _ => return false,
            }
        }
        snapshot.largest_fixed_free_bytes().unwrap_or(0) >= requirements.min_free_bytes
    }

    #[test]
    fn a_selected_artefact_is_always_one_the_machine_satisfies() {
        // This is the whole of "never install an incompatible version". A
        // property over the entire golden set, rather than a case per artefact,
        // is what keeps the guarantee true when an artefact is added later.
        for (name, machine) in machines::golden_set() {
            if let Selection::Artifact(artifact) = select(&machine) {
                assert!(
                    independently_satisfied(&artifact.requirements, &machine),
                    "{name} was offered {} but does not satisfy it",
                    artifact.id
                );
            }
        }
    }

    #[test]
    fn a_capable_windows_machine_is_offered_docker_desktop() {
        match select(&machines::windows_11_workstation()) {
            Selection::Artifact(artifact) => {
                assert_eq!(artifact.product, DockerProduct::DockerDesktop);
            }
            Selection::Blocked(blockers) => panic!("unexpectedly blocked: {blockers:?}"),
        }
    }

    #[test]
    fn ubuntu_is_offered_docker_engine() {
        match select(&machines::ubuntu_desktop()) {
            Selection::Artifact(artifact) => {
                assert_eq!(artifact.product, DockerProduct::DockerEngine);
            }
            Selection::Blocked(blockers) => panic!("unexpectedly blocked: {blockers:?}"),
        }
    }

    #[test]
    fn windows_on_arm_is_blocked_rather_than_given_the_x64_build() {
        let Selection::Blocked(blockers) = select(&machines::windows_on_arm()) else {
            panic!("an ARM machine must never be offered an x86_64 installer");
        };
        assert!(blockers
            .iter()
            .any(|blocker| matches!(blocker, Blocker::ArchitectureUnsupported { .. })));
    }

    #[test]
    fn disabled_virtualization_is_reported_as_fixable() {
        let Selection::Blocked(blockers) = select(&machines::windows_virtualization_disabled())
        else {
            panic!("virtualization is off; nothing should be offered");
        };
        assert!(
            blockers.iter().any(|blocker| matches!(
                blocker,
                Blocker::VirtualizationDisabled { .. }
            )),
            "got {blockers:?}"
        );
    }

    #[test]
    fn an_unsupported_cpu_is_distinguished_from_a_disabled_setting() {
        let mut machine = machines::windows_11_workstation();
        machine.virtualization.supported = Some(false);
        machine.virtualization.enabled = Some(false);
        let Selection::Blocked(blockers) = select(&machine) else {
            panic!("should be blocked");
        };
        assert!(blockers
            .iter()
            .any(|blocker| matches!(blocker, Blocker::VirtualizationUnsupported)));
    }

    #[test]
    fn a_full_disk_blocks_and_names_both_numbers() {
        let Selection::Blocked(blockers) = select(&machines::windows_full_disk()) else {
            panic!("4 GB free is not enough to install Docker");
        };
        assert!(blockers
            .iter()
            .any(|blocker| matches!(blocker, Blocker::InsufficientDisk { .. })));
    }

    #[test]
    fn an_old_windows_build_is_blocked_with_both_builds_named() {
        let mut machine = machines::windows_11_workstation();
        machine.os.build = Some(17763);
        machine.os.version = Some("10.0.17763".to_string());
        let Selection::Blocked(blockers) = select(&machine) else {
            panic!("build 17763 predates the WSL2 backend");
        };
        assert!(blockers
            .iter()
            .any(|blocker| matches!(blocker, Blocker::OsTooOld { .. })));
    }

    #[test]
    fn a_machine_that_knows_nothing_is_blocked_rather_than_guessed_at() {
        // The most important case: absence of evidence must never become an
        // install.
        let Selection::Blocked(blockers) = select(&machines::knows_nothing()) else {
            panic!("nothing is known about this machine; nothing may be installed");
        };
        assert!(!blockers.is_empty());
    }

    #[test]
    fn every_catalogued_artefact_is_reachable_by_some_machine() {
        // An artefact no machine can be offered is dead weight, and usually
        // means its requirements are wrong.
        for artifact in catalog() {
            assert!(
                machines::golden_set().iter().any(|(_, machine)| {
                    matches!(select(machine), Selection::Artifact(chosen) if chosen.id == artifact.id)
                }),
                "{} is offered to no machine in the golden set",
                artifact.id
            );
        }
    }

    #[test]
    fn a_removable_volume_cannot_satisfy_the_disk_requirement() {
        let mut machine = machines::windows_full_disk();
        machine.volumes.push(project_host_platform::VolumeInfo {
            mount_point: "E:\\".to_string(),
            total_bytes: 500 * GB,
            free_bytes: 400 * GB,
            removable: true,
            kind: project_host_platform::StorageKind::Unknown,
        });
        assert!(
            matches!(select(&machine), Selection::Blocked(_)),
            "a USB stick must not satisfy the install requirement"
        );
        let _ = Architecture::X86_64;
    }
}
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cargo test -p project-host-compatibility select`
Expected: FAIL — `Requirements`, `select`, `catalog` and `Selection` do not exist.

- [ ] **Step 6: Implement the catalogue**

Create `crates/compatibility/src/catalog.rs`:

```rust
//! What can be installed, and what each thing requires.
//!
//! Requirements are declared beside the artefact rather than embedded in the
//! selection logic, so that adding an artefact is a data change and the
//! property test in `select` covers it automatically.
//!
//! Minimum builds are taken from the vendor's published requirements. Where a
//! value is a floor for a *backend* rather than for the product, the comment
//! says which.

use project_host_platform::{Architecture, PackageManager};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerProduct {
    DockerDesktop,
    DockerEngine,
}

impl DockerProduct {
    pub fn display_name(self) -> &'static str {
        match self {
            DockerProduct::DockerDesktop => "Docker Desktop",
            DockerProduct::DockerEngine => "Docker Engine",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    Windows,
    Linux,
}

/// What a machine must be for an artefact to be offered to it.
#[derive(Debug, Clone)]
pub struct Requirements {
    pub arch: &'static [Architecture],
    /// Windows build floor. `None` on Linux, where the kernel floor is old
    /// enough that every distribution this can detect clears it.
    pub min_os_build: Option<u32>,
    /// Substrings, any of which the reported edition may contain.
    pub required_editions: Option<&'static [&'static str]>,
    pub requires_virtualization: bool,
    pub min_free_bytes: u64,
    pub package_managers: Option<&'static [PackageManager]>,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub id: &'static str,
    pub product: DockerProduct,
    pub host: HostKind,
    pub requirements: Requirements,
}

const GB: u64 = 1024 * 1024 * 1024;

/// x86_64 only: Docker publishes no Docker Desktop build for Windows on ARM,
/// which is why that machine is blocked rather than given the x64 installer.
static WINDOWS_ARCHES: &[Architecture] = &[Architecture::X86_64];
static LINUX_ARCHES: &[Architecture] = &[Architecture::X86_64, Architecture::Aarch64];

static LINUX_MANAGERS: &[PackageManager] = &[
    PackageManager::Apt,
    PackageManager::Dnf,
    PackageManager::Pacman,
    PackageManager::Zypper,
];

static CATALOG: &[Artifact] = &[
    Artifact {
        id: "docker-desktop-windows-x86_64",
        product: DockerProduct::DockerDesktop,
        host: HostKind::Windows,
        requirements: Requirements {
            arch: WINDOWS_ARCHES,
            // The WSL2 backend's floor. Below this, Docker Desktop's supported
            // configurations do not include this machine.
            min_os_build: Some(19045),
            required_editions: None,
            requires_virtualization: true,
            min_free_bytes: 20 * GB,
            package_managers: None,
        },
    },
    Artifact {
        id: "docker-engine-linux",
        product: DockerProduct::DockerEngine,
        host: HostKind::Linux,
        requirements: Requirements {
            arch: LINUX_ARCHES,
            min_os_build: None,
            required_editions: None,
            // Docker Engine runs natively on the host kernel; there is no VM
            // and therefore no virtualization requirement.
            requires_virtualization: false,
            min_free_bytes: 10 * GB,
            package_managers: Some(LINUX_MANAGERS),
        },
    },
];

pub fn catalog() -> &'static [Artifact] {
    CATALOG
}
```

- [ ] **Step 7: Implement selection**

Prepend to `crates/compatibility/src/select.rs`, above the test module:

```rust
//! Which artefact this machine gets, or why it gets none.
//!
//! Selection is total: it returns **exactly one artefact, or a non-empty list
//! of blockers**. There is no fallback, no closest match and no default
//! artefact. That is the whole of "never install an incompatible version", and
//! the property test below is what holds it true as the catalogue grows.

use project_host_platform::SystemSnapshot;

use crate::blocker::Blocker;
pub use crate::catalog::{catalog, Artifact, DockerProduct, HostKind, Requirements};

#[derive(Debug)]
pub enum Selection {
    Artifact(&'static Artifact),
    Blocked(Vec<Blocker>),
}

const GB: u64 = 1024 * 1024 * 1024;

/// Which firmware setting to name, from the CPU vendor. Naming the wrong one
/// sends the user hunting for a setting their firmware does not have.
fn virtualization_setting(snapshot: &SystemSnapshot) -> &'static str {
    match snapshot.cpu.vendor.as_deref() {
        Some(vendor) if vendor.contains("AMD") => "AMD SVM (often listed as SVM Mode)",
        Some(vendor) if vendor.contains("Intel") => "Intel VT-x (often listed as Intel Virtualization Technology)",
        // Both, when the vendor is unknown: one of the two names will match
        // what is on screen.
        _ => "Intel VT-x or AMD SVM",
    }
}

/// Every requirement this machine fails. Empty means it satisfies them all.
pub fn unmet(requirements: &Requirements, snapshot: &SystemSnapshot) -> Vec<Blocker> {
    let mut blockers = Vec::new();

    if !requirements.arch.contains(&snapshot.arch) {
        blockers.push(Blocker::ArchitectureUnsupported {
            found: format!("{:?}", snapshot.arch),
        });
    }

    if requirements.requires_virtualization {
        match (snapshot.virtualization.enabled, snapshot.virtualization.supported) {
            (Some(true), _) => {}
            // Supported but off is fixable; unsupported is not. The two get
            // different advice and must not be collapsed.
            (_, Some(false)) => blockers.push(Blocker::VirtualizationUnsupported),
            (Some(false), _) => blockers.push(Blocker::VirtualizationDisabled {
                setting: virtualization_setting(snapshot),
            }),
            (None, _) => blockers.push(Blocker::VirtualizationDisabled {
                setting: virtualization_setting(snapshot),
            }),
        }
    }

    if let Some(required) = requirements.min_os_build {
        match snapshot.os.build {
            Some(found) if found >= required => {}
            Some(found) => blockers.push(Blocker::OsTooOld {
                product: "Docker Desktop",
                name: "Windows",
                required,
                found,
            }),
            None => blockers.push(Blocker::OsBuildUnknown),
        }
    }

    if let Some(editions) = requirements.required_editions {
        match snapshot.os.edition.as_deref() {
            Some(found) if editions.iter().any(|wanted| found.contains(wanted)) => {}
            Some(found) => blockers.push(Blocker::EditionUnsupported {
                found: found.to_string(),
                needed: editions.join(" or "),
            }),
            None => blockers.push(Blocker::EditionUnsupported {
                found: "an unrecognised edition".to_string(),
                needed: editions.join(" or "),
            }),
        }
    }

    if let Some(managers) = requirements.package_managers {
        let found = snapshot
            .linux
            .as_ref()
            .and_then(|linux| linux.package_manager);
        match found {
            Some(manager) if managers.contains(&manager) => {}
            _ => blockers.push(Blocker::NoPackageManager),
        }
    }

    // Unknown free space counts as zero. Absence of evidence must never become
    // an install.
    let free = snapshot.largest_fixed_free_bytes().unwrap_or(0);
    if free < requirements.min_free_bytes {
        blockers.push(Blocker::InsufficientDisk {
            needed_gb: requirements.min_free_bytes / GB,
            found_gb: free / GB,
        });
    }

    blockers
}

/// The artefact for this machine, or every reason it has none.
///
/// When several artefacts fail, the reported blockers are those of the artefact
/// that failed on the fewest counts — the one the machine is closest to being
/// able to run, and therefore the one whose advice is most likely to be
/// actionable.
pub fn select(snapshot: &SystemSnapshot) -> Selection {
    let mut closest: Option<Vec<Blocker>> = None;

    for artifact in catalog() {
        let blockers = unmet(&artifact.requirements, snapshot);
        if blockers.is_empty() {
            return Selection::Artifact(artifact);
        }
        if closest
            .as_ref()
            .is_none_or(|best| blockers.len() < best.len())
        {
            closest = Some(blockers);
        }
    }

    Selection::Blocked(closest.unwrap_or_else(|| vec![Blocker::VirtualizationUnsupported]))
}
```

Note: `Architecture` must derive `PartialEq` (it does, from Task 1) for `contains` to work, and `unmet` is `pub` so the wizard plan can reuse it.

Add to `crates/compatibility/src/lib.rs`:

```rust
pub mod catalog;
pub mod select;

pub use catalog::{Artifact, DockerProduct, HostKind, Requirements};
pub use select::{select, unmet, Selection};
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p project-host-compatibility`
Expected: PASS. If `every_catalogued_artefact_is_reachable_by_some_machine` fails, the requirements are wrong — fix the requirement, not the test.

- [ ] **Step 9: Verify and commit**

```bash
pnpm verify
git add crates/compatibility
git commit -m "Select a Docker artefact, or say why none applies"
```

---

### Task 10: Apply the defaults to new projects

The payoff. After this task every user gets defaults matched to their machine, with no wizard involved.

**Files:**

- Modify: `crates/app-core/Cargo.toml`
- Modify: `crates/app-core/src/state.rs:17-30` (`Inner`), `:46-65` (`AppState::new`), and add an accessor
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs:543-546`
- Test: `crates/app-core/tests/resource_defaults.rs`

**Interfaces:**

- Consumes: `assess`, `Assessment`, `ResourceDefaults` (Task 8); `SystemProbe`, `SystemScanner`, `FixedProbe` (Task 2).
- Produces: `AppState::resource_defaults(&self) -> ResourceDefaults`, and a new `AppState::new` parameter `assessment: Assessment` placed immediately after `docker_status`.

- [ ] **Step 1: Add the dependencies**

In `crates/app-core/Cargo.toml`, under `[dependencies]`:

```toml
project-host-compatibility = { path = "../compatibility" }
```

In `apps/desktop/src-tauri/Cargo.toml`, under `[dependencies]`:

```toml
project-host-compatibility = { path = "../../../crates/compatibility" }
```

- [ ] **Step 2: Write the failing test**

Create `crates/app-core/tests/resource_defaults.rs`:

```rust
//! The defaults a new project starts with come from the machine, not a
//! constant.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use project_host_compatibility::machines::{windows_11_low_end, windows_11_workstation};
use project_host_compatibility::{assess, PerformanceTier};
use project_host_platform::SystemSnapshot;

#[test]
fn a_low_end_machine_gets_smaller_defaults_than_a_workstation() {
    let low = assess(&windows_11_low_end());
    let high = assess(&windows_11_workstation());

    assert!(low.defaults.memory_limit_mb < high.defaults.memory_limit_mb);
    assert!(low.defaults.cpu_limit_cores < high.defaults.cpu_limit_cores);
    assert!(low.defaults.process_limit < high.defaults.process_limit);
    assert_eq!(low.tier, PerformanceTier::Minimal);
}

#[test]
fn an_unknown_machine_gets_the_defaults_that_were_hardcoded_before() {
    // The regression guard on this whole change: a machine we cannot measure
    // must behave exactly as the application did when 512/1.0/128 were literals
    // at the creation call site.
    let assessment = assess(&SystemSnapshot::unknown());
    assert_eq!(assessment.defaults.memory_limit_mb, 512);
    assert_eq!(assessment.defaults.process_limit, 128);
}
```

Add `project-host-compatibility` and `project-host-platform` to `crates/app-core`'s `[dev-dependencies]` if they are not already reachable.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p project-host-app-core --test resource_defaults`
Expected: FAIL — `project_host_compatibility` is not a dependency.

- [ ] **Step 4: Implement**

In `crates/app-core/src/state.rs`, add to `Inner` after `docker_status`:

```rust
    /// Resource defaults for newly created projects, decided once at startup
    /// from what this machine is. Existing projects are never touched: a user
    /// who set a limit deliberately does not have it overwritten.
    pub assessment: Assessment,
```

Add `use project_host_compatibility::{Assessment, ResourceDefaults};` to the imports, add an `assessment: Assessment` parameter to `AppState::new` immediately after `docker_status`, set `assessment,` in the `Inner` construction, and add the accessor beside `config()`:

```rust
    pub fn resource_defaults(&self) -> ResourceDefaults {
        self.0.assessment.defaults
    }
```

In `apps/desktop/src-tauri/src/lib.rs`, at the point where `AppState::new` is called, compute the assessment first:

```rust
    let assessment = project_host_compatibility::assess(
        &project_host_platform::SystemScanner.snapshot(),
    );
    tracing::info!(
        tier = assessment.tier.as_str(),
        memory_limit_mb = assessment.defaults.memory_limit_mb,
        cpu_limit_cores = assessment.defaults.cpu_limit_cores,
        "assessed this machine"
    );
```

`SystemScanner.snapshot()` needs `use project_host_platform::SystemProbe;` in scope.

Then replace lines 543-546:

```rust
            memory_limit_mb: defaults.memory_limit_mb,
            cpu_limit_cores: defaults.cpu_limit_cores,
            storage_limit_mb: 2048,
            process_limit: defaults.process_limit,
```

with `let defaults = app.resource_defaults();` bound above the `NewProject` literal. `storage_limit_mb` is unchanged: it is not part of `ResourceDefaults`, because no probe here measures what a project will store.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p project-host-app-core --test resource_defaults`
Expected: PASS, 2 tests.

Run: `cargo test --workspace`
Expected: PASS. Existing `AppState::new` call sites in tests need the new argument; pass `project_host_compatibility::assess(&SystemSnapshot::unknown())`, which reproduces the previous hardcoded values exactly.

- [ ] **Step 6: Verify and commit**

```bash
pnpm verify
git add crates/app-core apps/desktop/src-tauri Cargo.lock
git commit -m "Give new projects defaults matched to the machine"
```

---

### Task 11: Document the scan

**Files:**

- Create: `docs/compatibility.md`
- Modify: `docs/platform-support.md` (link the new document)

- [ ] **Step 1: Write the document**

`docs/compatibility.md` covers, in prose matching the house style of `docs/installers.md`:

- what the scan reads, and that it cannot fail
- the tier table from spec §4, and why CPU age is not an input
- the 12.5% invariant, and that it outranks the table
- that defaults apply to new projects only
- the catalogue's requirements per artefact, and the selection guarantee
- the blocker list, and that firmware cannot be changed by any application
- **what is unverified**: every Linux path, every subprocess invocation in
  `probe::platform_specific`, and that no real Docker install has been performed
  by any of this code

- [ ] **Step 2: Verify and commit**

```bash
pnpm verify
git add docs
git commit -m "Document the compatibility scan"
```

---

## Self-Review

**Spec coverage.** §2 shape → Tasks 1, 2, 7. §3 scan → Tasks 1, 3, 4, 5, 6. §4 tier → Task 8. §5 selection → Task 9; §5.1 authenticity is stage 6, out of this plan's scope by design. §6 step machine → out of scope (stages 5–7). §7 parity → Task 9 catalogue covers both hosts; the acquisition half is out of scope. §8 errors → Task 9 `Blocker`. §9 testing → tests throughout, unverifiable paths marked in Tasks 6 and 11. §10 staging 1–4 → this plan; 5–8 → the two follow-on plans.

**Placeholders.** None. Every code step carries real code; Task 11 is a documentation task whose contents are enumerated rather than deferred.

**Type consistency.** `SystemSnapshot` field names are fixed in Task 1 and used unchanged in Tasks 3–9. `ResourceDefaults`'s three fields match the `projects` column names exactly, and are consumed under those names in Task 10. `unmet` is the single requirement-checking function, deliberately shadowed in the Task 9 test by an independently written `independently_satisfied` so the property test is not a tautology. `Architecture` derives `PartialEq` in Task 1, which `contains` in Task 9 requires.

**Scope of what ships.** Task 10 is the point at which this plan produces user-visible value. Tasks 1–9 are all testable but internal; a reviewer stopping after Task 9 would have a well-tested library that nothing calls — the state `crates/host-runner` is in today, and the reason Task 10 is in this plan rather than the next one.
