//! Finding and driving the Codex CLI.
//!
//! The CLI is not always on `PATH`: the VS Code extension and the desktop app
//! ship their own copy, which is the normal install for most users. Relay walks
//! the same candidates the session resolver uses, so both agree on which Codex is
//! in charge.

use std::path::PathBuf;
use std::time::Duration;

use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

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

pub async fn find_codex_cli() -> Option<CodexExecutable> {
    for candidate in codex_candidates() {
        if candidate != "codex" && !PathBuf::from(&candidate).is_file() {
            continue;
        }
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
}
