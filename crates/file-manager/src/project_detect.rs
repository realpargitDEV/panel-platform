//! Deciding whether a dropped folder *is* a project.
//!
//! The question matters because the answer changes where the files land.
//! Dropping a folder that is a project should put its contents at the project
//! root — the folder itself is a container the user already opened, and keeping
//! it produces `MyProject/MyProject/package.json`. Dropping a folder of
//! holiday photos should keep the folder, because there the name is the point.
//!
//! No single file settles it. A lone `package.json` is a fragment, not a
//! project; the same file beside a `src/` directory and a lockfile is a
//! project. So this scores the evidence and applies a threshold, and reports
//! the signals it found so the interface can say *why* it decided.
//!
//! Only the top level is read. Walking a whole tree to classify it would cost
//! the same as importing it, and every marker worth finding sits at the root of
//! the thing it marks.

/// A directory whose presence says nothing about whether this is a project.
///
/// They are generated: `node_modules` appears in every Node project and in any
/// folder someone once ran `npm install` in. They are *ignored for scoring*
/// only — an import still copies them, because deleting a user's files to make
/// a heuristic tidier is not a trade this code gets to make.
pub const GENERATED_DIRECTORIES: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "venv",
    ".venv",
    "env",
    "__pycache__",
    "vendor",
    ".gradle",
    "bin",
    "obj",
];

/// How much each marker is worth, and the name shown when it is found.
///
/// Weights rather than a checklist: a manifest is strong evidence, a lockfile
/// beside it is corroboration, and a `.gitignore` on its own is nearly
/// meaningless.
const STRONG: u32 = 3;
const SUPPORTING: u32 = 2;
const WEAK: u32 = 1;

/// The score at which a folder is treated as a project.
///
/// Four is chosen so that a single manifest (3) is not enough on its own but a
/// manifest with anything corroborating it is. That is the rule the interface
/// promises: one configuration file is a file, not a project.
pub const PROJECT_THRESHOLD: u32 = 4;

/// Files whose name alone is a strong signal.
const STRONG_FILES: &[&str] = &[
    "package.json",
    "cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "pipfile",
    "composer.json",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "go.mod",
    "gemfile",
    "pubspec.yaml",
    "mix.exs",
];

/// Lockfiles and container definitions: corroboration, not proof.
const SUPPORTING_FILES: &[&str] = &[
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
    "bun.lockb",
    "cargo.lock",
    "poetry.lock",
    "pipfile.lock",
    "composer.lock",
    "go.sum",
    "gemfile.lock",
    "dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "pnpm-workspace.yaml",
    "lerna.json",
    "turbo.json",
    "nx.json",
];

/// Configuration that often sits beside a project but proves little alone.
const WEAK_FILES: &[&str] = &[
    "tsconfig.json",
    "jsconfig.json",
    "angular.json",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".env",
    ".env.example",
    "makefile",
    "readme.md",
    "cmakelists.txt",
];

/// Config files identified by prefix, because the extension varies:
/// `vite.config.ts`, `vite.config.js`, `vite.config.mjs`.
const WEAK_PREFIXES: &[&str] = &[
    "vite.config",
    "next.config",
    "nuxt.config",
    "svelte.config",
    "webpack.config",
    "rollup.config",
    "astro.config",
    "tailwind.config",
    "eslint.config",
    "babel.config",
    "jest.config",
    "vitest.config",
];

/// Extensions that identify a manifest whose name is the project's, not a
/// fixed word: `Api.csproj`, `Solution.sln`.
const STRONG_EXTENSIONS: &[&str] = &[".csproj", ".sln", ".fsproj", ".vbproj", ".vcxproj"];

/// Directories that suggest source laid out in the usual way.
const SOURCE_DIRECTORIES: &[&str] = &[
    "src",
    "app",
    "lib",
    "public",
    "server",
    "client",
    "api",
    "components",
    "pages",
    "crates",
    "packages",
    "cmd",
    "internal",
    "tests",
    "test",
    "spec",
];

/// One entry at the top level of the folder being judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub is_directory: bool,
}

impl Candidate {
    pub fn file(name: &str) -> Self {
        Self {
            name: name.to_string(),
            is_directory: false,
        }
    }

    pub fn directory(name: &str) -> Self {
        Self {
            name: name.to_string(),
            is_directory: true,
        }
    }
}

/// The ecosystem a project belongs to, named from its markers.
///
/// Reported so the preview can say "Node.js project" rather than "project", and
/// so a monorepo's members can be recognised as the same kind of thing. The
/// order of the checks is the order of specificity: a Tauri project is also a
/// Node project, and the more specific answer is the useful one.
const ECOSYSTEMS: &[(&str, &[&str])] = &[
    ("Tauri", &["tauri.conf.json", "src-tauri"]),
    (
        "Electron",
        &["electron.vite.config.ts", "electron-builder.yml"],
    ),
    (
        "Next.js",
        &["next.config.js", "next.config.mjs", "next.config.ts"],
    ),
    ("Nuxt", &["nuxt.config.ts", "nuxt.config.js"]),
    ("Angular", &["angular.json"]),
    ("Astro", &["astro.config.mjs", "astro.config.ts"]),
    ("SvelteKit", &["svelte.config.js"]),
    (
        "Vite",
        &["vite.config.ts", "vite.config.js", "vite.config.mjs"],
    ),
    ("Rust", &["cargo.toml"]),
    ("Go", &["go.mod"]),
    (
        "Python",
        &["pyproject.toml", "requirements.txt", "pipfile", "setup.py"],
    ),
    ("Ruby", &["gemfile"]),
    ("PHP", &["composer.json"]),
    (".NET", &["global.json"]),
    ("Maven", &["pom.xml"]),
    (
        "Gradle",
        &["build.gradle", "build.gradle.kts", "settings.gradle"],
    ),
    (
        "Docker",
        &["docker-compose.yml", "docker-compose.yaml", "dockerfile"],
    ),
    ("Node.js", &["package.json"]),
];

/// Markers that say a folder is a workspace holding several packages.
const MONOREPO_FILES: &[&str] = &[
    "pnpm-workspace.yaml",
    "lerna.json",
    "turbo.json",
    "nx.json",
    "rush.json",
];

/// Folders a monorepo keeps its members in.
///
/// A `package.json` inside one of these is a workspace member, not a project
/// somebody happened to nest — so it is reported as part of the parent rather
/// than as a rival to it.
pub const WORKSPACE_DIRECTORIES: &[&str] = &[
    "packages", "apps", "crates", "services", "libs", "modules", "examples",
];

/// What was found, and what it adds up to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub score: u32,
    /// The markers that were found, in the order they were scored. Shown to the
    /// user so the decision can be argued with rather than only obeyed.
    pub signals: Vec<String>,
    pub is_project: bool,
    /// `Node.js`, `Rust`, `Tauri`… `None` when nothing identified it.
    pub ecosystem: Option<String>,
    /// True when the folder holds several packages rather than being one.
    pub is_monorepo: bool,
}

/// Name the ecosystem from the entries, most specific first.
fn ecosystem_of(entries: &[Candidate]) -> Option<String> {
    let names: Vec<String> = entries
        .iter()
        .map(|entry| entry.name.to_lowercase())
        .collect();

    for (label, markers) in ECOSYSTEMS {
        if markers
            .iter()
            .any(|marker| names.iter().any(|name| name == marker))
        {
            // `.csproj` and `.sln` carry the project's own name, so they cannot
            // be matched by a fixed string like the rest.
            return Some((*label).to_string());
        }
    }

    if names
        .iter()
        .any(|name| STRONG_EXTENSIONS.iter().any(|ext| name.ends_with(ext)))
    {
        return Some(".NET".to_string());
    }

    None
}

fn is_monorepo(entries: &[Candidate]) -> bool {
    entries.iter().any(|entry| {
        !entry.is_directory && MONOREPO_FILES.contains(&entry.name.to_lowercase().as_str())
    }) || entries.iter().any(|entry| {
        entry.is_directory && WORKSPACE_DIRECTORIES.contains(&entry.name.to_lowercase().as_str())
    })
}

/// Judge a folder from its top-level entries.
///
/// Case-insensitive throughout: `Dockerfile`, `dockerfile` and `DOCKERFILE` are
/// the same file on Windows and the same evidence everywhere.
pub fn detect(entries: &[Candidate]) -> Detection {
    let mut score = 0u32;
    let mut signals = Vec::new();
    let mut source_directories = 0u32;

    for entry in entries {
        let lower = entry.name.to_lowercase();

        if entry.is_directory {
            if lower == ".git" {
                score += SUPPORTING;
                signals.push(".git".to_string());
            } else if GENERATED_DIRECTORIES.contains(&lower.as_str()) {
                // Deliberately worth nothing. `node_modules` is present in
                // every Node project and in any folder anyone ever installed
                // into, so it separates nothing from nothing.
                continue;
            } else if SOURCE_DIRECTORIES.contains(&lower.as_str()) {
                // Capped at two: ten source-shaped folders is not five times
                // the evidence of two, and without a cap a deeply organised
                // folder of documents would out-score a real project.
                if source_directories < 2 {
                    source_directories += 1;
                    score += WEAK;
                    signals.push(format!("{}/", entry.name));
                }
            }
            continue;
        }

        if STRONG_FILES.contains(&lower.as_str())
            || STRONG_EXTENSIONS
                .iter()
                .any(|extension| lower.ends_with(extension))
        {
            score += STRONG;
            signals.push(entry.name.clone());
        } else if SUPPORTING_FILES.contains(&lower.as_str()) {
            score += SUPPORTING;
            signals.push(entry.name.clone());
        } else if WEAK_FILES.contains(&lower.as_str())
            || WEAK_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
        {
            score += WEAK;
            signals.push(entry.name.clone());
        }
    }

    Detection {
        is_project: score >= PROJECT_THRESHOLD,
        score,
        signals,
        ecosystem: ecosystem_of(entries),
        is_monorepo: is_monorepo(entries),
    }
}

/// Read a directory's top level and judge it.
///
/// An unreadable directory is not a project as far as this is concerned: the
/// import will report the real error, and guessing from a partial listing would
/// be worse than declining to guess.
pub fn detect_directory(path: &std::path::Path) -> Detection {
    let mut entries = Vec::new();

    if let Ok(listing) = std::fs::read_dir(path) {
        for item in listing.flatten() {
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let is_directory = item.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            entries.push(Candidate { name, is_directory });
        }
    }

    detect(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_names(files: &[&str], directories: &[&str]) -> Detection {
        let mut entries: Vec<Candidate> = files.iter().map(|name| Candidate::file(name)).collect();
        entries.extend(directories.iter().map(|name| Candidate::directory(name)));
        detect(&entries)
    }

    #[test]
    fn a_lone_manifest_is_not_a_project() {
        // The rule the interface promises: one configuration file is a file.
        let detection = detect_names(&["package.json"], &[]);
        assert_eq!(detection.score, STRONG);
        assert!(!detection.is_project);
    }

    #[test]
    fn a_manifest_with_a_lockfile_is_a_project() {
        assert!(detect_names(&["package.json", "package-lock.json"], &[]).is_project);
    }

    #[test]
    fn a_manifest_beside_source_is_a_project() {
        assert!(detect_names(&["package.json"], &["src"]).is_project);
    }

    #[test]
    fn the_folder_from_the_bug_report_is_a_project() {
        // The folder that produced `ROMIBOT/RomiPlayoff/package.json`.
        let detection = detect_names(
            &[
                ".env",
                ".gitignore",
                "CLAUDE.md",
                "guide.md",
                "package-lock.json",
                "package.json",
                "pnpm-lock.yaml",
                "pnpm-workspace.yaml",
                "README.md",
                "tsconfig.json",
            ],
            &["data", "dist", "docs", "node_modules", "src"],
        );
        assert!(detection.is_project);
        assert!(detection
            .signals
            .iter()
            .any(|signal| signal == "package.json"));
    }

    #[test]
    fn generated_directories_are_worth_nothing() {
        // Otherwise any folder someone once ran `npm install` in scores.
        assert_eq!(
            detect_names(&[], &["node_modules", "dist", "build"]).score,
            0
        );
    }

    #[test]
    fn a_folder_of_documents_is_not_a_project() {
        let detection = detect_names(
            &["notes.txt", "logo.png", "backup.json", "invoice.pdf"],
            &["photos", "receipts"],
        );
        assert_eq!(detection.score, 0);
        assert!(!detection.is_project);
    }

    #[test]
    fn a_repository_with_source_is_a_project_without_any_manifest() {
        // .git (2) + src (1) + public (1)
        assert!(detect_names(&[], &[".git", "src", "public"]).is_project);
    }

    #[test]
    fn source_directories_are_capped_so_an_archive_cannot_out_score_a_project() {
        let detection = detect_names(&[], &["src", "app", "lib", "api", "server", "client"]);
        assert_eq!(detection.score, 2);
        assert!(!detection.is_project);
    }

    #[test]
    fn case_does_not_matter() {
        // The same file on Windows, and the same evidence everywhere.
        assert_eq!(
            detect_names(&["Package.JSON", "DOCKERFILE"], &[]).score,
            STRONG + SUPPORTING
        );
    }

    #[test]
    fn dotnet_projects_are_recognised_by_extension() {
        assert!(detect_names(&["Api.csproj", "Api.sln"], &[]).is_project);
    }

    #[test]
    fn config_files_are_matched_by_prefix_whatever_the_extension() {
        let detection = detect_names(&["vite.config.mts", "next.config.mjs"], &[]);
        assert_eq!(detection.score, WEAK * 2);
    }

    #[test]
    fn signals_report_what_was_actually_found() {
        let detection = detect_names(&["Cargo.toml", "Cargo.lock"], &["src"]);
        assert_eq!(detection.signals, vec!["Cargo.toml", "Cargo.lock", "src/"]);
    }

    #[test]
    fn an_empty_folder_is_not_a_project() {
        assert!(!detect(&[]).is_project);
    }

    #[test]
    fn the_ecosystem_is_named_from_its_markers() {
        assert_eq!(
            detect_names(&["package.json", "package-lock.json"], &[])
                .ecosystem
                .as_deref(),
            Some("Node.js")
        );
        assert_eq!(
            detect_names(&["Cargo.toml", "Cargo.lock"], &[])
                .ecosystem
                .as_deref(),
            Some("Rust")
        );
        assert_eq!(
            detect_names(&["go.mod", "go.sum"], &[])
                .ecosystem
                .as_deref(),
            Some("Go")
        );
        assert_eq!(
            detect_names(&["pyproject.toml"], &["src"])
                .ecosystem
                .as_deref(),
            Some("Python")
        );
        assert_eq!(
            detect_names(&["composer.json"], &["src"])
                .ecosystem
                .as_deref(),
            Some("PHP")
        );
        assert_eq!(
            detect_names(&["Gemfile"], &["app"]).ecosystem.as_deref(),
            Some("Ruby")
        );
        assert_eq!(
            detect_names(&["pom.xml"], &["src"]).ecosystem.as_deref(),
            Some("Maven")
        );
        assert_eq!(
            detect_names(&["build.gradle"], &["src"])
                .ecosystem
                .as_deref(),
            Some("Gradle")
        );
        assert_eq!(
            detect_names(&["Api.csproj"], &["src"]).ecosystem.as_deref(),
            Some(".NET")
        );
    }

    #[test]
    fn the_more_specific_framework_wins_over_the_language_beneath_it() {
        // A Tauri project is also a Node project; naming it Node.js is true and
        // useless.
        let tauri = detect_names(&["package.json", "tauri.conf.json"], &["src-tauri"]);
        assert_eq!(tauri.ecosystem.as_deref(), Some("Tauri"));

        let next = detect_names(&["package.json", "next.config.js"], &["app"]);
        assert_eq!(next.ecosystem.as_deref(), Some("Next.js"));

        let vite = detect_names(&["package.json", "vite.config.ts"], &["src"]);
        assert_eq!(vite.ecosystem.as_deref(), Some("Vite"));

        let angular = detect_names(&["package.json", "angular.json"], &["src"]);
        assert_eq!(angular.ecosystem.as_deref(), Some("Angular"));
    }

    #[test]
    fn nothing_recognisable_has_no_ecosystem() {
        assert_eq!(detect_names(&["notes.txt"], &["photos"]).ecosystem, None);
    }

    #[test]
    fn workspace_markers_identify_a_monorepo() {
        assert!(detect_names(&["package.json", "pnpm-workspace.yaml"], &[]).is_monorepo);
        assert!(detect_names(&["package.json", "turbo.json"], &[]).is_monorepo);
        assert!(detect_names(&["package.json", "nx.json"], &[]).is_monorepo);
        assert!(detect_names(&["Cargo.toml"], &["crates"]).is_monorepo);
        assert!(detect_names(&["package.json"], &["packages"]).is_monorepo);
        assert!(detect_names(&["package.json"], &["apps"]).is_monorepo);
    }

    #[test]
    fn an_ordinary_project_is_not_a_monorepo() {
        assert!(!detect_names(&["package.json", "package-lock.json"], &["src"]).is_monorepo);
    }
}
