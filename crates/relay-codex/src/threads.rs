//! Resolving a Codex thread into a Relay host session.
//!
//! Relay never invents a session name. The `SessionStart` hook supplies the
//! thread id; the real name, working directory and model are read from the local
//! Codex app-server over JSON-RPC.

use std::process::Stdio;
use std::time::Duration;

use relay_core::{RelayError, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::cli::codex_candidates;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexThreadMetadata {
    pub id: String,
    pub display_name: String,
    pub cwd: String,
    pub model: Option<String>,
}

#[async_trait::async_trait]
pub trait CodexThreadMetadataResolver: Send + Sync {
    async fn resolve(&self, thread_id: &str) -> Result<CodexThreadMetadata>;
}

pub struct CodexAppServerThreadResolver {
    executable_path: Option<String>,
    timeout: Duration,
}

impl Default for CodexAppServerThreadResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAppServerThreadResolver {
    pub fn new() -> Self {
        Self { executable_path: None, timeout: Duration::from_secs(5) }
    }

    pub fn with_executable(executable: impl Into<String>, timeout: Duration) -> Self {
        Self { executable_path: Some(executable.into()), timeout }
    }

    async fn resolve_with(&self, executable: &str, thread_id: &str) -> Result<CodexThreadMetadata> {
        let mut command = Command::new(executable);
        command
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            RelayError::new("RUNTIME_NOT_FOUND", format!("{executable}: {error}"))
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| RelayError::new("RUNTIME_NOT_FOUND", "codex app-server stdin is unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RelayError::new("RUNTIME_NOT_FOUND", "codex app-server stdout is unavailable"))?;

        let initialize = serde_json::json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "relay", "title": "Relay", "version": relay_config::relay_version() },
                "capabilities": null,
            }
        });
        let write = async {
            stdin.write_all(format!("{initialize}\n").as_bytes()).await?;
            stdin.flush().await
        };
        write
            .await
            .map_err(|error| RelayError::new("RUNTIME_NOT_FOUND", format!("codex app-server: {error}")))?;

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut asked_for_thread = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(RelayError::new(
                    "SESSION_NAME_UNAVAILABLE",
                    format!("Timed out reading Codex thread {thread_id}"),
                ));
            }
            let line = match tokio::time::timeout(remaining, lines.next_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => {
                    return Err(RelayError::new(
                        "SESSION_NAME_UNAVAILABLE",
                        "Codex app-server exited unexpectedly",
                    ))
                }
                Ok(Err(error)) => {
                    return Err(RelayError::new("SESSION_NAME_UNAVAILABLE", error.to_string()))
                }
                Err(_) => {
                    return Err(RelayError::new(
                        "SESSION_NAME_UNAVAILABLE",
                        format!("Timed out reading Codex thread {thread_id}"),
                    ))
                }
            };
            let Ok(response) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            match response.get("id").and_then(|value| value.as_i64()) {
                Some(1) => {
                    if !asked_for_thread {
                        asked_for_thread = true;
                        let initialized = serde_json::json!({ "method": "initialized" });
                        let request = serde_json::json!({
                            "method": "thread/read",
                            "id": 2,
                            "params": { "threadId": thread_id, "includeTurns": false }
                        });
                        let _ = stdin
                            .write_all(format!("{initialized}\n{request}\n").as_bytes())
                            .await;
                        let _ = stdin.flush().await;
                    }
                }
                Some(2) => {
                    if let Some(error) = response.get("error") {
                        let message = error
                            .get("message")
                            .and_then(|value| value.as_str())
                            .unwrap_or("Codex thread was not found")
                            .to_string();
                        return Err(RelayError::new("SESSION_NAME_UNAVAILABLE", message));
                    }
                    let thread = response.get("result").and_then(|result| result.get("thread"));
                    let Some(thread) = thread else {
                        return Err(RelayError::new(
                            "SESSION_NAME_UNAVAILABLE",
                            format!("Codex returned no metadata for thread {thread_id}"),
                        ));
                    };
                    let name = thread
                        .get("name")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty());
                    let preview = thread
                        .get("preview")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty());
                    let Some(display_name) = name.or(preview) else {
                        return Err(RelayError::new(
                            "SESSION_NAME_UNAVAILABLE",
                            format!("Codex did not provide a display name for thread {thread_id}"),
                        ));
                    };
                    let (Some(id), Some(cwd)) = (
                        thread.get("id").and_then(|value| value.as_str()),
                        thread.get("cwd").and_then(|value| value.as_str()),
                    ) else {
                        return Err(RelayError::new(
                            "SESSION_NAME_UNAVAILABLE",
                            format!("Codex returned invalid metadata for thread {thread_id}"),
                        ));
                    };
                    return Ok(CodexThreadMetadata {
                        id: id.to_string(),
                        display_name: display_name.to_string(),
                        cwd: cwd.to_string(),
                        model: thread.get("model").and_then(|value| value.as_str()).map(str::to_string),
                    });
                }
                _ => {}
            }
        }
    }
}

#[async_trait::async_trait]
impl CodexThreadMetadataResolver for CodexAppServerThreadResolver {
    /// Tries the configured path first, then the locations Codex installs itself
    /// into. A missing executable must not surface as a spawn error to the model.
    async fn resolve(&self, thread_id: &str) -> Result<CodexThreadMetadata> {
        let mut candidates: Vec<String> = Vec::new();
        if let Some(configured) = &self.executable_path {
            candidates.push(configured.clone());
        } else {
            candidates.push("codex".to_string());
            candidates.extend(codex_candidates());
        }
        let mut last_error: Option<RelayError> = None;
        let mut seen: Vec<String> = Vec::new();
        for candidate in candidates {
            if seen.contains(&candidate) {
                continue;
            }
            seen.push(candidate.clone());
            if candidate != "codex" && !std::path::Path::new(&candidate).is_file() {
                continue;
            }
            match self.resolve_with(&candidate, thread_id).await {
                Ok(metadata) => return Ok(metadata),
                Err(error) => {
                    let missing = error.message().contains("No such file") || error.message().contains("not found");
                    last_error = Some(error);
                    // Only a missing binary is worth retrying with the next candidate.
                    if !missing {
                        break;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            RelayError::new("RUNTIME_NOT_FOUND", "Codex CLI was not found")
        }))
    }
}
