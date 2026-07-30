//! Compiles the minisign public key into the binary.
//!
//! It is read from `tauri.conf.json`, which is where the in-app updater's key
//! already lives, so the two cannot drift apart and there is one place to
//! change if the key is ever rotated. A key that arrived with the release it is
//! meant to authenticate would authenticate nothing.

use std::path::PathBuf;

fn main() {
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src-tauri/tauri.conf.json");

    println!("cargo:rerun-if-changed={}", config.display());

    let text = match std::fs::read_to_string(&config) {
        Ok(text) => text,
        Err(error) => {
            println!("cargo:warning=cannot read {}: {error}", config.display());
            std::process::exit(1);
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            println!(
                "cargo:warning={} is not valid JSON: {error}",
                config.display()
            );
            std::process::exit(1);
        }
    };

    let key = parsed
        .get("plugins")
        .and_then(|plugins| plugins.get("updater"))
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|pubkey| pubkey.as_str());

    match key {
        Some(key) if !key.is_empty() => {
            println!("cargo:rustc-env=PANEL_MINISIGN_PUBKEY={key}");
        }
        _ => {
            // Failing the build is the only safe answer. A stub built without a
            // key could only either refuse everything or verify nothing, and
            // the second is the kind of thing that ships by accident.
            println!(
                "cargo:warning=plugins.updater.pubkey is missing from {}",
                config.display()
            );
            std::process::exit(1);
        }
    }
}
