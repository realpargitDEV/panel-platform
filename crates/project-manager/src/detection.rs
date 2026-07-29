//! Runtime detection.
//!
//! Inspects an uploaded or selected directory and proposes a runtime, version,
//! package manager and start command. Everything it returns is a *proposal*:
//! the wizard shows it, the user corrects it, and the corrected values are
//! validated against the template manifest before anything is built.
//!
//! Detection never executes anything from the project. It reads files.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Runtime {
    NodeJs,
    Python,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageManager {
    Pnpm,
    Npm,
    Yarn,
    Pip,
    Poetry,
    Uv,
    Pipenv,
    None,
}

/// Something the user should know before deploying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionWarning {
    pub code: String,
    pub message: String,
}

/// A problem that stops a deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detection {
    pub runtime: Runtime,
    pub package_manager: PackageManager,
    pub has_lockfile: bool,
    /// Scripts found in `package.json`.
    pub scripts: BTreeMap<String, String>,
    /// The script or entry file suggested, if one could be determined.
    pub suggested_start: Option<String>,
    pub suggested_entry_file: Option<String>,
    pub suggested_build_command: Option<String>,
    pub suggested_publish_dir: Option<String>,
    pub warnings: Vec<DetectionWarning>,
    /// Non-empty means the project cannot be deployed as-is.
    pub errors: Vec<DetectionError>,
}

impl Detection {
    pub fn is_deployable(&self) -> bool {
        self.errors.is_empty()
    }

    fn warn(&mut self, code: &str, message: &str) {
        self.warnings.push(DetectionWarning {
            code: code.to_string(),
            message: message.to_string(),
        });
    }

    fn fail(&mut self, code: &str, message: &str) {
        self.errors.push(DetectionError {
            code: code.to_string(),
            message: message.to_string(),
        });
    }
}

/// Inspect a directory.
pub fn detect(root: &Path) -> Detection {
    if root.join("package.json").is_file() {
        detect_node(root)
    } else if has_any_python_marker(root) {
        detect_python(root)
    } else if root.join("index.html").is_file() {
        detect_static(root)
    } else {
        let mut detection = empty(Runtime::Static, PackageManager::None);
        detection.fail(
            "NO_RUNTIME_DETECTED",
            "No package.json, Python project file or index.html was found. \
             Choose a runtime manually, or check that the archive contains the \
             project at its top level rather than inside a folder.",
        );
        detection
    }
}

fn empty(runtime: Runtime, package_manager: PackageManager) -> Detection {
    Detection {
        runtime,
        package_manager,
        has_lockfile: false,
        scripts: BTreeMap::new(),
        suggested_start: None,
        suggested_entry_file: None,
        suggested_build_command: None,
        suggested_publish_dir: None,
        warnings: Vec::new(),
        errors: Vec::new(),
    }
}

// ---------------------------------------------------------------- Node.js

/// Scripts whose presence means "development mode". Never chosen by default.
const DEVELOPMENT_SCRIPTS: &[&str] = &["dev", "watch", "start:dev", "develop", "nodemon"];

/// Preference order when no `start` script exists.
const PRODUCTION_SCRIPT_CANDIDATES: &[&str] = &["start", "serve", "start:prod", "production"];

fn detect_node(root: &Path) -> Detection {
    let mut detection = empty(Runtime::NodeJs, PackageManager::Pnpm);

    let raw = match std::fs::read_to_string(root.join("package.json")) {
        Ok(contents) => contents,
        Err(error) => {
            detection.fail(
                "PACKAGE_JSON_UNREADABLE",
                &format!("package.json could not be read: {error}"),
            );
            return detection;
        }
    };

    // An invalid manifest stops the deployment here rather than producing a
    // container that exits immediately with an obscure message.
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            detection.fail(
                "PACKAGE_JSON_INVALID",
                &format!(
                    "package.json is not valid JSON ({}). Fix it before deploying.",
                    error
                ),
            );
            return detection;
        }
    };

    if !parsed.is_object() {
        detection.fail(
            "PACKAGE_JSON_INVALID",
            "package.json must contain a JSON object.",
        );
        return detection;
    }

    // Lockfile evidence decides the package manager; pnpm is the default when
    // there is nothing to go on.
    let (manager, lockfile) = if root.join("pnpm-lock.yaml").is_file() {
        (PackageManager::Pnpm, true)
    } else if root.join("yarn.lock").is_file() {
        (PackageManager::Yarn, true)
    } else if root.join("package-lock.json").is_file() {
        (PackageManager::Npm, true)
    } else {
        (PackageManager::Pnpm, false)
    };
    detection.package_manager = manager;
    detection.has_lockfile = lockfile;

    if !lockfile {
        // Defined fallback rather than improvisation: the build proceeds, but
        // the user is told it is not reproducible and it is recorded.
        detection.warn(
            "NO_LOCKFILE",
            "No lockfile was found. Dependencies will be resolved at build time, \
             so this build is not reproducible and a later rebuild may install \
             different versions. Commit a lockfile to avoid that.",
        );
    }

    if let Some(scripts) = parsed.get("scripts").and_then(|value| value.as_object()) {
        for (name, command) in scripts {
            if let Some(command) = command.as_str() {
                detection.scripts.insert(name.clone(), command.to_string());
            }
        }
    }

    // A production script if there is one; never a development script.
    let chosen = PRODUCTION_SCRIPT_CANDIDATES
        .iter()
        .find(|candidate| detection.scripts.contains_key(**candidate))
        .map(|candidate| (*candidate).to_string());

    match chosen {
        Some(script) => detection.suggested_start = Some(script),
        None => {
            let entry = ["index.js", "main.js", "src/index.js", "app.js", "server.js"]
                .into_iter()
                .find(|candidate| root.join(candidate).is_file());

            match entry {
                Some(file) => {
                    detection.suggested_entry_file = Some(file.to_string());
                    detection.suggested_start = Some(format!("node {file}"));
                }
                None => detection.fail(
                    "NO_START_COMMAND",
                    "No `start` script and no recognisable entry file were found. \
                     Add a `start` script to package.json, or choose an entry file.",
                ),
            }
        }
    }

    if detection
        .scripts
        .keys()
        .any(|name| DEVELOPMENT_SCRIPTS.contains(&name.as_str()))
    {
        detection.warn(
            "DEVELOPMENT_SCRIPTS_PRESENT",
            "This project has development scripts. They are never selected \
             automatically — a file watcher in production restarts the app on \
             every change.",
        );
    }

    // TypeScript needs a build, and the output directory is where the compiled
    // entry point will be.
    let typescript = root.join("tsconfig.json").is_file();
    if typescript && detection.scripts.contains_key("build") {
        detection.suggested_build_command = Some("build".to_string());
        detection.warn(
            "TYPESCRIPT_DETECTED",
            "TypeScript was detected. The build script will run before start, \
             and only the built output ships in the runtime image.",
        );
    } else if typescript {
        detection.warn(
            "TYPESCRIPT_WITHOUT_BUILD",
            "tsconfig.json is present but there is no `build` script. If the \
             project needs compiling, add one.",
        );
    }

    if let Some(engines) = parsed
        .get("engines")
        .and_then(|value| value.get("node"))
        .and_then(|value| value.as_str())
    {
        detection.warn(
            "ENGINE_CONSTRAINT",
            &format!(
                "package.json requires Node {engines}. Choose a matching \
                 supported version; unsupported versions are refused."
            ),
        );
    }

    detection
}

// ---------------------------------------------------------------- Python

fn has_any_python_marker(root: &Path) -> bool {
    [
        "requirements.txt",
        "pyproject.toml",
        "Pipfile",
        "main.py",
        "app.py",
    ]
    .iter()
    .any(|marker| root.join(marker).is_file())
}

fn detect_python(root: &Path) -> Detection {
    let mut detection = empty(Runtime::Python, PackageManager::Pip);

    // Lockfiles first: they are stronger evidence than a manifest.
    let (manager, lockfile) = if root.join("uv.lock").is_file() {
        (PackageManager::Uv, true)
    } else if root.join("poetry.lock").is_file() {
        (PackageManager::Poetry, true)
    } else if root.join("Pipfile.lock").is_file() {
        (PackageManager::Pipenv, true)
    } else if root.join("Pipfile").is_file() {
        (PackageManager::Pipenv, false)
    } else if root.join("pyproject.toml").is_file() {
        (PackageManager::Pip, false)
    } else if root.join("requirements.txt").is_file() {
        // Pinned requirements are effectively a lockfile.
        let pinned = std::fs::read_to_string(root.join("requirements.txt"))
            .map(|contents| contents.contains("=="))
            .unwrap_or(false);
        (PackageManager::Pip, pinned)
    } else {
        (PackageManager::None, false)
    };

    detection.package_manager = manager;
    detection.has_lockfile = lockfile;

    if manager == PackageManager::None {
        detection.warn(
            "NO_DEPENDENCIES",
            "No dependency file was found. The project will run against a bare \
             Python image with no third-party packages installed.",
        );
    } else if !lockfile {
        detection.warn(
            "NO_LOCKFILE",
            "Dependencies are not pinned, so a later rebuild may install \
             different versions. Pin them for reproducible builds.",
        );
    }

    let entry = ["main.py", "app.py", "bot.py", "run.py", "src/main.py"]
        .into_iter()
        .find(|candidate| root.join(candidate).is_file());

    match entry {
        Some(file) => {
            detection.suggested_entry_file = Some(file.to_string());
            detection.suggested_start = Some(format!("python {file}"));
        }
        None => detection.fail(
            "NO_ENTRY_FILE",
            "No entry file was found. Expected one of main.py, app.py, bot.py \
             or run.py, or choose one manually.",
        ),
    }

    detection
}

// ---------------------------------------------------------------- static

fn detect_static(root: &Path) -> Detection {
    let mut detection = empty(Runtime::Static, PackageManager::None);
    detection.suggested_publish_dir = Some(".".to_string());

    // A build tool's output directory is what should be served, not the source.
    for (marker, output) in [
        ("vite.config.ts", "dist"),
        ("vite.config.js", "dist"),
        ("next.config.js", "out"),
        ("svelte.config.js", "build"),
    ] {
        if root.join(marker).is_file() {
            detection.suggested_publish_dir = Some(output.to_string());
            detection.suggested_build_command = Some("build".to_string());
            detection.warn(
                "BUILD_TOOL_DETECTED",
                &format!(
                    "{marker} was found, so the site is built before serving and \
                     `{output}` is published rather than the source."
                ),
            );
            break;
        }
    }

    if !root.join("index.html").is_file() && detection.suggested_build_command.is_none() {
        detection.fail(
            "NO_INDEX_HTML",
            "No index.html was found and no build tool was detected.",
        );
    }

    detection
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temp dir");
        for (path, contents) in files {
            let full = directory.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(full, contents).expect("write");
        }
        directory
    }

    // ---------------------------------------------------------- Node.js

    #[test]
    fn a_node_project_with_a_start_script_is_detected() {
        let dir = project(&[
            (
                "package.json",
                r#"{"name":"x","scripts":{"start":"node index.js"}}"#,
            ),
            ("pnpm-lock.yaml", ""),
        ]);
        let detection = detect(dir.path());
        assert_eq!(detection.runtime, Runtime::NodeJs);
        assert_eq!(detection.package_manager, PackageManager::Pnpm);
        assert!(detection.has_lockfile);
        assert_eq!(detection.suggested_start.as_deref(), Some("start"));
        assert!(detection.is_deployable());
    }

    #[test]
    fn lockfiles_choose_the_package_manager() {
        for (lockfile, expected) in [
            ("pnpm-lock.yaml", PackageManager::Pnpm),
            ("yarn.lock", PackageManager::Yarn),
            ("package-lock.json", PackageManager::Npm),
        ] {
            let dir = project(&[
                ("package.json", r#"{"scripts":{"start":"node ."}}"#),
                (lockfile, ""),
            ]);
            assert_eq!(detect(dir.path()).package_manager, expected, "{lockfile}");
        }
    }

    #[test]
    fn pnpm_is_the_default_without_a_lockfile_and_the_fallback_is_announced() {
        let dir = project(&[("package.json", r#"{"scripts":{"start":"node ."}}"#)]);
        let detection = detect(dir.path());
        assert_eq!(detection.package_manager, PackageManager::Pnpm);
        assert!(!detection.has_lockfile);
        assert!(
            detection.warnings.iter().any(|w| w.code == "NO_LOCKFILE"),
            "the fallback must be stated, not silent"
        );
        assert!(detection.is_deployable());
    }

    #[test]
    fn invalid_package_json_stops_the_deployment() {
        let dir = project(&[("package.json", "{ not json at all")]);
        let detection = detect(dir.path());
        assert!(!detection.is_deployable());
        assert_eq!(detection.errors[0].code, "PACKAGE_JSON_INVALID");
    }

    #[test]
    fn a_package_json_that_is_not_an_object_is_refused() {
        let dir = project(&[("package.json", "[1,2,3]")]);
        assert!(!detect(dir.path()).is_deployable());
    }

    #[test]
    fn a_missing_start_script_falls_back_to_an_entry_file() {
        let dir = project(&[
            ("package.json", r#"{"scripts":{"test":"vitest"}}"#),
            ("index.js", ""),
        ]);
        let detection = detect(dir.path());
        assert_eq!(detection.suggested_start.as_deref(), Some("node index.js"));
        assert!(detection.is_deployable());
    }

    #[test]
    fn no_start_script_and_no_entry_file_is_an_error() {
        let dir = project(&[("package.json", r#"{"scripts":{"test":"vitest"}}"#)]);
        let detection = detect(dir.path());
        assert!(!detection.is_deployable());
        assert_eq!(detection.errors[0].code, "NO_START_COMMAND");
    }

    #[test]
    fn a_development_script_is_never_suggested() {
        let dir = project(&[(
            "package.json",
            r#"{"scripts":{"dev":"nodemon index.js","start":"node index.js"}}"#,
        )]);
        let detection = detect(dir.path());
        assert_eq!(detection.suggested_start.as_deref(), Some("start"));
        assert!(detection
            .warnings
            .iter()
            .any(|w| w.code == "DEVELOPMENT_SCRIPTS_PRESENT"));
    }

    #[test]
    fn a_dev_only_project_does_not_silently_use_dev() {
        // `dev` exists but is not a production script, so the entry-file path
        // is taken instead of quietly running a watcher in production.
        let dir = project(&[
            ("package.json", r#"{"scripts":{"dev":"nodemon ."}}"#),
            ("index.js", ""),
        ]);
        let detection = detect(dir.path());
        assert_eq!(detection.suggested_start.as_deref(), Some("node index.js"));
    }

    #[test]
    fn typescript_projects_get_a_build_step() {
        let dir = project(&[
            (
                "package.json",
                r#"{"scripts":{"start":"node dist/index.js","build":"tsc"}}"#,
            ),
            ("tsconfig.json", "{}"),
            ("pnpm-lock.yaml", ""),
        ]);
        let detection = detect(dir.path());
        assert_eq!(detection.suggested_build_command.as_deref(), Some("build"));
        assert!(detection
            .warnings
            .iter()
            .any(|w| w.code == "TYPESCRIPT_DETECTED"));
    }

    // ---------------------------------------------------------- Python

    #[test]
    fn python_lockfiles_take_precedence_in_order() {
        for (files, expected) in [
            (
                vec!["uv.lock", "poetry.lock", "requirements.txt"],
                PackageManager::Uv,
            ),
            (
                vec!["poetry.lock", "requirements.txt"],
                PackageManager::Poetry,
            ),
            (vec!["Pipfile.lock"], PackageManager::Pipenv),
        ] {
            let mut entries: Vec<(&str, &str)> = files.iter().map(|f| (*f, "")).collect();
            entries.push(("main.py", ""));
            let dir = project(&entries);
            let detection = detect(dir.path());
            assert_eq!(detection.runtime, Runtime::Python);
            assert_eq!(detection.package_manager, expected, "{files:?}");
            assert!(detection.has_lockfile);
        }
    }

    #[test]
    fn pinned_requirements_count_as_a_lockfile() {
        let dir = project(&[
            ("requirements.txt", "discord.py==2.3.2\naiohttp==3.9.1\n"),
            ("main.py", ""),
        ]);
        let detection = detect(dir.path());
        assert!(detection.has_lockfile);
        assert!(!detection.warnings.iter().any(|w| w.code == "NO_LOCKFILE"));
    }

    #[test]
    fn unpinned_requirements_warn() {
        let dir = project(&[("requirements.txt", "discord.py\n"), ("main.py", "")]);
        let detection = detect(dir.path());
        assert!(!detection.has_lockfile);
        assert!(detection.warnings.iter().any(|w| w.code == "NO_LOCKFILE"));
    }

    #[test]
    fn a_python_entry_file_is_suggested() {
        let dir = project(&[("requirements.txt", ""), ("bot.py", "")]);
        let detection = detect(dir.path());
        assert_eq!(detection.suggested_entry_file.as_deref(), Some("bot.py"));
        assert_eq!(detection.suggested_start.as_deref(), Some("python bot.py"));
    }

    #[test]
    fn python_without_an_entry_file_is_an_error() {
        let dir = project(&[("requirements.txt", "")]);
        let detection = detect(dir.path());
        assert!(!detection.is_deployable());
        assert_eq!(detection.errors[0].code, "NO_ENTRY_FILE");
    }

    // ---------------------------------------------------------- static

    #[test]
    fn a_plain_html_site_is_detected() {
        let dir = project(&[("index.html", "<h1>hi</h1>")]);
        let detection = detect(dir.path());
        assert_eq!(detection.runtime, Runtime::Static);
        assert_eq!(detection.suggested_publish_dir.as_deref(), Some("."));
        assert!(detection.is_deployable());
    }

    #[test]
    fn a_vite_site_publishes_its_build_output() {
        let dir = project(&[("index.html", ""), ("vite.config.ts", "")]);
        let detection = detect(dir.path());
        assert_eq!(detection.suggested_publish_dir.as_deref(), Some("dist"));
        assert_eq!(detection.suggested_build_command.as_deref(), Some("build"));
    }

    // ---------------------------------------------------------- nothing

    #[test]
    fn an_unrecognisable_directory_explains_the_likely_cause() {
        let dir = project(&[("README.md", "hello")]);
        let detection = detect(dir.path());
        assert!(!detection.is_deployable());
        assert_eq!(detection.errors[0].code, "NO_RUNTIME_DETECTED");
        // The most common real cause is a ZIP with a wrapping folder.
        assert!(detection.errors[0].message.contains("top level"));
    }

    #[test]
    fn detection_never_executes_anything() {
        // A canary: if detection ever shells out, this file would be run.
        let dir = project(&[
            ("package.json", r#"{"scripts":{"start":"node ."}}"#),
            ("index.js", "require('fs').writeFileSync('EXECUTED','1')"),
        ]);
        let _ = detect(dir.path());
        assert!(!dir.path().join("EXECUTED").exists());
    }
}
