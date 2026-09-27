//! Build glue for Relay's desktop shell.
//!
//! `tauri.conf.json` references two things that only exist after a build:
//!
//! * `bundle.resources` carries `relayd` and `relay-mcp` from `target/release`
//!   into `Contents/Resources`, where the shell and the Codex integration look
//!   for them. They are ordinary workspace binaries, so
//!   `cargo build --release -p relayd -p relay-mcp` produces them and nothing has
//!   to be staged by hand. Tauri requires every resource path to exist while the
//!   build script runs, so a debug build records a warning and leaves an empty
//!   placeholder; a release build refuses to continue unless the real binaries
//!   are there, because a bundle must never ship an empty daemon.
//!
//! * `build.frontendDist` — the Trunk-built UI at `apps/relay-desktop/ui/dist`.
//!   That directory belongs to the Leptos crate and is built separately
//!   (`trunk build --release`); this script only creates the empty output
//!   directory when it is missing, so a checkout where the UI has not been built
//!   yet can still compile the shell. Nothing inside `ui/` is ever written.

use std::path::{Path, PathBuf};

fn main() {
    check_sidecars();
    ensure_ui_output_directory();
    tauri_build::build();
}

/// `bundle.resources` copies these two into `Contents/Resources`.
fn check_sidecars() {
    let release = std::env::var("PROFILE")
        .map(|profile| profile == "release")
        .unwrap_or(false);
    let target = std::env::var("TARGET").unwrap_or_else(|_| "aarch64-apple-darwin".to_string());
    let _ = target;
    for name in ["relayd", "relay-mcp"] {
        let binary = manifest_dir().join("../../../target/release").join(name);
        let staged = binary
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if staged > 100_000 {
            continue;
        }
        if release {
            panic!(
                "the {name} binary is missing from target/release (or is an empty placeholder).\n                 A release bundle must contain a real daemon and MCP server:\n                 \x20 cargo build --release -p relayd -p relay-mcp"
            );
        }
        if let Some(parent) = binary.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&binary, []) {
            println!("cargo:warning=could not record a {name} placeholder: {error}");
        } else {
            println!(
                "cargo:warning={name} is not built in release mode yet; recorded an empty placeholder so this development build can link. Release builds require `cargo build --release -p relayd -p relay-mcp` first."
            );
        }
    }
}

fn manifest_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("cargo sets CARGO_MANIFEST_DIR")
}

fn ensure_ui_output_directory() {
    let ui_dist = manifest_dir().join("../ui/dist");
    if ui_dist.is_dir() {
        return;
    }
    match std::fs::create_dir_all(&ui_dist) {
        Ok(()) => println!(
            "cargo:warning={} did not exist; created the empty directory. Build the UI with `trunk build --release` in apps/relay-desktop/ui.",
            display(&ui_dist)
        ),
        Err(error) => println!("cargo:warning=could not create {}: {error}", display(&ui_dist)),
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}
