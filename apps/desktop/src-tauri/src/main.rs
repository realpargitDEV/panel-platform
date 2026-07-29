// Hide the console window on Windows in a release build. Left visible in
// development, where the log output is the fastest way to see what happened.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = project_host_desktop_lib::run() {
        // Nothing has a window yet at this point, so the console and the exit
        // code are the only ways to report a failure to start.
        eprintln!("Panel Platform could not start: {error}");
        std::process::exit(1);
    }
}
