//! Writes the generated contract to disk.
//!
//! ```text
//! cargo run -p project-host-api-types --bin emit-contracts
//! cargo run -p project-host-api-types --bin emit-contracts -- --check
//! ```
//!
//! `--check` regenerates in memory and compares, exiting non-zero on any
//! difference. That is what CI runs: a Rust type changed without regenerating
//! the TypeScript cannot merge.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use project_host_api_types::{codegen, contract_schema};

struct Output {
    path: PathBuf,
    contents: String,
}

fn main() -> ExitCode {
    let check_only = std::env::args().any(|arg| arg == "--check");

    let root = match workspace_root() {
        Ok(root) => root,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };

    let schema = contract_schema();
    let schema_json = match serde_json::to_string_pretty(&schema) {
        Ok(json) => format!("{json}\n"),
        Err(error) => {
            eprintln!("error: could not serialise the schema: {error}");
            return ExitCode::FAILURE;
        }
    };

    let (typescript, zod) = match codegen::generate(&schema) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("error: contract generation failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    let outputs = vec![
        Output {
            path: root.join("contracts/schema.json"),
            contents: schema_json,
        },
        Output {
            path: root.join("packages/shared-types/src/generated.ts"),
            contents: typescript,
        },
        Output {
            path: root.join("packages/api-contracts/src/generated.ts"),
            contents: zod,
        },
    ];

    let mut stale = Vec::new();
    for output in &outputs {
        match apply(output, check_only) {
            Ok(true) => stale.push(output.path.clone()),
            Ok(false) => {}
            Err(message) => {
                eprintln!("error: {message}");
                return ExitCode::FAILURE;
            }
        }
    }

    if check_only {
        if stale.is_empty() {
            println!("contracts are up to date");
            return ExitCode::SUCCESS;
        }
        eprintln!("error: generated contracts are out of date:");
        for path in &stale {
            eprintln!("  {}", path.display());
        }
        eprintln!("\nrun: cargo run -p project-host-api-types --bin emit-contracts");
        return ExitCode::FAILURE;
    }

    for output in &outputs {
        println!("wrote {}", output.path.display());
    }
    ExitCode::SUCCESS
}

/// Returns `Ok(true)` when the file on disk differs from what was generated.
fn apply(output: &Output, check_only: bool) -> Result<bool, String> {
    let existing = match fs::read_to_string(&output.path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!("could not read {}: {error}", output.path.display()));
        }
    };

    // Compare with line endings normalised: a checkout with CRLF must not look
    // stale to a generator that emits LF.
    let differs = existing
        .as_deref()
        .map(|contents| normalise(contents) != normalise(&output.contents))
        .unwrap_or(true);

    if check_only || !differs {
        return Ok(differs);
    }

    if let Some(parent) = output.path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(&output.path, &output.contents)
        .map_err(|error| format!("could not write {}: {error}", output.path.display()))?;
    Ok(true)
}

fn normalise(contents: &str) -> String {
    contents.replace("\r\n", "\n")
}

/// Walks up from this crate to the directory holding the workspace manifest.
fn workspace_root() -> Result<PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut current = Some(manifest);
    while let Some(directory) = current {
        if directory.join("pnpm-workspace.yaml").is_file() {
            return Ok(directory.to_path_buf());
        }
        current = directory.parent();
    }
    Err(format!(
        "could not locate the workspace root above {}",
        manifest.display()
    ))
}
