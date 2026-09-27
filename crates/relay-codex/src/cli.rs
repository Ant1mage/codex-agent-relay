//! Finding and driving the Codex CLI.
//!
//! The CLI is not always on `PATH`: the VS Code extension and the desktop app
//! ship their own copy, which is the normal install for most users. Relay walks
//! the same candidates the session resolver uses, so both agree on which Codex is
//! in charge.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// How the Codex CLI is stored inside the ChatGPT desktop app. Both shapes ship
/// in the same bundle: `bin/codex` is the plain executable and
/// `CodexCLI.app/Contents/MacOS/codex` is the helper app the ChatGPT window
/// launches. Neither is ever put on `PATH`.
const CHATGPT_APP_CODEX_PATHS: [&str; 2] = [
    "Contents/Resources/codex-cli/bin/codex",
    "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecutable {
    pub path: String,
    pub version: String,
}

pub fn codex_candidates() -> Vec<String> {
    let home = relay_config::paths::home_dir();
    let codex_home = relay_config::codex_home();
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(explicit) = std::env::var("CODEX_PATH") {
        if !explicit.is_empty() {
            candidates.push(explicit);
        }
    }
    candidates.push("codex".to_string());
    candidates.push(codex_home.join("bin/codex").display().to_string());
    candidates.push(home.join(".local/bin/codex").display().to_string());
    candidates.push(home.join(".npm-global/bin/codex").display().to_string());
    candidates.push("/usr/local/bin/codex".to_string());
    candidates.push("/opt/homebrew/bin/codex".to_string());
    // The ChatGPT desktop app carries a full Codex CLI instead of installing one
    // on `PATH`, and it can be installed for the whole machine or for one user.
    for root in [PathBuf::from("/Applications"), home.join("Applications")] {
        let app = root.join("ChatGPT.app");
        for relative in CHATGPT_APP_CODEX_PATHS {
            candidates.push(app.join(relative).display().to_string());
        }
    }
    for root in [
        home.join(".vscode/extensions"),
        home.join(".vscode-insiders/extensions"),
    ] {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("openai.chatgpt-"))
                    .unwrap_or(false)
            })
            .collect();
        versions.sort();
        versions.reverse();
        for version in versions {
            for target in [
                "macos-aarch64",
                "macos-x86_64",
                "linux-x86_64",
                "linux-aarch64",
            ] {
                candidates.push(
                    version
                        .join("bin")
                        .join(target)
                        .join("codex")
                        .display()
                        .to_string(),
                );
            }
        }
    }
    candidates
}

/// Whether a candidate may be spawned at all.
///
/// A bare `codex` is resolved through `PATH` by the operating system, so it is
/// passed on untouched. Every other candidate has to be an *executable* file: a
/// directory, a dangling symlink or a plain file that merely carries the name
/// would otherwise be spawned and surface as a mysterious failure. The
/// executable bit is exactly the test `execvp` performs before it runs a path.
pub fn is_spawnable_candidate(candidate: &str) -> bool {
    if candidate == "codex" {
        return true;
    }
    is_executable_file(Path::new(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // `metadata` follows symlinks: a link to an executable is executable.
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

pub async fn find_codex_cli() -> Option<CodexExecutable> {
    for candidate in codex_candidates() {
        if !is_spawnable_candidate(&candidate) {
            continue;
        }
        // Every candidate reports its own version. Two Codex installs on one
        // machine routinely differ, so a version is never carried over from a
        // neighbouring candidate: a candidate that cannot answer `--version` is
        // simply the wrong one to hand to the model.
        if let Some(version) = version_of(&candidate).await {
            return Some(CodexExecutable {
                path: candidate,
                version,
            });
        }
    }
    None
}

async fn version_of(executable: &str) -> Option<String> {
    let output = tokio::time::timeout(TIMEOUT, Command::new(executable).arg("--version").output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(if text.is_empty() {
        "unknown version".to_string()
    } else {
        text
    })
}

pub struct CodexOutput {
    pub ok: bool,
    pub text: String,
}

pub async fn codex(executable: &CodexExecutable, args: &[&str]) -> CodexOutput {
    match tokio::time::timeout(
        COMMAND_TIMEOUT,
        Command::new(&executable.path).args(args).output(),
    )
    .await
    {
        Ok(Ok(output)) => CodexOutput {
            ok: output.status.success(),
            text: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        },
        Ok(Err(error)) => CodexOutput {
            ok: false,
            text: error.to_string(),
        },
        Err(_) => CodexOutput {
            ok: false,
            text: "codex command timed out".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_include_the_ide_extension_locations() {
        let candidates = codex_candidates();
        assert!(candidates.contains(&"codex".to_string()));
        assert!(candidates
            .iter()
            .any(|candidate| candidate.contains(".codex/bin/codex")));
        assert!(candidates
            .iter()
            .any(|candidate| candidate.contains(".local/bin/codex")));
    }

    /// The ChatGPT desktop app is a normal install, so its bundled CLI has to be
    /// on the list for both the machine-wide and the per-user location — without
    /// the app having to be installed for the test to hold.
    #[test]
    fn candidates_include_the_chatgpt_desktop_app() {
        let candidates = codex_candidates();
        let home = relay_config::paths::home_dir();
        for root in [PathBuf::from("/Applications"), home.join("Applications")] {
            let app = root.join("ChatGPT.app");
            for relative in CHATGPT_APP_CODEX_PATHS {
                let expected = app.join(relative).display().to_string();
                assert!(
                    candidates.contains(&expected),
                    "codex_candidates() is missing {expected}"
                );
            }
        }
    }

    /// A file that only shares the name must never be spawned: without the
    /// executable bit the kernel would refuse it anyway, and the failure would
    /// surface far away from the candidate list.
    #[cfg(unix)]
    #[test]
    fn a_non_executable_file_with_the_right_name_is_not_a_candidate() {
        let directory = scratch_directory("non-executable");
        let candidate = directory.join("codex");
        std::fs::write(
            &candidate,
            b"#!/bin/sh
echo codex-cli 0.0.0
",
        )
        .unwrap();
        assert!(candidate.is_file());
        assert!(
            !is_spawnable_candidate(&candidate.display().to_string()),
            "a file without the executable bit must be skipped"
        );

        make_executable(&candidate);
        assert!(is_spawnable_candidate(&candidate.display().to_string()));

        // A directory is not a candidate, executable bit or not, and neither is
        // a path that does not exist.
        assert!(!is_spawnable_candidate(&directory.display().to_string()));
        assert!(!is_spawnable_candidate(
            &directory.join("missing").display().to_string()
        ));

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The only thing that proves a candidate works is running it; the version
    /// that comes back is that candidate's own.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_candidate_is_validated_by_running_it() {
        let directory = scratch_directory("version");
        let working = directory.join("codex");
        std::fs::write(
            &working,
            b"#!/bin/sh
echo codex-cli 9.9.9
",
        )
        .unwrap();
        make_executable(&working);
        assert_eq!(
            version_of(&working.display().to_string()).await.as_deref(),
            Some("codex-cli 9.9.9")
        );

        let broken = directory.join("codex-broken");
        std::fs::write(
            &broken,
            b"#!/bin/sh
exit 1
",
        )
        .unwrap();
        make_executable(&broken);
        assert!(version_of(&broken.display().to_string()).await.is_none());

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A unique, writable scratch directory. `tempfile` is not a dependency of
    /// this crate, so the tests create and remove their own.
    fn scratch_directory(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let directory = std::env::temp_dir().join(format!(
            "relay-codex-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
}
