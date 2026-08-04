//! Print this machine's snapshot as JSON.
//!
//! `cargo run -p project-host-platform --example scan`
//!
//! Exists so that a support request can carry the exact scan the application
//! saw, and so the platform-specific probes can be exercised on a real machine
//! without starting the desktop shell.

fn main() {
    use project_host_platform::SystemProbe;

    let snapshot = project_host_platform::SystemScanner.snapshot();
    match serde_json::to_string_pretty(&snapshot) {
        Ok(json) => println!("{json}"),
        Err(error) => eprintln!("could not serialise the snapshot: {error}"),
    }
}
