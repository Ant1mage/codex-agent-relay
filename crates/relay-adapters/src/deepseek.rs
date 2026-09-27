//! DeepSeek Harness (`dsh`).
//!
//! The JSON stream is the supported path. Its semantics are Relay's contract
//! with this CLI and are deliberately unchanged:
//!
//! ```text
//! session      → native session id
//! thinking     → worker/reasoning
//! text         → worker/message
//! status       → worker/message (status) + turn_end kind
//! tool_call    → tool/read · tool/search · tool/edit · tool/command
//! tool_result  → tool/result
//! final        → worker/message (final) + the summary of worker/completed
//! error        → worker/message (diagnostic) + the failure message
//! ```
//!
//! Core never sees any of this: only the mapped Relay events leave this module.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, DetectionResult, RelayError, RelayEventType,
    Result, ResumeInput, Runtime, RuntimeHealth, RuntimeOptions, StartInput, WorkerHandle,
};

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::probe::{
    discover_executable, probe_runtime_options, read_help, version_of, with_selection_args,
    Selection,
};

pub const ADAPTER_ID: &str = "deepseek-harness";
pub const RUNTIME_ID: &str = "runtime:deepseek-harness";

/// Maps one native line into Relay events. Public so it can be tested directly.
pub fn parse_line(line: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "diagnostic", "message": "DeepSeek Harness emitted malformed JSON" }),
                serde_json::Value::String(line.chars().take(8_192).collect()),
            ));
            return parsed;
        }
    };
    let Some(object) = value.as_object() else {
        parsed.events.push(AdapterEvent::with_native(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "diagnostic", "message": "DeepSeek Harness emitted an invalid event" }),
            value,
        ));
        return parsed;
    };
    let event_type = object
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    match event_type {
        "session" => {
            parsed.session_id = object
                .get("sessionId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
        }
        "thinking" => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerReasoning,
                serde_json::json!({ "text": object.get("text").cloned().unwrap_or(serde_json::Value::String(String::new())) }),
                value,
            ));
        }
        "text" => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "text": object.get("text").cloned().unwrap_or(serde_json::Value::String(String::new())) }),
                value,
            ));
        }
        "status" => {
            let reason_kind =
                if object.get("phase").and_then(|value| value.as_str()) == Some("turn_end") {
                    object
                        .get("reason")
                        .and_then(|reason| reason.get("kind"))
                        .and_then(|kind| kind.as_str())
                        .map(str::to_string)
                } else {
                    None
                };
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({
                    "kind": "status",
                    "phase": object.get("phase").cloned().unwrap_or(serde_json::Value::Null),
                    "turn": object.get("turn").cloned().unwrap_or(serde_json::Value::Null),
                    "step": object.get("step").cloned().unwrap_or(serde_json::Value::Null),
                    "reason": object.get("reason").cloned().unwrap_or(serde_json::Value::Null),
                    "usage": object.get("usage").cloned().unwrap_or(serde_json::Value::Null),
                }),
                value,
            ));
            parsed.turn_end_kind = reason_kind;
        }
        "tool_call" => {
            let tool = object
                .get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            parsed.events.push(AdapterEvent::with_native(
                tool_event_type(&tool),
                serde_json::json!({
                    "callId": object.get("callId").cloned().unwrap_or(serde_json::Value::Null),
                    "tool": tool,
                    "input": object.get("input").cloned().unwrap_or(serde_json::Value::Null),
                }),
                value,
            ));
        }
        "tool_result" => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::ToolResult,
                serde_json::json!({
                    "callId": object.get("callId").cloned().unwrap_or(serde_json::Value::Null),
                    "status": object.get("status").cloned().unwrap_or(serde_json::Value::Null),
                    "result": object.get("result").cloned().unwrap_or(serde_json::Value::Null),
                }),
                value,
            ));
        }
        "final" => {
            let text = object
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "final", "text": text }),
                value,
            ));
            parsed.final_text = Some(text);
        }
        "error" => {
            let message = object
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("DeepSeek Harness failed")
                .to_string();
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "diagnostic", "level": "error", "message": message }),
                value,
            ));
            parsed.error_message = Some(message);
        }
        _ => {}
    }
    parsed
}

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
    } else {
        RelayEventType::ToolCommand
    }
}

fn terminal_event(outcome: &StreamOutcome) -> AdapterEvent {
    let completed = outcome.exit_code == Some(0)
        && outcome.error_message.is_none()
        && outcome.spawn_error.is_none()
        && outcome
            .turn_end_kind
            .as_deref()
            .map(|kind| kind == "completed")
            .unwrap_or(true);
    if completed {
        AdapterEvent::new(
            RelayEventType::WorkerCompleted,
            serde_json::json!({
                "summary": outcome.final_text.clone().unwrap_or_default(),
                "exitCode": outcome.exit_code,
            }),
        )
    } else {
        let message = outcome
            .error_message
            .clone()
            .or_else(|| {
                let tail = outcome.stderr_tail.trim();
                (!tail.is_empty()).then(|| tail.to_string())
            })
            .unwrap_or_else(|| "DeepSeek Harness exited unsuccessfully".to_string());
        AdapterEvent::new(
            RelayEventType::WorkerFailed,
            serde_json::json!({
                "message": message,
                "exitCode": outcome.exit_code,
                "signal": outcome.signal,
                "turnEndKind": outcome.turn_end_kind,
            }),
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct Features {
    json: bool,
    resume: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            json: true,
            resume: true,
        }
    }
}

pub struct DeepSeekAdapter {
    configured_executable: Option<String>,
    prefix_args: Vec<String>,
    environment: Vec<(String, String)>,
    supervisor: Arc<ProcessSupervisor>,
    features: Mutex<Features>,
    options: Mutex<Option<RuntimeOptions>>,
}

impl Default for DeepSeekAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self {
            configured_executable: None,
            prefix_args: Vec::new(),
            environment: Vec::new(),
            supervisor: Arc::new(ProcessSupervisor::new()),
            features: Mutex::new(Features::default()),
            options: Mutex::new(None),
        }
    }

    pub fn with_executable(executable: impl Into<String>) -> Self {
        Self {
            configured_executable: Some(executable.into()),
            ..Self::new()
        }
    }

    pub fn with_prefix_args(mut self, args: Vec<String>) -> Self {
        self.prefix_args = args;
        self
    }

    pub fn with_environment(mut self, env: Vec<(String, String)>) -> Self {
        self.environment = env;
        self
    }

    pub fn with_supervisor(mut self, supervisor: Arc<ProcessSupervisor>) -> Self {
        self.supervisor = supervisor;
        self
    }

    /// `dsh` may be installed through npx rather than on `PATH`.
    fn executable(&self) -> Option<String> {
        if let Some(configured) = &self.configured_executable {
            return Some(configured.clone());
        }
        let mut extra = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            let npx_root = PathBuf::from(home).join(".npm").join("_npx");
            if let Ok(entries) = std::fs::read_dir(&npx_root) {
                let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = entries
                    .flatten()
                    .map(|entry| entry.path().join("node_modules").join(".bin").join("dsh"))
                    .filter(|path| path.is_file())
                    .filter_map(|path| {
                        let modified = path.metadata().ok()?.modified().ok()?;
                        Some((modified, path))
                    })
                    .collect();
                candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
                extra.extend(candidates.into_iter().map(|(_, path)| path));
            }
        }
        discover_executable("dsh", &extra).map(|path| path.to_string_lossy().to_string())
    }

    /// Probes `--help` once so model/reasoning flags are only ever sent when the
    /// CLI advertises them.
    async fn refresh_options(&self, executable: &str) -> RuntimeOptions {
        let mut prefix = self.prefix_args.clone();
        prefix.push("--profile".to_string());
        prefix.push("headless".to_string());
        let evidence = read_help(executable, &prefix).await;
        self.set_features(&evidence.text);
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options.clone());
        options
    }

    fn set_features(&self, help: &str) {
        *self.features.lock().unwrap() = Features {
            json: help.contains("--json"),
            resume: help.contains("--session-id"),
        };
    }

    fn features(&self) -> Features {
        *self.features.lock().unwrap()
    }

    fn selection(&self, input: &StartInput) -> Selection {
        Selection {
            model: input.model.clone(),
            reasoning: input.reasoning.clone(),
        }
    }

    async fn launch(
        &self,
        input: StartInput,
        resume_session_id: Option<String>,
    ) -> Result<WorkerHandle> {
        let executable = input
            .executable_path
            .clone()
            .or_else(|| self.executable())
            .ok_or_else(|| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    "DeepSeek Harness executable `dsh` was not found",
                )
            })?;
        let options = self.refresh_options(&executable).await;
        let features = self.features();

        if !features.json {
            return self
                .launch_plain(&executable, input, resume_session_id)
                .await;
        }

        let mut args = self.prefix_args.clone();
        args.extend([
            "--profile".to_string(),
            "headless".to_string(),
            "--json".to_string(),
        ]);
        if let Some(session_id) = &resume_session_id {
            args.push("--session-id".to_string());
            args.push(session_id.clone());
        }
        let args = with_selection_args(&args, &self.selection(&input), &options);

        run_cli(
            StreamSpec {
                executable,
                args,
                cwd: input.cwd.clone(),
                env: self.environment.clone(),
                stdin: Some(input.task.clone()),
                supervisor_key: input.worker_session_id.clone(),
            },
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            true,
        )
        .await
    }

    async fn launch_plain(
        &self,
        executable: &str,
        input: StartInput,
        resume_session_id: Option<String>,
    ) -> Result<WorkerHandle> {
        if resume_session_id.is_some() && !self.features().resume {
            return Err(RelayError::new(
                "OPERATION_UNSUPPORTED",
                "This DeepSeek Harness version does not support --session-id",
            ));
        }
        let options = self
            .options
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| RuntimeOptions::empty(RUNTIME_ID, ADAPTER_ID, "plain mode"));
        let mut args = self.prefix_args.clone();
        args.extend(["--profile".to_string(), "headless".to_string()]);
        if let Some(session_id) = &resume_session_id {
            args.push("--session-id".to_string());
            args.push(session_id.clone());
        }
        args.push(input.task.clone());
        let args = with_selection_args(&args, &self.selection(&input), &options);

        run_cli(
            StreamSpec {
                executable: executable.to_string(),
                args,
                cwd: input.cwd.clone(),
                env: self.environment.clone(),
                stdin: None,
                supervisor_key: input.worker_session_id.clone(),
            },
            StreamMode::WholeOutput,
            Arc::new(parse_plain_output),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            false,
        )
        .await
    }
}

/// Plain-text fallback: everything the CLI printed becomes the final message.
fn parse_plain_output(output: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let text = output.trim();
    if !text.is_empty() {
        parsed.final_text = Some(text.to_string());
        parsed.events.push(AdapterEvent::new(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "final", "text": text }),
        ));
    }
    parsed
}

#[async_trait]
impl AgentAdapter for DeepSeekAdapter {
    fn id(&self) -> &str {
        ADAPTER_ID
    }

    fn capabilities(&self) -> AdapterCapabilities {
        let features = self.features();
        AdapterCapabilities {
            non_interactive: true,
            structured_events: features.json,
            cwd: true,
            resume: features.resume,
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
                diagnostics: vec!["DeepSeek Harness executable `dsh` was not found".to_string()],
            };
        };
        let mut prefix = self.prefix_args.clone();
        prefix.push("--profile".to_string());
        prefix.push("headless".to_string());
        let evidence = read_help(&executable, &prefix).await;
        self.set_features(&evidence.text);
        let version = version_of(&executable, &self.prefix_args).await;
        let capabilities = self.capabilities();

        let mut diagnostics = Vec::new();
        if version.is_none() {
            diagnostics.push("DeepSeek Harness version check failed".to_string());
        } else {
            diagnostics.push(
                "Authentication is validated by DeepSeek Harness when a run starts".to_string(),
            );
            if !capabilities.structured_events {
                diagnostics.push(
                    "This dsh version has no --json stream; Relay will use bounded plain-text mode"
                        .to_string(),
                );
            }
        }

        DetectionResult {
            runtimes: vec![Runtime {
                id: RUNTIME_ID.to_string(),
                adapter_id: ADAPTER_ID.to_string(),
                executable_path: executable,
                version,
                health: RuntimeHealth::Available,
                capabilities,
            }],
            diagnostics,
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
        let options = self.refresh_options(&executable).await;
        let mut options = options;
        options.runtime_id = runtime_id.to_string();
        options
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        self.launch(input, None).await
    }

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        let session_id = input.native_session_id.clone();
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
            Some(session_id),
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
    fn the_documented_stream_maps_to_stable_relay_events() {
        let session = parse_line(r#"{"type":"session","sessionId":"session-1","cwd":"/tmp"}"#);
        assert_eq!(session.session_id.as_deref(), Some("session-1"));
        assert!(session.events.is_empty());

        let tool = parse_line(
            r#"{"type":"tool_call","callId":"c1","tool":"str_replace_editor","input":{"path":"a.ts"}}"#,
        );
        assert_eq!(tool.events[0].event_type, RelayEventType::ToolEdit);
        assert_eq!(tool.events[0].data["callId"], "c1");

        let status = parse_line(
            r#"{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"max-tokens"}}"#,
        );
        assert_eq!(status.turn_end_kind.as_deref(), Some("max-tokens"));
        assert_eq!(status.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(status.events[0].data["kind"], "status");
    }

    #[test]
    fn thinking_text_and_final_keep_their_meaning() {
        let thinking = parse_line(r#"{"type":"thinking","text":"consider"}"#);
        assert_eq!(
            thinking.events[0].event_type,
            RelayEventType::WorkerReasoning
        );

        let text = parse_line(r#"{"type":"text","text":"hello"}"#);
        assert_eq!(text.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(text.events[0].data["text"], "hello");

        let final_event = parse_line(r#"{"type":"final","text":"done"}"#);
        assert_eq!(final_event.final_text.as_deref(), Some("done"));
        assert_eq!(final_event.events[0].data["kind"], "final");
    }

    #[test]
    fn tool_results_and_errors_are_kept() {
        let result = parse_line(
            r#"{"type":"tool_result","callId":"c1","status":"completed","result":"ok"}"#,
        );
        assert_eq!(result.events[0].event_type, RelayEventType::ToolResult);
        assert_eq!(result.events[0].data["status"], "completed");

        let error = parse_line(r#"{"type":"error","message":"boom"}"#);
        assert_eq!(error.error_message.as_deref(), Some("boom"));
        assert_eq!(error.events[0].data["kind"], "diagnostic");
    }

    #[test]
    fn malformed_output_becomes_a_bounded_diagnostic() {
        let parsed = parse_line("not-json");
        assert_eq!(parsed.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(parsed.events[0].data["kind"], "diagnostic");
        assert!(parsed.events[0].native_event.is_some());
    }

    #[test]
    fn tool_names_choose_the_event_kind() {
        assert_eq!(tool_event_type("read_file"), RelayEventType::ToolRead);
        assert_eq!(tool_event_type("grep_search"), RelayEventType::ToolSearch);
        assert_eq!(tool_event_type("apply_patch"), RelayEventType::ToolEdit);
        assert_eq!(tool_event_type("run_shell"), RelayEventType::ToolCommand);
    }

    #[test]
    fn completion_carries_the_final_text_as_the_summary() {
        let outcome = StreamOutcome {
            exit_code: Some(0),
            final_text: Some("work complete".to_string()),
            turn_end_kind: Some("completed".to_string()),
            ..StreamOutcome::default()
        };
        let event = terminal_event(&outcome);
        assert_eq!(event.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(event.data["summary"], "work complete");
    }

    #[test]
    fn a_non_zero_exit_or_turn_end_failure_reports_failure() {
        let failed_exit = terminal_event(&StreamOutcome {
            exit_code: Some(1),
            stderr_tail: "boom\n".to_string(),
            ..StreamOutcome::default()
        });
        assert_eq!(failed_exit.event_type, RelayEventType::WorkerFailed);
        assert_eq!(failed_exit.data["message"], "boom");

        let failed_turn = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            turn_end_kind: Some("max-tokens".to_string()),
            ..StreamOutcome::default()
        });
        assert_eq!(failed_turn.event_type, RelayEventType::WorkerFailed);
        assert_eq!(failed_turn.data["turnEndKind"], "max-tokens");
    }
}
