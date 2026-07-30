//! Panel Platform setup.
//!
//! With no arguments it opens a window. `--silent` runs the identical pipeline
//! with text output, so it works over SSH and is what CI exercises; `--dry-run`
//! stops after verification without changing the machine.

// A GUI program should not also open a console window on Windows. `--silent`
// still prints, because a console attached by the shell is inherited.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ui;

use std::sync::atomic::AtomicBool;

use panel_platform_setup as setup;
use setup::Stage;

const USAGE: &str = "\
Panel Platform setup

    panel-platform-setup [options]

Downloads the latest Panel Platform installer for this machine, checks it
against Panel Platform's signature, and starts it.

Options:
    --silent     No window. Report progress as text.
    --dry-run    Stop after verifying. Nothing is installed.
    --version    Print this program's version.
    --help       Print this message.
";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|arg| arg == name);

    if flag("--help") || flag("-h") {
        print!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }

    if flag("--version") {
        println!("panel-platform-setup {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }

    let dry_run = flag("--dry-run");

    if flag("--silent") || dry_run {
        silent(dry_run)
    } else {
        window()
    }
}

/// The same pipeline the window drives, printed instead of drawn.
fn silent(dry_run: bool) -> std::process::ExitCode {
    let agent = setup::net::agent();

    let resolved = match setup::resolve(&agent) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    println!(
        "Panel Platform {} — {} ({})",
        resolved.offer.version,
        resolved.offer.kind.describe(),
        resolved.offer.size()
    );
    println!("  {}", resolved.offer.asset);

    let cancel = AtomicBool::new(false);
    let mut last_percent = u64::MAX;

    let result = setup::install(&agent, &resolved, dry_run, &cancel, &mut |stage| {
        match stage {
            Stage::Downloading { done, total } if total > 0 => {
                // Every ten percent, so a log is readable rather than a wall of
                // carriage returns nobody can scroll through.
                let percent = done * 100 / total;
                if percent / 10 != last_percent / 10 {
                    last_percent = percent;
                    println!("  downloading… {percent}%");
                }
            }
            Stage::Downloading { .. } | Stage::Checking => {}
            Stage::Verifying => println!("  verifying signature and checksum…"),
            Stage::Installing => println!("  starting the installer…"),
        }
    });

    match result {
        Ok(()) if dry_run => {
            println!("verified. Nothing was installed (--dry-run).");
            std::process::ExitCode::SUCCESS
        }
        Ok(()) => {
            println!("the installer has been started.");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn window() -> std::process::ExitCode {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([480.0, 260.0])
            .with_resizable(false)
            .with_title("Panel Platform Setup"),
        ..Default::default()
    };

    let started = eframe::run_native(
        "Panel Platform Setup",
        options,
        Box::new(|context| Ok(Box::new(ui::App::new(&context.egui_ctx)))),
    );

    match started {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // A machine with no display is not a broken installer, and the
            // answer is a flag rather than a stack trace.
            eprintln!("could not open a window: {error}");
            eprintln!("run with --silent to install without one.");
            std::process::ExitCode::FAILURE
        }
    }
}
