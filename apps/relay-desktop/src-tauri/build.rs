//! Build glue for Relay's desktop shell.
//!
//! Tauri needs its configured sidecars and frontend output directory to exist
//! while this script runs. Release builds require real daemon binaries; debug
//! builds get empty placeholders so a checkout can compile before those bins
//! are built. The Leptos UI is built separately with Trunk.

use std::path::PathBuf;

fn main() {
    check_sidecars();
    ensure_ui_output_directory();
    tauri_build::build();
}

fn check_sidecars() {
    let release = std::env::var("PROFILE")
        .map(|profile| profile == "release")
        .unwrap_or(false);
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
            ui_dist.display()
        ),
        Err(error) => println!(
            "cargo:warning=could not create {}: {error}",
            ui_dist.display()
        ),
    }
}
