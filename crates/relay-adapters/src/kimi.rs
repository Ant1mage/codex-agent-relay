//! Kimi Code (`kimi`).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, DetectionResult, RelayError, RelayEventType,
    Result, Runtime, RuntimeHealth, RuntimeOptions, StartInput, WorkerHandle,
};

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::probe::{
    discover_executable, probe_runtime_options, read_help, version_of, with_selection_args,
    Selection,
};

pub const ADAPTER_ID: &str = "kimi-code";
pub const RUNTIME_ID: &str = "runtime:kimi-code";

fn text_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .map(|part| {
                part.get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
            })
            .collect(),
        _ => String::new(),
    }
}

fn tool_event_type(tool: &str) -> RelayEventType {
    let name = tool.to_lowercase();
    if ["search", "grep", "glob", "find", "fetch"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        RelayEventType::ToolSearch
    } else if ["edit", "write", "patch", "replace", "delete", "move"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        RelayEventType::ToolEdit
    } else if ["read", "view", "open", "list"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        RelayEventType::ToolRead
    } else if name.contains("test") {
        RelayEventType::TestResult
    } else {
        RelayEventType::ToolCommand
    }
}

pub fn parse_line(line: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(line) else {
        parsed.events.push(AdapterEvent::with_native(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "diagnostic", "message": "Kimi Code emitted malformed JSON" }),
            serde_json::Value::String(line.chars().take(8_192).collect()),
        ));
        return parsed;
    };
    let Some(message) = raw.as_object() else {
        return parsed;
    };
    match message.get("role").and_then(|value| value.as_str()) {
        Some("meta") => {
            parsed.session_id = message
                .get("session_id")
                .or_else(|| message.get("sessionId"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
        }
        Some("assistant") => {
            let text = text_content(message.get("content").unwrap_or(&serde_json::Value::Null));
            if !text.is_empty() {
                parsed.events.push(AdapterEvent::with_native(
                    RelayEventType::WorkerMessage,
                    serde_json::json!({ "text": text }),
                    raw.clone(),
                ));
                parsed.final_text = Some(text);
            }
            if let Some(calls) = message.get("tool_calls").and_then(|value| value.as_array()) {
                for call in calls {
                    let function = call
                        .get("function")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let tool = function
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    parsed.events.push(AdapterEvent::with_native(
                        tool_event_type(&tool),
                        serde_json::json!({
                            "callId": call.get("id").cloned().unwrap_or(serde_json::Value::Null),
                            "tool": tool,
                            "arguments": function.get("arguments").cloned().unwrap_or(serde_json::Value::Null),
                        }),
                        raw.clone(),
                    ));
                }
            }
        }
        Some("tool") => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::ToolResult,
                serde_json::json!({
                    "callId": message.get("tool_call_id").cloned().unwrap_or(serde_json::Value::Null),
                    "output": text_content(message.get("content").unwrap_or(&serde_json::Value::Null)),
                }),
                raw,
            ));
        }
        _ => {}
    }
    parsed
}

fn terminal_event(outcome: &StreamOutcome) -> AdapterEvent {
    if outcome.exited_cleanly() {
        AdapterEvent::new(
            RelayEventType::WorkerCompleted,
            serde_json::json!({
                "summary": outcome.final_text.clone().unwrap_or_default(),
                "exitCode": outcome.exit_code,
            }),
        )
    } else {
        let message = outcome
            .spawn_error
            .clone()
            .or_else(|| outcome.error_message.clone())
            .or_else(|| {
                let tail = outcome.stderr_tail.trim();
                (!tail.is_empty()).then(|| tail.to_string())
            })
            .unwrap_or_else(|| "Kimi Code exited unsuccessfully".to_string());
        AdapterEvent::new(
            RelayEventType::WorkerFailed,
            serde_json::json!({
                "message": message,
                "exitCode": outcome.exit_code,
                "signal": outcome.signal,
            }),
        )
    }
}

pub struct KimiAdapter {
    configured_executable: Option<String>,
    prefix_args: Vec<String>,
    environment: Vec<(String, String)>,
    supervisor: Arc<ProcessSupervisor>,
    options: Mutex<Option<RuntimeOptions>>,
}

impl Default for KimiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl KimiAdapter {
    pub fn new() -> Self {
        Self {
            configured_executable: None,
            prefix_args: Vec::new(),
            environment: Vec::new(),
            supervisor: Arc::new(ProcessSupervisor::new()),
            options: Mutex::new(None),
        }
    }

    pub fn with_executable(executable: impl Into<String>) -> Self {
        Self {
            configured_executable: Some(executable.into()),
            ..Self::new()
        }
    }

    pub fn with_supervisor(mut self, supervisor: Arc<ProcessSupervisor>) -> Self {
        self.supervisor = supervisor;
        self
    }

    fn executable(&self) -> Option<String> {
        if let Some(configured) = &self.configured_executable {
            return Some(configured.clone());
        }
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let extra: Vec<std::path::PathBuf> = home
            .map(|home| {
                vec![
                    home.join(".local/bin/kimi"),
                    home.join(".kimi-code/bin/kimi"),
                ]
            })
            .unwrap_or_default();
        discover_executable("kimi", &extra).map(|path| path.to_string_lossy().to_string())
    }
}

#[async_trait]
impl AgentAdapter for KimiAdapter {
    fn id(&self) -> &str {
        ADAPTER_ID
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            non_interactive: true,
            structured_events: true,
            cwd: true,
            resume: false,
            send: false,
            cancel: true,
            child_sessions: false,
            model_selection: None,
        }
    }

    async fn detect(&self) -> DetectionResult {
        let Some(executable) = self.executable() else {
            return DetectionResult {
                runtimes: Vec::new(),
                diagnostics: vec!["Kimi Code executable `kimi` was not found".to_string()],
            };
        };
        let evidence = read_help(&executable, &self.prefix_args).await;
        let (capabilities, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options);
        let version = version_of(&executable, &self.prefix_args).await;
        DetectionResult {
            runtimes: vec![Runtime {
                id: RUNTIME_ID.to_string(),
                adapter_id: ADAPTER_ID.to_string(),
                executable_path: executable,
                version: version.clone(),
                health: RuntimeHealth::Available,
                capabilities,
            }],
            diagnostics: if version.is_some() {
                vec!["Authentication is validated by Kimi Code when a run starts".to_string()]
            } else {
                vec!["Kimi Code version check failed".to_string()]
            },
        }
    }

    async fn report_options(&self, runtime_id: &str) -> RuntimeOptions {
        let Some(executable) = self.executable() else {
            return RuntimeOptions::empty(
                runtime_id,
                ADAPTER_ID,
                "Runtime executable was not found, so Relay cannot read its model options",
            );
        };
        let evidence = read_help(&executable, &self.prefix_args).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, runtime_id, ADAPTER_ID);
        options
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        let executable = input
            .executable_path
            .clone()
            .or_else(|| self.executable())
            .ok_or_else(|| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    "Kimi Code executable `kimi` was not found",
                )
            })?;
        let evidence = read_help(&executable, &self.prefix_args).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options.clone());

        let base = [
            self.prefix_args.clone(),
            vec![
                "--prompt".to_string(),
                input.task.clone(),
                "--output-format".to_string(),
                "stream-json".to_string(),
            ],
        ]
        .concat();
        let selection = Selection {
            model: input.model.clone(),
            reasoning: input.reasoning.clone(),
        };
        let args = with_selection_args(&base, &selection, &options);

        run_cli(
            StreamSpec {
                executable,
                args,
                cwd: input.cwd.clone(),
                env: self.environment.clone(),
                stdin: None,
                supervisor_key: input.worker_session_id.clone(),
            },
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            false,
        )
        .await
    }

    async fn cancel(&self, native_session_id: &str) -> Result<()> {
        self.supervisor.terminate(native_session_id);
        Ok(())
    }

    async fn dispose(&self) {
        self.supervisor.terminate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_text_and_tool_calls_become_events() {
        let parsed = parse_line(
            r#"{"role":"assistant","content":"looking","tool_calls":[{"id":"c1","function":{"name":"read_file","arguments":"{}"}}]}"#,
        );
        assert_eq!(parsed.events.len(), 2);
        assert_eq!(parsed.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(parsed.events[1].event_type, RelayEventType::ToolRead);
        assert_eq!(parsed.final_text.as_deref(), Some("looking"));
    }

    #[test]
    fn meta_carries_the_native_session_id() {
        let parsed = parse_line(r#"{"role":"meta","session_id":"kimi-1"}"#);
        assert_eq!(parsed.session_id.as_deref(), Some("kimi-1"));
        assert!(parsed.events.is_empty());
    }

    #[test]
    fn tool_results_and_malformed_lines_are_kept() {
        let result = parse_line(r#"{"role":"tool","tool_call_id":"c1","content":[{"text":"ok"}]}"#);
        assert_eq!(result.events[0].event_type, RelayEventType::ToolResult);
        assert_eq!(result.events[0].data["output"], "ok");

        let malformed = parse_line("nope");
        assert_eq!(malformed.events[0].data["kind"], "diagnostic");
    }

    #[test]
    fn completion_uses_the_last_assistant_text() {
        let event = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some("done".to_string()),
            ..StreamOutcome::default()
        });
        assert_eq!(event.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(event.data["summary"], "done");
    }
}
