//! Relay's desktop shell binary.
//!
//! Everything lives in the library, so the menu model, the panel URL builder and
//! the daemon rules stay unit testable without a GUI session.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // GUI-launched apps do not inherit the user's shell PATH. Restore it before
    // the bundled daemon starts so JS CLIs such as dsh can find Node.
    if let Err(error) = fix_path_env::fix() {
        eprintln!("[relay] could not load the user's shell PATH: {error}");
    }
    relay_desktop::run();
}
