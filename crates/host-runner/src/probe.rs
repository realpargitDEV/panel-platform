//! Finding a runtime's toolchain on this machine.
//!
//! A container brings its own Node or Python. The host may have neither, and
//! the product never installs one behind the user's back — so before host mode
//! is offered for a project, the toolchain it needs has to be found and named.
//!
//! Discovery is split from execution on purpose. [`probe`] takes an
//! [`ExecutableResolver`], so the tests here decide what the machine appears to
//! have rather than depending on what it really has. A test suite whose result
//! changes when someone installs Deno is not a test suite.

use std::path::PathBuf;

/// Where an executable lives on this machine, if it lives here at all.
///
/// `Debug` is a supertrait so that anything holding one — [`CommandInputs`],
/// in particular — can derive its own, which the workspace lints require.
///
/// [`CommandInputs`]: crate::CommandInputs
pub trait ExecutableResolver: Send + Sync + std::fmt::Debug {
    /// The absolute path `name` resolves to, or `None` if it is not on `PATH`.
    fn resolve(&self, name: &str) -> Option<PathBuf>;

    /// The version string `executable` reports, or `None` if asking failed.
    ///
    /// Separate from [`Self::resolve`] because it costs a process launch, and
    /// because it is the half the tests replace.
    fn version(&self, executable: &std::path::Path) -> Option<String>;
}

/// What probing found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toolchain {
    Found {
        executable: PathBuf,
        /// What the executable said when asked. Reported to the user as-is
        /// rather than parsed into a number: "v22.3.0", "Python 3.12.4" and
        /// "go version go1.22.1 windows/amd64" have nothing in common, and a
        /// parser that mangles one of them is worse than showing the line.
        version: Option<String>,
    },
    /// Nothing was found, and this is everything that was tried — the message
    /// has to name what to install, not just say no.
    Missing { looked_for: Vec<String> },
    /// This runtime has no single toolchain to look for, and that is not a
    /// problem to report.
    ///
    /// `POLYGLOT` is the case: a project that genuinely needs several
    /// toolchains cannot be answered by finding one executable, and probing for
    /// one and failing would refuse a project that runs perfectly well. What
    /// checks such a project is the start command's own program — if it says
    /// `make build`, then `make` is what has to exist, and
    /// [`crate::start_command`] says so by name when it does not.
    NotRequired,
}

impl Toolchain {
    pub fn is_found(&self) -> bool {
        matches!(self, Toolchain::Found { .. })
    }

    /// Whether a command may be built against this. False only for `Missing`.
    pub fn is_usable(&self) -> bool {
        !matches!(self, Toolchain::Missing { .. })
    }
}

/// The executables that can run a given runtime, best first.
///
/// Order is the preference order. `python3` before `python` because a machine
/// with both has `python` pointing at 2.x often enough to matter, and a project
/// that asked for Python meant the one that is still supported.
pub fn candidates_for(runtime: &str) -> &'static [&'static str] {
    match runtime {
        "NODEJS" | "TYPESCRIPT" => &["node"],
        "BUN" => &["bun"],
        "DENO" => &["deno"],
        "PYTHON" => &["python3", "python"],
        "GO" => &["go"],
        "RUST" => &["cargo"],
        "JAVA" => &["java"],
        "PHP" => &["php"],
        "RUBY" => &["ruby"],
        "DOTNET" => &["dotnet"],
        // A static site needs no language toolchain, and POLYGLOT means
        // several at once — neither can be answered by finding one executable.
        // Both are refused for host mode by the caller rather than pretended
        // about here.
        _ => &[],
    }
}

/// Find the toolchain for `runtime` on this machine.
pub fn probe(runtime: &str, resolver: &dyn ExecutableResolver) -> Toolchain {
    let candidates = candidates_for(runtime);

    for name in candidates {
        if let Some(executable) = resolver.resolve(name) {
            let version = resolver.version(&executable);
            return Toolchain::Found {
                executable,
                version,
            };
        }
    }

    Toolchain::Missing {
        looked_for: candidates.iter().map(|name| (*name).to_string()).collect(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    #[derive(Debug)]
    struct FakeMachine {
        installed: Vec<&'static str>,
    }

    impl ExecutableResolver for FakeMachine {
        fn resolve(&self, name: &str) -> Option<PathBuf> {
            self.installed
                .contains(&name)
                .then(|| PathBuf::from(format!("/usr/bin/{name}")))
        }

        fn version(&self, executable: &std::path::Path) -> Option<String> {
            Some(format!("{} 1.2.3", executable.display()))
        }
    }

    fn machine(installed: &[&'static str]) -> FakeMachine {
        FakeMachine {
            installed: installed.to_vec(),
        }
    }

    #[test]
    fn a_runtime_that_is_installed_is_found_with_its_version() {
        let found = probe("NODEJS", &machine(&["node"]));

        match found {
            Toolchain::Found {
                executable,
                version,
            } => {
                assert!(executable.ends_with("node"));
                assert_eq!(version.as_deref(), Some("/usr/bin/node 1.2.3"));
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    /// A machine with both usually has `python` on 2.x. A project that said
    /// Python did not mean that one.
    #[test]
    fn python3_is_preferred_over_python() {
        let found = probe("PYTHON", &machine(&["python3", "python"]));

        match found {
            Toolchain::Found { executable, .. } => assert!(executable.ends_with("python3")),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn python_alone_is_still_accepted() {
        let found = probe("PYTHON", &machine(&["python"]));

        match found {
            Toolchain::Found { executable, .. } => assert!(executable.ends_with("python")),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    /// The refusal has to say what to install. "Go is not available" leaves the
    /// user nowhere; naming `go` tells them exactly what was looked for.
    #[test]
    fn a_missing_toolchain_reports_everything_it_looked_for() {
        match probe("PYTHON", &machine(&[])) {
            Toolchain::Missing { looked_for } => assert_eq!(looked_for, vec!["python3", "python"]),
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    /// A runtime with no candidates must report `Missing` rather than being
    /// quietly treated as present.
    #[test]
    fn a_runtime_with_no_single_executable_is_not_found() {
        for runtime in ["STATIC", "POLYGLOT"] {
            assert!(
                !probe(runtime, &machine(&["node", "python3"])).is_found(),
                "{runtime} must not claim a toolchain it cannot have"
            );
        }
    }

    /// Every runtime the product plans for either has candidates or is one of
    /// the two that deliberately do not. A new runtime added to the planner
    /// without a line here fails this test rather than failing at the moment a
    /// user presses Start.
    #[test]
    fn every_language_runtime_has_at_least_one_candidate() {
        let languages = [
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

        for runtime in languages {
            assert!(
                !candidates_for(runtime).is_empty(),
                "{runtime} has no candidate executable"
            );
        }

        for runtime in ["STATIC", "POLYGLOT"] {
            assert!(
                candidates_for(runtime).is_empty(),
                "{runtime} cannot be satisfied by one executable"
            );
        }
    }

    #[test]
    fn an_unknown_runtime_is_missing_rather_than_a_panic() {
        assert_eq!(
            probe("PASCAL", &machine(&["node"])),
            Toolchain::Missing { looked_for: vec![] }
        );
    }
}
