//! Antigravity CLI (`agy`).
//!
//! Supersedes the retired Gemini CLI adapter. Its stream reports conversation
//! ids, per-step updates (including child agents) and a final result with an
//! explicit status, which is what decides completion.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, DetectionResult, EnforcementSet, RelayError,
    RelayEventType, Result, ResumeInput, Runtime, RuntimeHealth, RuntimeOptions, StartInput,
    WorkerHandle,
};

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::probe::{
    discover_executable, probe_runtime_options, probe_target, read_help, version_of,
    with_selection_args, Selection,
};

pub const ADAPTER_ID: &str = "antigravity-cli";
pub const RUNTIME_ID: &str = "runtime:antigravity-cli";

fn tool_event_type(tool: &str) -> RelayEventType {
    let name = tool.to_lowercase();
    if ["search", "grep", "glob", "find"]
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

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

pub fn parse_line(line: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(line) else {
        parsed.events.push(AdapterEvent::with_native(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "diagnostic", "message": "Antigravity emitted malformed JSON" }),
            serde_json::Value::String(line.chars().take(8_192).collect()),
        ));
        return parsed;
    };
    let Some(event) = raw.as_object() else {
        return parsed;
    };

    match event.get("event").and_then(|value| value.as_str()) {
        Some("init") => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "status", "phase": "initialized" }),
                raw.clone(),
            ));
            parsed.session_id = string_field(&raw, "conversation_id");
        }
        Some("step_update") => {
            let Some(step) = event.get("step_update").and_then(|value| value.as_object()) else {
                return parsed;
            };
            let step = serde_json::Value::Object(step.clone());
            parsed.session_id = string_field(&step, "conversation_id");
            let state = string_field(&step, "state");

            if let Some(subagent) = step.get("subagent_info") {
                parsed.events.push(AdapterEvent::with_native(
                    if state.as_deref() == Some("DONE") {
                        RelayEventType::ChildCompleted
                    } else {
                        RelayEventType::ChildStarted
                    },
                    subagent.clone(),
                    raw,
                ));
                return parsed;
            }

            if string_field(&step, "step_type").as_deref() == Some("agent_response") {
                if let Some(text) = string_field(&step, "text_delta") {
                    parsed.events.push(AdapterEvent::with_native(
                        RelayEventType::WorkerMessage,
                        serde_json::json!({ "kind": "delta", "text": text, "state": state }),
                        raw,
                    ));
                }
                return parsed;
            }

            if string_field(&step, "step_type").as_deref() == Some("tool") {
                let tool =
                    string_field(&step, "tool_name").unwrap_or_else(|| "unknown".to_string());
                let info = step
                    .get("tool_info")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                parsed.events.push(AdapterEvent::with_native(
                    tool_event_type(&tool),
                    serde_json::json!({
                        "tool": tool,
                        "state": state,
                        "parameters": info.get("parameters").cloned().unwrap_or(serde_json::Value::Null),
                    }),
                    raw.clone(),
                ));
                if state.as_deref() == Some("DONE") {
                    parsed.events.push(AdapterEvent::with_native(
                        RelayEventType::ToolResult,
                        serde_json::json!({
                            "tool": tool,
                            "output": info.get("output").cloned().unwrap_or(serde_json::Value::Null),
                            "error": info.get("error").cloned().unwrap_or(serde_json::Value::Null),
                        }),
                        raw,
                    ));
                }
                return parsed;
            }

            let mut data = step.clone();
            if let Some(object) = data.as_object_mut() {
                object.insert(
                    "kind".to_string(),
                    serde_json::Value::String("status".to_string()),
                );
            }
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                data,
                raw,
            ));
        }
        Some("result") => {
            let Some(result) = event.get("result") else {
                return parsed;
            };
            let text = string_field(result, "response").unwrap_or_default();
            if !text.is_empty() {
                parsed.events.push(AdapterEvent::with_native(
                    RelayEventType::WorkerMessage,
                    serde_json::json!({ "kind": "final", "text": text }),
                    raw.clone(),
                ));
            }
            parsed.final_text = Some(text);
            parsed.session_id = string_field(result, "conversation_id");
            parsed.result_status = string_field(result, "status");
            parsed.error_message = string_field(result, "error");
        }
        _ => {}
    }
    parsed
}

fn terminal_event(outcome: &StreamOutcome) -> AdapterEvent {
    match outcome.result_status.as_deref() {
        Some("CANCELED") | Some("INTERRUPTED") => AdapterEvent::new(
            RelayEventType::WorkerCancelled,
            serde_json::json!({ "status": outcome.result_status, "signal": outcome.signal }),
        ),
        Some("SUCCESS") if outcome.exit_code == Some(0) => AdapterEvent::new(
            RelayEventType::WorkerCompleted,
            serde_json::json!({
                "summary": outcome.final_text.clone().unwrap_or_default(),
                "exitCode": outcome.exit_code,
            }),
        ),
        _ => {
            let message = outcome
                .error_message
                .clone()
                .or_else(|| {
                    let tail = outcome.stderr_tail.trim();
                    (!tail.is_empty()).then(|| tail.to_string())
                })
                .unwrap_or_else(|| "Antigravity CLI exited unsuccessfully".to_string());
            AdapterEvent::new(
                RelayEventType::WorkerFailed,
                serde_json::json!({
                    "message": message,
                    "status": outcome.result_status,
                    "exitCode": outcome.exit_code,
                    "signal": outcome.signal,
                }),
            )
        }
    }
}

pub struct AntigravityAdapter {
    configured_executable: Option<String>,
    prefix_args: Vec<String>,
    environment: Vec<(String, String)>,
    supervisor: Arc<ProcessSupervisor>,
    options: Mutex<Option<RuntimeOptions>>,
}

impl Default for AntigravityAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AntigravityAdapter {
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
        let extra: Vec<std::path::PathBuf> = std::env::var_os("HOME")
            .map(|home| vec![std::path::PathBuf::from(home).join(".local/bin/agy")])
            .unwrap_or_default();
        discover_executable("agy", &extra).map(|path| path.to_string_lossy().to_string())
    }

    async fn launch(
        &self,
        input: StartInput,
        conversation_id: Option<String>,
    ) -> Result<WorkerHandle> {
        let executable = input
            .executable_path
            .clone()
            .or_else(|| self.executable())
            .ok_or_else(|| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    "Antigravity CLI executable `agy` was not found",
                )
            })?;
        let evidence = read_help(&executable, &self.prefix_args).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options.clone());

        let mut base = [
            self.prefix_args.clone(),
            vec![
                "-p".to_string(),
                crate::instructions::enveloped(&input.task, input.instructions.as_deref()),
                "--output-format".to_string(),
                "stream-json".to_string(),
            ],
        ]
        .concat();
        if let Some(conversation_id) = &conversation_id {
            base.push("--conversation".to_string());
            base.push(conversation_id.clone());
        }
        let selection = Selection {
            model: input.model.clone(),
            reasoning: input.reasoning.clone(),
        };
        let args = with_selection_args(&base, &selection, &options);

        let require_session = conversation_id.is_none();
        run_cli(
            StreamSpec {
                executable,
                args,
                cwd: input.cwd.clone(),
                env: self.environment.clone(),
                stdin: None,
                supervisor_key: input.worker_session_id.clone(),
                cleanup: None,
            },
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            require_session,
        )
        .await
    }
}

#[async_trait]
impl AgentAdapter for AntigravityAdapter {
    fn id(&self) -> &str {
        ADAPTER_ID
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            non_interactive: true,
            structured_events: true,
            cwd: true,
            resume: true,
            send: false,
            cancel: true,
            child_sessions: true,
            model_selection: None,
            enforcement: EnforcementSet::default(),
        }
    }

    async fn detect(&self) -> DetectionResult {
        let Some(executable) = self.executable() else {
            return DetectionResult {
                runtimes: Vec::new(),
                diagnostics: vec!["Antigravity CLI executable `agy` was not found".to_string()],
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
                vec!["Authentication is validated by Antigravity CLI when a run starts".to_string()]
            } else {
                vec!["Antigravity CLI version check failed".to_string()]
            },
        }
    }

    /// Reads the runtime's own options from exactly the executable it is
    /// registered with, never from whatever discovery would find first.
    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions {
        let (executable, diagnostics) = probe_target(runtime, || self.executable());
        let Some(executable) = executable else {
            return RuntimeOptions::empty(&runtime.id, ADAPTER_ID, diagnostics.join("; "));
        };
        let evidence = read_help(&executable, &self.prefix_args).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, &runtime.id, ADAPTER_ID);
        let mut options = options;
        options.diagnostics.splice(0..0, diagnostics);
        options
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        self.launch(input, None).await
    }

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        let conversation_id = input.native_session_id.clone();
        self.launch(
            StartInput {
                run_id: input.run_id,
                worker_session_id: input.worker_session_id,
                task: input.task,
                cwd: input.cwd,
                access_mode: input.access_mode,
                executable_path: input.executable_path,
                model: None,
                reasoning: None,
                instructions: input.instructions,
            },
            Some(conversation_id),
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
    fn init_reports_the_conversation_id() {
        let parsed = parse_line(r#"{"event":"init","conversation_id":"conv-1"}"#);
        assert_eq!(parsed.session_id.as_deref(), Some("conv-1"));
        assert_eq!(parsed.events[0].data["phase"], "initialized");
    }

    #[test]
    fn a_tool_step_produces_a_tool_event_and_its_result() {
        let running = parse_line(
            r#"{"event":"step_update","step_update":{"conversation_id":"conv-1","step_type":"tool","tool_name":"read_file","state":"RUNNING","tool_info":{"parameters":{}}}}"#,
        );
        assert_eq!(running.events.len(), 1);
        assert_eq!(running.events[0].event_type, RelayEventType::ToolRead);

        let done = parse_line(
            r#"{"event":"step_update","step_update":{"step_type":"tool","tool_name":"read_file","state":"DONE","tool_info":{"output":"ok"}}}"#,
        );
        assert_eq!(done.events.len(), 2);
        assert_eq!(done.events[1].event_type, RelayEventType::ToolResult);
    }

    #[test]
    fn child_agents_stay_child_events() {
        let started = parse_line(
            r#"{"event":"step_update","step_update":{"subagent_info":{"id":"sub-1"},"state":"RUNNING"}}"#,
        );
        assert_eq!(started.events[0].event_type, RelayEventType::ChildStarted);
        let done = parse_line(
            r#"{"event":"step_update","step_update":{"subagent_info":{"id":"sub-1"},"state":"DONE"}}"#,
        );
        assert_eq!(done.events[0].event_type, RelayEventType::ChildCompleted);
    }

    #[test]
    fn the_result_event_decides_completion() {
        let success = parse_line(
            r#"{"event":"result","result":{"conversation_id":"c","response":"done","status":"SUCCESS"}}"#,
        );
        assert_eq!(success.final_text.as_deref(), Some("done"));
        assert_eq!(success.result_status.as_deref(), Some("SUCCESS"));
        let event = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: success.final_text.clone(),
            result_status: success.result_status.clone(),
            ..StreamOutcome::default()
        });
        assert_eq!(event.event_type, RelayEventType::WorkerCompleted);

        let cancelled = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            result_status: Some("CANCELED".to_string()),
            ..StreamOutcome::default()
        });
        assert_eq!(cancelled.event_type, RelayEventType::WorkerCancelled);

        let failed = terminal_event(&StreamOutcome {
            exit_code: Some(1),
            error_message: Some("nope".to_string()),
            result_status: Some("ERROR".to_string()),
            ..StreamOutcome::default()
        });
        assert_eq!(failed.event_type, RelayEventType::WorkerFailed);
        assert_eq!(failed.data["message"], "nope");
    }
}
