//! What can be installed for a runtime, and what it needs first.
//!
//! Package identifiers live beside the runtime rather than inside the planner,
//! so adding a language is a data change and the property tests here cover it
//! automatically. This is the arrangement `compatibility::catalog` already uses,
//! for the same reason.
//!
//! Identifiers are the vendors' published ones. Where a Linux distribution does
//! not package a runtime at all — Bun and Deno, which ship their own installers
//! and appear in no default repository — the list is empty rather than filled
//! with a guess, and the planner turns that into a blocker naming the platform.

use project_host_platform::PackageManager;

/// A package that a toolchain needs before it is usable, rather than merely
/// present.
///
/// Not a runtime: nothing resolves "MSVC Build Tools" to an executable and
/// nothing starts a project with it, so it carries package identifiers and no
/// probe candidates.
#[derive(Debug, Clone)]
pub struct Prerequisite {
    pub id: &'static str,
    pub display_name: &'static str,
    pub winget_id: Option<&'static str>,
    pub linux_packages: &'static [(PackageManager, &'static str)],
}

#[derive(Debug, Clone)]
pub struct ToolchainSpec {
    pub id: &'static str,
    /// Matches `api_types::Runtime`'s wire value, which is how a project says
    /// what it needs.
    pub runtime: &'static str,
    pub display_name: &'static str,
    pub winget_id: Option<&'static str>,
    pub linux_packages: &'static [(PackageManager, &'static str)],
    /// Where to get it by hand. Named in the blocker raised on a platform that
    /// does not package it, so the refusal ends somewhere rather than nowhere.
    pub vendor: &'static str,
    /// Ids into [`prerequisites`].
    pub prerequisites: &'static [&'static str],
}

use PackageManager::{Apt, Dnf, Pacman, Zypper};

static GIT: &[(PackageManager, &str)] =
    &[(Apt, "git"), (Dnf, "git"), (Pacman, "git"), (Zypper, "git")];

/// Compiling a native addon needs a C toolchain. Without it `npm install` fails
/// on the first dependency with a binding, after a toolchain install that
/// reported success.
static BUILD_TOOLS: &[(PackageManager, &str)] = &[
    (Apt, "build-essential"),
    (Dnf, "gcc-c++"),
    (Pacman, "base-devel"),
    (Zypper, "gcc-c++"),
];

static PREREQUISITES: &[Prerequisite] = &[
    Prerequisite {
        id: "git",
        display_name: "Git",
        winget_id: Some("Git.Git"),
        linux_packages: GIT,
    },
    Prerequisite {
        id: "build-tools",
        display_name: "C/C++ build tools",
        winget_id: Some("Microsoft.VisualStudio.2022.BuildTools"),
        linux_packages: BUILD_TOOLS,
    },
];

static NEEDS_BUILD: &[&str] = &["git", "build-tools"];
static NEEDS_GIT: &[&str] = &["git"];

static NODE_PACKAGES: &[(PackageManager, &str)] = &[
    (Apt, "nodejs"),
    (Dnf, "nodejs"),
    (Pacman, "nodejs"),
    (Zypper, "nodejs"),
];

/// Empty on purpose: Bun and Deno are in no distribution's default repository.
/// An invented package name would fail at the moment a user pressed Start.
static NONE: &[(PackageManager, &str)] = &[];

static CATALOG: &[ToolchainSpec] = &[
    ToolchainSpec {
        id: "nodejs",
        runtime: "NODEJS",
        display_name: "Node.js",
        winget_id: Some("OpenJS.NodeJS.LTS"),
        linux_packages: NODE_PACKAGES,
        vendor: "https://nodejs.org",
        prerequisites: NEEDS_BUILD,
    },
    ToolchainSpec {
        // TypeScript is compiled by a package the project installs; what the
        // machine needs is the interpreter underneath it, which is Node.
        id: "typescript",
        runtime: "TYPESCRIPT",
        display_name: "Node.js (for TypeScript)",
        winget_id: Some("OpenJS.NodeJS.LTS"),
        linux_packages: NODE_PACKAGES,
        vendor: "https://nodejs.org",
        prerequisites: NEEDS_BUILD,
    },
    ToolchainSpec {
        id: "bun",
        runtime: "BUN",
        display_name: "Bun",
        winget_id: Some("Oven-sh.Bun"),
        linux_packages: NONE,
        vendor: "https://bun.sh",
        prerequisites: NEEDS_GIT,
    },
    ToolchainSpec {
        id: "deno",
        runtime: "DENO",
        display_name: "Deno",
        winget_id: Some("DenoLand.Deno"),
        linux_packages: NONE,
        vendor: "https://deno.com",
        prerequisites: NEEDS_GIT,
    },
    ToolchainSpec {
        id: "python",
        runtime: "PYTHON",
        display_name: "Python 3",
        winget_id: Some("Python.Python.3.12"),
        linux_packages: &[
            (Apt, "python3"),
            (Dnf, "python3"),
            // Arch's `python` is 3.x; its `python2` is the one that is versioned.
            (Pacman, "python"),
            (Zypper, "python3"),
        ],
        vendor: "https://www.python.org",
        prerequisites: NEEDS_BUILD,
    },
    ToolchainSpec {
        id: "go",
        runtime: "GO",
        display_name: "Go",
        winget_id: Some("GoLang.Go"),
        linux_packages: &[
            (Apt, "golang-go"),
            (Dnf, "golang"),
            (Pacman, "go"),
            (Zypper, "go"),
        ],
        vendor: "https://go.dev",
        prerequisites: NEEDS_GIT,
    },
    ToolchainSpec {
        id: "rust",
        runtime: "RUST",
        display_name: "Rust",
        winget_id: Some("Rustlang.Rustup"),
        linux_packages: &[
            (Apt, "cargo"),
            (Dnf, "cargo"),
            (Pacman, "rust"),
            (Zypper, "cargo"),
        ],
        vendor: "https://rustup.rs",
        prerequisites: NEEDS_BUILD,
    },
    ToolchainSpec {
        id: "java",
        runtime: "JAVA",
        display_name: "Java (Temurin JDK)",
        winget_id: Some("EclipseAdoptium.Temurin.21.JDK"),
        linux_packages: &[
            (Apt, "default-jdk"),
            (Dnf, "java-21-openjdk-devel"),
            (Pacman, "jdk-openjdk"),
            (Zypper, "java-21-openjdk-devel"),
        ],
        vendor: "https://adoptium.net",
        prerequisites: NEEDS_GIT,
    },
    ToolchainSpec {
        id: "php",
        runtime: "PHP",
        display_name: "PHP",
        winget_id: Some("PHP.PHP.8.3"),
        linux_packages: &[(Apt, "php"), (Dnf, "php"), (Pacman, "php"), (Zypper, "php")],
        vendor: "https://www.php.net",
        prerequisites: NEEDS_GIT,
    },
    ToolchainSpec {
        id: "ruby",
        runtime: "RUBY",
        display_name: "Ruby",
        winget_id: Some("RubyInstallerTeam.Ruby.3.3"),
        linux_packages: &[
            (Apt, "ruby-full"),
            (Dnf, "ruby"),
            (Pacman, "ruby"),
            (Zypper, "ruby"),
        ],
        vendor: "https://www.ruby-lang.org",
        prerequisites: NEEDS_BUILD,
    },
    ToolchainSpec {
        id: "dotnet",
        runtime: "DOTNET",
        display_name: ".NET SDK",
        winget_id: Some("Microsoft.DotNet.SDK.8"),
        linux_packages: &[
            (Apt, "dotnet-sdk-8.0"),
            (Dnf, "dotnet-sdk-8.0"),
            (Pacman, "dotnet-sdk"),
            (Zypper, "dotnet-sdk-8.0"),
        ],
        vendor: "https://dotnet.microsoft.com",
        prerequisites: NEEDS_GIT,
    },
];

pub fn catalog() -> &'static [ToolchainSpec] {
    CATALOG
}

pub fn prerequisites() -> &'static [Prerequisite] {
    PREREQUISITES
}

/// The spec for a runtime, or `None` where one executable cannot satisfy it.
///
/// `STATIC` needs no toolchain and `POLYGLOT` needs several; both are absent
/// from the catalogue rather than given an entry that would be wrong.
pub fn spec_for(runtime: &str) -> Option<&'static ToolchainSpec> {
    CATALOG.iter().find(|spec| spec.runtime == runtime)
}

pub fn prerequisite(id: &str) -> Option<&'static Prerequisite> {
    PREREQUISITES.iter().find(|entry| entry.id == id)
}

impl ToolchainSpec {
    /// The package to install under `manager`, if this distribution has one.
    pub fn linux_package(&self, manager: PackageManager) -> Option<&'static str> {
        self.linux_packages
            .iter()
            .find(|(candidate, _)| *candidate == manager)
            .map(|(_, package)| *package)
    }
}

impl Prerequisite {
    pub fn linux_package(&self, manager: PackageManager) -> Option<&'static str> {
        self.linux_packages
            .iter()
            .find(|(candidate, _)| *candidate == manager)
            .map(|(_, package)| *package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANAGERS: [PackageManager; 4] = [Apt, Dnf, Pacman, Zypper];

    /// Every runtime the product plans for, minus the two that cannot be
    /// satisfied by installing one thing. A runtime added to `api-types`
    /// without an entry here fails this test rather than failing at the moment
    /// a user presses Start.
    const INSTALLABLE: [&str; 11] = [
        "NODEJS",
        "TYPESCRIPT",
        "BUN",
        "DENO",
        "PYTHON",
        "GO",
        "RUST",
        "JAVA",
        "PHP",
        "RUBY",
        "DOTNET",
    ];

    #[test]
    fn every_installable_runtime_has_a_spec() {
        for runtime in INSTALLABLE {
            assert!(spec_for(runtime).is_some(), "{runtime} has no spec");
        }
    }

    /// Installing something for a static site, or picking one toolchain for a
    /// project that declared several, would both be wrong.
    #[test]
    fn static_and_polyglot_have_no_spec() {
        assert!(spec_for("STATIC").is_none());
        assert!(spec_for("POLYGLOT").is_none());
    }

    #[test]
    fn every_spec_has_a_distinct_id() {
        let mut ids: Vec<&str> = catalog().iter().map(|spec| spec.id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate toolchain id");
    }

    /// Windows is the platform every runtime here is published for, so a
    /// missing winget id is an omission rather than a fact about the world.
    #[test]
    fn every_spec_can_be_installed_on_windows() {
        for spec in catalog() {
            assert!(spec.winget_id.is_some(), "{} has no winget id", spec.id);
        }
    }

    /// Partial coverage is the dangerous state: a runtime installable on
    /// Debian and silently unavailable on Fedora would be found by a user, not
    /// by us. Either every manager packages it or none does.
    #[test]
    fn linux_coverage_is_all_managers_or_none() {
        for spec in catalog() {
            let covered = MANAGERS
                .iter()
                .filter(|manager| spec.linux_package(**manager).is_some())
                .count();

            assert!(
                covered == 0 || covered == MANAGERS.len(),
                "{} is packaged for {covered} of {} managers",
                spec.id,
                MANAGERS.len()
            );
        }
    }

    /// The two runtimes that ship their own installers and appear in no
    /// default repository. Named explicitly so that adding a package for one
    /// later is a deliberate edit to this test.
    #[test]
    fn bun_and_deno_are_the_only_runtimes_absent_from_linux() {
        let absent: Vec<&str> = catalog()
            .iter()
            .filter(|spec| spec.linux_package(Apt).is_none())
            .map(|spec| spec.id)
            .collect();

        assert_eq!(absent, vec!["bun", "deno"]);
    }

    #[test]
    fn every_prerequisite_named_by_a_spec_exists() {
        for spec in catalog() {
            for id in spec.prerequisites {
                assert!(
                    prerequisite(id).is_some(),
                    "{} names unknown prerequisite {id}",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn every_prerequisite_is_installable_on_every_platform() {
        for entry in prerequisites() {
            assert!(entry.winget_id.is_some(), "{} has no winget id", entry.id);
            for manager in MANAGERS {
                assert!(
                    entry.linux_package(manager).is_some(),
                    "{} has no package for {}",
                    entry.id,
                    manager.as_str()
                );
            }
        }
    }
}
