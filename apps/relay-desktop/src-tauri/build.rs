//! Build glue for Relay's desktop shell.
//!
//! `tauri.conf.json` references two things that only exist after a build:
//!
//! * `bundle.resources` carries `relayd` and `relay-mcp` from `target/release`
//!   into `Contents/Resources/bin`, where the shell and the Codex integration
//!   look for them. They are ordinary workspace binaries, so
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
//!
//! A bundled app also needs the updater public key. With an empty
//! `plugins.updater.pubkey` it can never verify a downloaded update, so the
//! updater stays permanently unsupported. `RELAY_REQUIRE_UPDATER_PUBKEY=1` (set
//! by the release workflow) turns that into a build error instead of a silent,
//! un-updatable release; ordinary local builds are unaffected.

use std::path::{Path, PathBuf};

/// Set to `1` (or `true`) to require a non-empty updater public key for this
/// build. The release workflow sets it; it is unset for local builds.
const REQUIRE_UPDATER_PUBKEY_ENV: &str = "RELAY_REQUIRE_UPDATER_PUBKEY";

fn main() {
    check_sidecars();
    ensure_ui_output_directory();
    check_updater_pubkey();
    tauri_build::build();
}

/// `bundle.resources` copies these two into `Contents/Resources/bin`.
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

// ---------------------------------------------------------------------------
// The updater public key
// ---------------------------------------------------------------------------

/// Fails the build when the release workflow asked for a signed updater feed but
/// the effective configuration carries no public key.
fn check_updater_pubkey() {
    println!("cargo:rerun-if-env-changed={REQUIRE_UPDATER_PUBKEY_ENV}");
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    if !matches!(
        std::env::var(REQUIRE_UPDATER_PUBKEY_ENV).as_deref(),
        Ok("1") | Ok("true")
    ) {
        return;
    }
    let config_path = manifest_dir().join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", display(&config_path));
    match effective_updater_pubkey(&config_path) {
        Ok(pubkey) if !pubkey.trim().is_empty() => {}
        Ok(_) => panic!("{}", empty_updater_pubkey_message(&config_path)),
        Err(error) => panic!(
            "{REQUIRE_UPDATER_PUBKEY_ENV} is set, but the updater configuration could not be read: {error}"
        ),
    }
}

/// The public key the packaged app will actually carry.
///
/// Tauri's CLI merges every `--config` override into the file configuration and
/// hands that merge to this build script in `TAURI_CONFIG`; a plain `cargo build`
/// has no overlay at all. The overlay wins whenever it names the key — exactly
/// like Tauri's own merge — and `tauri.conf.json` is the fallback.
fn effective_updater_pubkey(config_path: &Path) -> Result<String, String> {
    if let Ok(overlay) = std::env::var("TAURI_CONFIG") {
        if let Some(pubkey) = updater_pubkey_in(&overlay) {
            return Ok(pubkey);
        }
    }
    let text = std::fs::read_to_string(config_path)
        .map_err(|error| format!("{} could not be read: {error}", display(config_path)))?;
    updater_pubkey_in(&text).ok_or_else(|| {
        format!(
            "{} does not name plugins.updater.pubkey",
            display(config_path)
        )
    })
}

fn empty_updater_pubkey_message(config_path: &Path) -> String {
    format!(
        "the updater public key is empty, but {REQUIRE_UPDATER_PUBKEY_ENV} is set. \
A release with an empty plugins.updater.pubkey can never verify a downloaded update, \
so the packaged app reports the updater as unsupported forever. \
The public key is not a secret: put the public half of TAURI_SIGNING_PRIVATE_KEY \
(`cargo tauri signer generate`) in the TAURI_UPDATER_PUBKEY repository secret, which \
the release workflow passes to the build as the --config override that sets \
plugins.updater.pubkey. \
Checked the TAURI_CONFIG merge and {}.",
        display(config_path)
    )
}

/// `plugins.updater.pubkey` from a Tauri configuration document, when the
/// document names it at all.
///
/// The build script cannot use `serde_json` — it is not a build-dependency of
/// this crate — and exactly one value is needed, so a small scanner walks the
/// document (objects, arrays, strings, scalars) while tracking its key path,
/// instead of linking a JSON parser for it. Escaped key names are not decoded,
/// and keys other than the one above are ignored: that is all a configuration
/// file written by Tauri's CLI needs.
fn updater_pubkey_in(json: &str) -> Option<String> {
    let mut scanner = Scanner {
        text: json.as_bytes(),
        at: 0,
    };
    let mut path: Vec<&str> = Vec::new();
    let mut found = None;
    scanner.value(&mut path, &mut found);
    found
}

/// The object path holding the updater's public key.
const UPDATER_PUBKEY_PATH: [&str; 3] = ["plugins", "updater", "pubkey"];
/// Stands in for an array in the path, so nothing inside one can match.
const ARRAY_MARKER: &str = "[]";

struct Scanner<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Scanner<'a> {
    fn byte(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// One JSON value. `path` is the key path that led here.
    fn value(&mut self, path: &mut Vec<&'a str>, found: &mut Option<String>) {
        self.skip_whitespace();
        match self.byte() {
            Some(b'{') => self.object(path, found),
            Some(b'[') => self.array(path, found),
            Some(b'"') => {
                let text = self.string();
                if path.as_slice() == UPDATER_PUBKEY_PATH.as_slice() {
                    *found = text.map(str::to_string);
                }
            }
            Some(_) => self.scalar(),
            None => {}
        }
    }

    fn object(&mut self, path: &mut Vec<&'a str>, found: &mut Option<String>) {
        self.at += 1; // '{'
        loop {
            self.skip_whitespace();
            match self.byte() {
                Some(b'}') => {
                    self.at += 1;
                    return;
                }
                Some(b',') => self.at += 1,
                Some(b'"') => {
                    let Some(key) = self.string() else { return };
                    self.skip_whitespace();
                    if self.byte() != Some(b':') {
                        return;
                    }
                    self.at += 1; // ':'
                    path.push(key);
                    self.value(path, found);
                    path.pop();
                }
                // Malformed input: step over the byte and keep looking for the
                // closing brace, so a broken document cannot loop forever.
                Some(_) => self.at += 1,
                None => return,
            }
        }
    }

    fn array(&mut self, path: &mut Vec<&'a str>, found: &mut Option<String>) {
        // Nothing inside an array can be \`plugins.updater.pubkey\`, so the marker
        // keeps a nested object from matching that path.
        path.push(ARRAY_MARKER);
        self.at += 1; // '['
        loop {
            self.skip_whitespace();
            match self.byte() {
                Some(b']') => {
                    self.at += 1;
                    break;
                }
                Some(b',') => self.at += 1,
                Some(_) => self.value(path, found),
                None => break,
            }
        }
        path.pop();
    }

    fn scalar(&mut self) {
        while let Some(byte) = self.byte() {
            if matches!(byte, b',' | b'}' | b']') || byte.is_ascii_whitespace() {
                return;
            }
            self.at += 1;
        }
    }

    /// The contents of the string at the cursor, without its quotes. Escapes are
    /// skipped over so the closing quote is found, but they are not decoded:
    /// only key names and a base64 public key are ever read from a Tauri config.
    fn string(&mut self) -> Option<&'a str> {
        if self.byte() != Some(b'"') {
            return None;
        }
        self.at += 1;
        let start = self.at;
        loop {
            match self.byte() {
                None => return None,
                Some(b'"') => {
                    let text = std::str::from_utf8(&self.text[start..self.at]).ok()?;
                    self.at += 1;
                    return Some(text);
                }
                Some(b'\\') => self.at += 2,
                Some(_) => self.at += 1,
            }
        }
    }
}
