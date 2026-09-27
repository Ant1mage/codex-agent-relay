//! Relay's desktop shell binary.
//!
//! Everything lives in the library, so the menu model, the panel URL builder and
//! the daemon rules stay unit testable without a GUI session.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    relay_desktop::run();
}
