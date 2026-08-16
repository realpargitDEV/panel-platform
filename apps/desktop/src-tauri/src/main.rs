// Hide the console window on Windows in a release build. Left visible in
// development, where the log output is the fastest way to see what happened.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // A static project is served by this executable in a second mode, so the
    // argument list is checked before anything else happens. It has to be
    // before Tauri: this process is a plain server here, with no window, no
    // database and no state, and building any of that first would mean a
    // static site could not start until the whole application had.
    //
    // Spawning ourselves rather than depending on a web server the user may not
    // have is what lets a static site run on a machine with no toolchain at all
    // — which is the whole point of a static site.
    if let Some(request) = project_host_desktop_lib::static_server_request() {
        if let Err(error) = project_host_desktop_lib::serve_static(request) {
            eprintln!("the static site could not be served: {error}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(error) = project_host_desktop_lib::run() {
        // Nothing has a window yet at this point, so the console and the exit
        // code are the only ways to report a failure to start.
        eprintln!("Panel Platform could not start: {error}");
        std::process::exit(1);
    }
}
