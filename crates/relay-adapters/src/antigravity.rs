//! Antigravity CLI (`agy`).
//!
//! The real headless interface is NDJSON on both directions: the task is written
//! to a private stdin as one `user` event, and stdout reports one `init`, a run
//! of `step_update` frames and a terminal `result`. It supersedes the retired
//! Gemini CLI adapter.
//!
//! Only what the CLI actually prints becomes an event. Text deltas stream as
//! assistant-text increments in Relay's unified worker-text contract
//! ([`relay_core::worker_text`]) — one message per `agent_response` step — and
//! the `result` frame stays the authoritative answer. A worker is successful
//! only when that frame says `SUCCESS`, the process exited zero and nothing
//! failed along the way.

mod catalog;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, AssistantTextDelta, DetectionResult,
    EnforcementSet, OptionsSource, RelayError, RelayEventType, Result, ResumeInput, Runtime,
    RuntimeHealth, RuntimeOptions, StartInput, WorkerHandle,
};
use serde_json::{json, Map, Value};

use crate::cli::{
    run_cli, ParsedOutput, ProcessSupervisor, StdinGate, StreamMode, StreamOutcome, StreamSpec,
    ToolTerminalState,
};
use crate::probe::{
    capture_with, discover_executable, probe_runtime_options, probe_target, read_help_with,
    with_selection_args, Selection,
};

pub const ADAPTER_ID: &str = "antigravity-cli";
pub const RUNTIME_ID: &str = "runtime:antigravity-cli";

/// Child-only switch that keeps the CLI from updating itself underneath a run.
const AUTO_UPDATE_ENV: (&str, &str) = ("AGY_CLI_DISABLE_AUTO_UPDATE", "1");

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

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn first_value(step: &Value, paths: &[&str]) -> Value {
    for path in paths {
        if let Some(value) = step.pointer(path).filter(|value| !value.is_null()) {
            return value.clone();
        }
    }
    Value::Null
}

/// The tool's own name, from the shapes the step frames use.
fn tool_name(step: &Value) -> String {
    string_field(step, "tool_name")
        .or_else(|| string_field(step, "tool"))
        .or_else(|| {
            step.pointer("/tool_info/name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Flattens the handful of argument keys the console's summary and Changes view
/// read (`path`, `command`, …) next to the full parameter object.
///
/// The real CLI's file tools publish the path as `AbsolutePath` or `TargetFile`
/// (the official headless guide's `write_to_file` sample uses `TargetFile`); the
/// canonical key the console looks for is `path`, so both are mapped there. The
/// command surface publishes `CommandLine` (the guide's `run_command` sample is
/// `{"CommandLine":"echo hello_headless_demo"}`), mapped to the console's
/// `command`. The lower-case spellings and `query` are the same public argument
/// names the other tools use. Nothing beyond what the CLI actually printed is
/// invented, and the untouched `parameters` object still travels on the event.
fn tool_arguments(parameters: &Value) -> Map<String, Value> {
    const MAPPINGS: [(&str, &[&str]); 3] = [
        (
            "path",
            &[
                "path",
                "file",
                "file_path",
                "target_file",
                "AbsolutePath",
                "TargetFile",
            ],
        ),
        ("command", &["command", "CommandLine"]),
        ("query", &["query"]),
    ];
    let mut data = Map::new();
    for (canonical, candidates) in MAPPINGS {
        for key in candidates {
            if let Some(value) = parameters.get(*key).filter(|value| !value.is_null()) {
                data.insert(canonical.to_string(), value.clone());
                break;
            }
        }
    }
    data
}

/// Reduces a terminal tool step's public error object to the state the terminal
/// decision needs. A `null` error means the call finished cleanly.
fn tool_terminal_state(error: &Value) -> ToolTerminalState {
    if error.is_null() {
        return ToolTerminalState::Ok;
    }
    if error_is_permission_denial(error) {
        ToolTerminalState::PermissionDenied
    } else {
        ToolTerminalState::Error
    }
}

/// True when a public tool error is the CLI's own permission policy denying the
/// call. The message is inspected only to classify it; it never reaches a
/// normalized event or the failure reason, so private output cannot leak.
fn error_is_permission_denial(error: &Value) -> bool {
    let kind = error
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    if kind.contains("permission") || kind.contains("denied") {
        return true;
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or_default()
        .to_lowercase();
    message.contains("permission")
        && (message.contains("denied") || message.contains("check failed"))
}

/// The reason a `SUCCESS` turn that carries no answer and follows a failed tool
/// must not be read as completion. The wording keeps the CLI's own permission
/// policy distinct from a login failure and never quotes the raw tool error.
fn empty_success_tool_failure(state: Option<ToolTerminalState>) -> String {
    match state {
        Some(ToolTerminalState::PermissionDenied) => "Antigravity CLI denied a tool call under its own permission policy and returned an empty response, so the task is not complete; this is a tool permission denial, not a login failure".to_string(),
        _ => "Antigravity CLI reported a tool error and returned an empty response, so the task is not complete".to_string(),
    }
}

/// Only the token counters the CLI publishes. Anything else in the usage object
/// — a provider signature, for instance — never reaches the normalized event.
fn public_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.and_then(Value::as_object) else {
        return Value::Null;
    };
    let mut public = Map::new();
    for key in [
        "input_tokens",
        "output_tokens",
        "thinking_tokens",
        "cache_read_tokens",
        "total_tokens",
    ] {
        if let Some(value) = usage.get(key) {
            public.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(public)
}

/// The result frame reduced to its public fields, for the native payload Relay
/// keeps. A provider signature inside `usage` must not ride along.
fn public_result(result: &Value) -> Value {
    let mut clean = Map::new();
    for key in [
        "conversation_id",
        "status",
        "response",
        "error",
        "error_message",
        "duration_seconds",
        "num_turns",
    ] {
        if let Some(value) = result.get(key).filter(|value| !value.is_null()) {
            clean.insert(key.to_string(), value.clone());
        }
    }
    let usage = public_usage(result.get("usage"));
    if !usage.is_null() {
        clean.insert("usage".to_string(), usage);
    }
    json!({ "event": "result", "result": Value::Object(clean) })
}

/// Incremental parse state: the conversation to bind and the merged answer text.
#[derive(Default)]
struct StreamState {
    /// The conversation a resume asked for; `init` must match it.
    requested_conversation: Option<String>,
    /// The conversation an accepted `init` established. It is only ever set from
    /// a validated init, so a later frame cannot substitute another one.
    conversation_id: Option<String>,
    /// True once a validated `init` has been seen.
    initialized: bool,
    /// True once the stream contradicted the handshake. A rejected stream stays
    /// rejected, so a wrong new id is never persisted as a legitimate resume.
    rejected: bool,
    text: String,
    /// The `step_index` whose `agent_response` text is currently streaming, and
    /// whether any increment of it has arrived. A new step index opens a new
    /// assistant message in the unified worker-text contract.
    text_message_step: Option<u64>,
    text_message_open: bool,
    /// Tool step indexes whose call event has already been emitted, so the
    /// terminal update does not repeat the call.
    reported_tools: std::collections::BTreeSet<u64>,
}

impl StreamState {
    /// Records a fatal contradiction and returns the failure it produces.
    fn reject(&mut self, message: String) -> ParsedOutput {
        self.rejected = true;
        ParsedOutput {
            error_message: Some(message),
            ..ParsedOutput::default()
        }
    }

    /// Checks a frame against the conversation validated at `init`. A frame that
    /// names a different conversation, or arrives before `init`, is fatal.
    fn accept_conversation(&mut self, actual: &str) -> Option<ParsedOutput> {
        if !self.initialized {
            return Some(self.reject(
                "Antigravity reported a conversation before a validated init".to_string(),
            ));
        }
        if self.conversation_id.as_deref() != Some(actual) {
            let expected = self.conversation_id.clone().unwrap_or_default();
            return Some(self.reject(format!(
                "Antigravity switched from conversation {expected} to {actual} mid-stream"
            )));
        }
        None
    }

    fn parse(&mut self, line: &str) -> ParsedOutput {
        let mut parsed = ParsedOutput::default();
        if self.rejected || line.trim().is_empty() {
            return parsed;
        }
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            parsed.error_message = Some("Antigravity emitted malformed JSON".to_string());
            return parsed;
        };
        let Some(event) = raw.as_object() else {
            return parsed;
        };

        match event.get("event").and_then(Value::as_str) {
            Some("init") => {
                let Some(conversation) = string_field(&raw, "conversation_id") else {
                    return self.reject(
                        "Antigravity started without reporting a conversation id".to_string(),
                    );
                };
                if uuid::Uuid::parse_str(&conversation).is_err() {
                    return self.reject(format!(
                        "Antigravity reported a conversation id that is not a UUID: {conversation}"
                    ));
                }
                if let Some(requested) = &self.requested_conversation {
                    // A resume that comes back on a different conversation has
                    // silently started a new one: fail instead of drifting.
                    if requested != &conversation {
                        return self.reject(format!(
                            "Antigravity resumed conversation {conversation} instead of the requested {requested}"
                        ));
                    }
                }
                self.initialized = true;
                self.conversation_id = Some(conversation.clone());
                parsed.session_id = Some(conversation);
                parsed.events.push(AdapterEvent::with_native(
                    RelayEventType::WorkerStatus,
                    json!({ "kind": "status", "phase": "initialized" }),
                    raw.clone(),
                ));
            }
            Some("step_update") => {
                let Some(step) = event.get("step_update").and_then(Value::as_object) else {
                    return parsed;
                };
                let step = Value::Object(step.clone());
                if let Some(conversation) = string_field(&step, "conversation_id") {
                    if let Some(rejection) = self.accept_conversation(&conversation) {
                        return rejection;
                    }
                    parsed.session_id = self.conversation_id.clone();
                }
                let state = string_field(&step, "state");
                let terminal = matches!(
                    state.as_deref(),
                    Some("DONE" | "ERROR" | "FAILED" | "CANCELLED" | "CANCELED")
                );

                if let Some(subagent) = step.get("subagent_info").filter(|value| !value.is_null()) {
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

                match string_field(&step, "step_type").as_deref() {
                    // The answer streams as deltas; Relay emits each increment in
                    // the unified worker-text contract and also preserves the
                    // authoritative final result. One `agent_response` step is
                    // one assistant message.
                    Some("agent_response") => {
                        if let Some(delta) = string_field(&step, "text_delta") {
                            self.text.push_str(&delta);
                            if !delta.is_empty() {
                                let step_index = step.get("step_index").and_then(Value::as_u64);
                                let starts_message =
                                    !self.text_message_open || self.text_message_step != step_index;
                                self.text_message_open = true;
                                self.text_message_step = step_index;
                                parsed
                                    .events
                                    .push(AdapterEvent::text_delta(if starts_message {
                                        AssistantTextDelta::message(delta)
                                    } else {
                                        AssistantTextDelta::chunk(delta)
                                    }));
                            }
                        }
                    }
                    Some("tool") => {
                        let tool = tool_name(&step);
                        let parameters = first_value(
                            &step,
                            &[
                                "/tool_info/parameters",
                                "/parameters",
                                "/tool_info/arguments",
                            ],
                        );
                        let step_index = step.get("step_index").and_then(Value::as_u64);
                        // The call is reported once; the terminal update carries
                        // the result, not a second copy of the call.
                        let first_report = match step_index {
                            Some(index) => self.reported_tools.insert(index),
                            None => !terminal,
                        };
                        if first_report {
                            let mut data = json!({
                                "tool": tool,
                                "state": state,
                                "stepIndex": step.get("step_index"),
                                "parameters": parameters,
                            });
                            if let Some(object) = data.as_object_mut() {
                                for (key, value) in tool_arguments(&parameters) {
                                    object.insert(key, value);
                                }
                            }
                            parsed.events.push(AdapterEvent::with_native(
                                tool_event_type(&tool),
                                data,
                                raw.clone(),
                            ));
                        }
                        if terminal {
                            let output = first_value(
                                &step,
                                &["/tool_info/output", "/tool_info/result", "/output"],
                            );
                            let error = first_value(&step, &["/tool_info/error", "/error"]);
                            // A public error object on the terminal update is the
                            // only signal that the call failed; a later clean tool
                            // terminal supersedes it.
                            parsed.tool_terminal = Some(tool_terminal_state(&error));
                            parsed.events.push(AdapterEvent::with_native(
                                RelayEventType::ToolResult,
                                json!({
                                    "tool": tool,
                                    "state": state,
                                    "stepIndex": step.get("step_index"),
                                    "output": output,
                                    "error": error,
                                }),
                                raw,
                            ));
                        }
                    }
                    // The echo of Relay's own private stdin task carries no new
                    // information; keep the frame without repeating its content.
                    Some("user_input") => {
                        parsed.events.push(AdapterEvent::with_native(
                            RelayEventType::WorkerStatus,
                            json!({ "kind": "status", "phase": "user_input", "state": state }),
                            raw,
                        ));
                    }
                    other => {
                        parsed.events.push(AdapterEvent::with_native(
                            RelayEventType::WorkerStatus,
                            json!({ "kind": "status", "stepType": other, "state": state }),
                            raw,
                        ));
                    }
                }
            }
            Some("result") => {
                let Some(result) = event.get("result") else {
                    return parsed;
                };
                let conversation = string_field(result, "conversation_id");
                if let Some(conversation) = &conversation {
                    if let Some(rejection) = self.accept_conversation(conversation) {
                        return rejection;
                    }
                }
                let text = string_field(result, "response")
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| self.text.clone());
                parsed.final_text = Some(text.clone());
                parsed.result_status = string_field(result, "status");
                parsed.error_message =
                    string_field(result, "error").or_else(|| string_field(result, "error_message"));
                parsed.session_id = conversation.or_else(|| self.conversation_id.clone());
                let native = public_result(result);
                if !text.is_empty() {
                    parsed.events.push(AdapterEvent::with_native(
                        RelayEventType::WorkerMessage,
                        json!({ "kind": "final", "text": text }),
                        native.clone(),
                    ));
                }
                parsed.events.push(AdapterEvent::with_native(
                    RelayEventType::WorkerStatus,
                    json!({
                        "kind": "result",
                        "status": parsed.result_status,
                        "usage": public_usage(result.get("usage")),
                        "durationSeconds": result.get("duration_seconds"),
                        "numTurns": result.get("num_turns"),
                    }),
                    native,
                ));
            }
            _ => {}
        }
        parsed
    }
}

fn terminal_event(outcome: &StreamOutcome) -> AdapterEvent {
    // A user cancel is authoritative in the core controller's cancel_requested.
    // A real SIGTERM makes the CLI wind itself down and print `ERROR` with exit
    // 1 — indistinguishable here from an ordinary failure — so `ERROR` is never
    // treated as a cancellation on its own. An explicit failure (a rejected or
    // missing handshake, a drift) outranks the signal heuristics: Relay's own
    // reclaim of a rejected worker must not read as a cancellation.
    let cancelled = outcome.error_message.is_none()
        && (matches!(
            outcome.result_status.as_deref(),
            Some("CANCELED" | "CANCELLED" | "INTERRUPTED")
        ) || matches!(
            outcome.signal,
            Some(libc::SIGTERM | libc::SIGINT | libc::SIGKILL)
        ) || matches!(outcome.exit_code, Some(130 | 143)));
    // A real `SUCCESS` can still hide an unfinished task: the CLI's last tool
    // step failed (a denied write, for instance) and the answer is empty. That
    // is a failure with its own reason, never a login failure. An actual answer,
    // or a later clean tool, keeps the success.
    let empty_reply = outcome
        .final_text
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty();
    let failed_on_empty_tool = outcome.error_message.is_none()
        && outcome.spawn_error.is_none()
        && outcome.result_status.as_deref() == Some("SUCCESS")
        && outcome.exit_code == Some(0)
        && empty_reply
        && matches!(
            outcome.tool_terminal,
            Some(ToolTerminalState::PermissionDenied | ToolTerminalState::Error)
        );
    let event_type = if cancelled {
        RelayEventType::WorkerCancelled
    } else if outcome.result_status.as_deref() == Some("SUCCESS")
        && outcome.exit_code == Some(0)
        && outcome.error_message.is_none()
        && outcome.spawn_error.is_none()
        && !failed_on_empty_tool
    {
        RelayEventType::WorkerCompleted
    } else {
        RelayEventType::WorkerFailed
    };
    let message = outcome
        .error_message
        .clone()
        .or_else(|| outcome.spawn_error.clone())
        .or_else(|| {
            failed_on_empty_tool.then(|| empty_success_tool_failure(outcome.tool_terminal))
        })
        .or_else(|| {
            let tail = outcome.stderr_tail.trim();
            (!tail.is_empty()).then(|| tail.to_string())
        })
        .unwrap_or_else(|| match outcome.result_status.as_deref() {
            Some(status) => format!("Antigravity CLI stopped with {status}"),
            None => "Antigravity CLI exited without a successful result; check the CLI login and connection"
                .to_string(),
        });
    let mut data = json!({
        "status": outcome.result_status,
        "exitCode": outcome.exit_code,
        "signal": outcome.signal,
    });
    if let Some(object) = data.as_object_mut() {
        match event_type {
            // The console reads `summary` before `message`, so only the event
            // that actually has an answer carries one.
            RelayEventType::WorkerCompleted => {
                object.insert(
                    "summary".to_string(),
                    json!(outcome.final_text.clone().unwrap_or_default()),
                );
            }
            RelayEventType::WorkerFailed => {
                object.insert("message".to_string(), json!(message));
            }
            _ => {}
        }
    }
    AdapterEvent::new(event_type, data)
}

pub struct AntigravityAdapter {
    configured_executable: Option<String>,
    prefix_args: Vec<String>,
    supervisor: Arc<ProcessSupervisor>,
    options: Mutex<Option<RuntimeOptions>>,
    /// How long a launch waits for the CLI to announce the conversation before
    /// the task is abandoned and the child is reclaimed.
    handshake_timeout: Duration,
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
            supervisor: Arc::new(ProcessSupervisor::new()),
            options: Mutex::new(None),
            handshake_timeout: Duration::from_secs(30),
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

    #[cfg(test)]
    fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
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

    async fn environment() -> Vec<(String, String)> {
        crate::environment::child_environment(&[AUTO_UPDATE_ENV]).await
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
        let env = Self::environment().await;
        let evidence = read_help_with(&executable, &self.prefix_args, &env).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options.clone());

        // There is no verified Antigravity headless read-only mode, so Relay
        // refuses to pretend it can enforce one. Authorized Write runs use the
        // CLI's edit-only `accept-edits` mode below.
        if !input.access_mode.is_write() {
            return Err(RelayError::new(
                "ADAPTER_FAILURE",
                "Antigravity headless cannot enforce a read-only workspace; Relay refuses to run this access mode",
            ));
        }
        if !evidence.text.contains("--mode") || !evidence.text.contains("accept-edits") {
            return Err(RelayError::new(
                "ADAPTER_FAILURE",
                "This Antigravity CLI does not advertise `--mode accept-edits`, which Relay needs to honor Write access in headless mode",
            ));
        }
        if input.model.is_some() && options.model_flag.is_none() {
            return Err(RelayError::new(
                "ADAPTER_FAILURE",
                "This Antigravity CLI cannot apply the selected model",
            ));
        }
        if input.reasoning.is_some() && options.reasoning_flag.is_none() {
            return Err(RelayError::new(
                "ADAPTER_FAILURE",
                "This Antigravity CLI cannot apply the selected reasoning strength",
            ));
        }
        if let Some(conversation) = &conversation_id {
            if uuid::Uuid::parse_str(conversation).is_err() {
                return Err(RelayError::new(
                    "ADAPTER_FAILURE",
                    "Antigravity resume requires the native conversation UUID",
                ));
            }
        }

        // The task stays out of the process list: one private NDJSON `user`
        // event on stdin, closed immediately after it is written.
        let payload = serde_json::to_string(&json!({
            "event": "user",
            "message": {
                "content": crate::instructions::enveloped(&input.task, input.instructions.as_deref()),
            }
        }))
        .map_err(|error| {
            RelayError::new("ADAPTER_FAILURE", format!("Could not encode Antigravity input: {error}"))
        })?;
        let stdin = format!("{payload}\n");

        let mut base = vec![
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            // Relay's Write access mode means file changes are authorized for
            // this run. In Antigravity headless mode, the default `request-review`
            // may require an interactive diff confirmation that headless cannot
            // present, leaving a denied edit with SUCCESS and no file. The
            // override approves file-edit confirmations only; shell commands
            // and non-workspace access remain governed by the user's CLI policy.
            "--mode".to_string(),
            "accept-edits".to_string(),
        ];
        if let Some(conversation) = &conversation_id {
            base.push("--conversation".to_string());
            base.push(conversation.clone());
        }
        let args = with_selection_args(
            &base,
            &Selection {
                model: input.model.clone(),
                reasoning: input.reasoning.clone(),
            },
            &options,
        );

        let state = Mutex::new(StreamState {
            requested_conversation: conversation_id.clone(),
            ..StreamState::default()
        });
        let mut handle = run_cli(
            StreamSpec {
                executable,
                args,
                cwd: input.cwd.clone(),
                env,
                stdin: Some(stdin),
                // The task is withheld until the CLI's own `init` names a valid
                // conversation, so a resume id the CLI does not know can never
                // receive a task against the wrong new session.
                stdin_gate: Some(StdinGate {
                    timeout: self.handshake_timeout,
                }),
                // Registered the moment it is spawned, so cancel can find it
                // before the first output line arrives.
                supervisor_key: input.worker_session_id.clone(),
                cleanup: None,
            },
            StreamMode::Lines,
            Arc::new(move |line| state.lock().unwrap().parse(line)),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            conversation_id.is_none(),
        )
        .await?;
        // A resume continues a conversation Relay already knows; report it on the
        // handle so the next round can resume again.
        if handle.native_session_id.is_none() {
            if let Some(conversation) = conversation_id {
                handle.native_session_id = Some(conversation);
            }
        }
        Ok(handle)
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
        let env = Self::environment().await;
        let evidence = read_help_with(&executable, &self.prefix_args, &env).await;
        let (capabilities, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options);
        let version = crate::probe::version_of(&executable, &self.prefix_args).await;
        // The run needs the stream-json input surface; a CLI without it is not a
        // headless runtime Relay can drive.
        let headless =
            evidence.text.contains("--input-format") || evidence.text.contains("stream-json");
        let available = version.is_some() && headless;
        DetectionResult {
            runtimes: vec![Runtime {
                id: RUNTIME_ID.to_string(),
                adapter_id: ADAPTER_ID.to_string(),
                executable_path: executable,
                version,
                health: if available {
                    RuntimeHealth::Available
                } else {
                    RuntimeHealth::Unavailable
                },
                capabilities,
            }],
            diagnostics: vec![if available {
                "Antigravity validates its existing CLI login when a run starts".to_string()
            } else {
                "Antigravity CLI version or the stream-json headless surface could not be verified"
                    .to_string()
            }],
        }
    }

    /// Reads the runtime's own options from exactly the executable it is
    /// registered with, never from whatever discovery would find first.
    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions {
        let (executable, mut diagnostics) = probe_target(runtime, || self.executable());
        let Some(executable) = executable else {
            return RuntimeOptions::empty(&runtime.id, ADAPTER_ID, diagnostics.join("; "));
        };
        let env = Self::environment().await;
        let evidence = read_help_with(&executable, &self.prefix_args, &env).await;
        let (_, mut options) =
            probe_runtime_options(self.capabilities(), &evidence, &runtime.id, ADAPTER_ID);

        // `agy models` is the only catalogue form: the CLI rejects an
        // --output-format flag on it, so it is probed plain and parsed as the
        // two-column table it prints.
        if options.model_flag.is_some() {
            match capture_with(
                &executable,
                &["models".to_string()],
                &env,
                Duration::from_secs(20),
            )
            .await
            {
                Some((0, stdout, _)) => {
                    let catalogue = catalog::model_catalogue(&stdout);
                    if catalogue.is_empty() {
                        // Keep whatever the help already offered; a silent
                        // catalogue must not erase a usable list.
                        if options.models.is_empty() {
                            diagnostics.push(format!(
                                "`{executable} models` listed no models; Relay cannot offer model selection"
                            ));
                        }
                    } else {
                        options.models = catalogue;
                        options.source = OptionsSource::Cli;
                        options.diagnostics.retain(|note| !note.contains("model names"));
                    }
                }
                Some((code, _, _)) => diagnostics.push(format!(
                    "`{executable} models` exited with {code}; Relay cannot read the model catalogue"
                )),
                None => diagnostics.push(format!(
                    "`{executable} models` did not finish; Relay cannot read the model catalogue"
                )),
            }
        }
        // Effort levels are never inferred from a model id suffix: a `-low` or
        // `-high` catalogue name says nothing about which values `--effort`
        // accepts. Only the enum the CLI's own help prints is reported.
        diagnostics.append(&mut options.diagnostics);
        options.diagnostics = diagnostics;
        options
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        self.launch(input, None).await
    }

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        // The native conversation keeps its own model and effort; Relay does not
        // require (or invent) a selection on ResumeInput.
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
            Some(input.native_session_id),
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
    use relay_core::AccessMode;
    use std::os::unix::fs::PermissionsExt;

    const CONVERSATION: &str = "e2fcb976-501d-4873-bbca-a7e98d5d6c83";
    const OTHER_CONVERSATION: &str = "11ca403f-67a1-49b2-bdb4-805e1ae06aba";
    const MISSING_CONVERSATION: &str = "ee1dc1ae-b75a-4b9b-9b07-590efa41c62a";
    const EVIDENCE_PATH: &str = "/private/var/folders/ky/57_m8sjj3pqcd2z3w_1b_97w0000gn/T/agy-host-native-review-m0gk7f4y/evidence.txt";
    const WRITE_TARGET: &str = "/private/var/folders/ky/57_m8sjj3pqcd2z3w_1b_97w0000gn/T/agy-host-write-review-ped3i0as/output.txt";

    /// The complete `agy --help` captured on the host (`host-help.txt`). The real
    /// CLI prints it on stderr and exits 0.
    const HOST_HELP: &str = "\nUsage of agy:\n  --add-dir                       Add a directory to the workspace (repeatable) (default [])\n  --agent                         Agent for the current CLI session\n  -c                              Short alias for --continue\n  --continue                      Continue the most recent conversation\n  --conversation                  Resume a previous conversation by ID\n  --dangerously-skip-permissions  Auto-approve all tool permission requests without prompting\n  --disable-slash-commands        Disable slash command and skill expansion in print mode\n  --effort                        Reasoning effort for the current CLI session (low|medium|high|max)\n  -i                              Short alias for --prompt-interactive\n  --input-format                  Input format for print mode (text, stream-json). stream-json reads one NDJSON message per line from stdin and runs a turn for each; it requires --output-format stream-json (default text)\n  --json-schema                   Optional JSON schema string or path to a schema file to enforce structured output (for stream-json, only applicable to the final result)\n  --log-file                      Override CLI log file path\n  --mode                          Set the agent execution mode for this session (accept-edits, plan)\n  --model                         Model for the current CLI session\n  --new-project                   Create a new project for this session\n  --output-format                 Output format for print mode (text, json, stream-json) (default text)\n  -p                              Short alias for --print\n  --print                         Run a single prompt non-interactively and print the response\n  --print-timeout                 Optional time limit for print mode; 0 waits until the turn completes (default 0s)\n  --project                       Project ID or project name for the current CLI session\n  --prompt                        Alias for --print\n  --prompt-interactive            Run an initial prompt interactively and continue the session\n  --remote-control                Create a remote connection for the CLI session on start up\n  --sandbox                       Run in a sandbox with terminal restrictions enabled\n\nAvailable subcommands:\n  agent           List available agents\n  agents          List available agents\n  changelog       Show changelog and release notes\n  help            Show help for subcommands\n  install         Configure environment paths and shell settings\n  mcp             Manage MCP servers (add, remove, list, enable, disable)\n  mic-serve       Serve this machine's microphone to a CLI on another host\n  models          List available models\n  plugin          Manage plugins (install, uninstall, list, enable, disable)\n  plugins         Alias for plugin\n  remote-control  Manage the remote-control background daemon (start, status, stop)\n  update          Update CLI\n";

    /// The complete `agy models` table captured on the host (`host-models.txt`).
    const HOST_MODELS: &str = "gemini-3.8-flash-high\tGemini 3.8 Flash (High)\ngemini-3.8-flash-medium\tGemini 3.8 Flash (Medium)\ngemini-3.8-flash-low\tGemini 3.8 Flash (Low)\ngemini-3.7-flash-high\tGemini 3.7 Flash (High)\ngemini-3.7-flash-medium\tGemini 3.7 Flash (Medium)\ngemini-3.7-flash-low\tGemini 3.7 Flash (Low)\ngemini-3.6-flash-high\tGemini 3.6 Flash (High)\ngemini-3.6-flash-medium\tGemini 3.6 Flash (Medium)\ngemini-3.6-flash-low\tGemini 3.6 Flash (Low)\ngemini-3.1-pro-high\tGemini 3.1 Pro (High)\ngemini-3.1-pro-low\tGemini 3.1 Pro (Low)\nclaude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\nclaude-opus-4-6-thinking\tClaude Opus 4.6 (Thinking)\ngpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n";

    /// The complete `host-tools.ndjson` capture: init, a `view_file` tool call
    /// whose only path field is `AbsolutePath`, and a SUCCESS result.
    const HOST_TOOLS_NDJSON: &str = r#"{"event": "init", "conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba"}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 0, "state": "DONE", "step_type": "user_input"}}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 1, "state": "DONE", "step_type": "agent_response", "duration_seconds": 2.959446, "usage": {"input_tokens": 12391, "output_tokens": 91, "thinking_tokens": 0, "cache_read_tokens": 0, "total_tokens": 12482}}}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 2, "state": "ACTIVE", "step_type": "tool", "tool_name": "view_file", "tool_info": {"name": "view_file", "parameters": {"AbsolutePath": "/private/var/folders/ky/57_m8sjj3pqcd2z3w_1b_97w0000gn/T/agy-host-native-review-m0gk7f4y/evidence.txt"}}}}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 2, "state": "DONE", "step_type": "tool", "tool_name": "view_file", "duration_seconds": 0.057809, "tool_info": {"name": "view_file", "parameters": {"AbsolutePath": "/private/var/folders/ky/57_m8sjj3pqcd2z3w_1b_97w0000gn/T/agy-host-native-review-m0gk7f4y/evidence.txt"}, "output": "2 lines, 26 bytes"}}}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 3, "state": "ACTIVE", "step_type": "agent_response", "text_delta": "RELAY_AGY_READ_FIXTU"}}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 3, "state": "ACTIVE", "step_type": "agent_response", "text_delta": "RE_OK"}}
{"event": "step_update", "step_update": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "step_index": 3, "state": "DONE", "step_type": "agent_response", "text_delta": "\n", "duration_seconds": 3.016703, "usage": {"input_tokens": 12727, "output_tokens": 13, "thinking_tokens": 0, "cache_read_tokens": 0, "total_tokens": 12740}}}
{"event": "result", "result": {"conversation_id": "11ca403f-67a1-49b2-bdb4-805e1ae06aba", "status": "SUCCESS", "response": "RELAY_AGY_READ_FIXTURE_OK\n", "duration_seconds": 6.074175, "num_turns": 1, "usage": {"input_tokens": 25118, "output_tokens": 104, "thinking_tokens": 0, "cache_read_tokens": 0, "total_tokens": 25222}}}"#;

    /// The real `host-coding-tools.ndjson` `run_command` step. The command
    /// surface publishes the command as `CommandLine` (the official headless
    /// guide's sample is `{"CommandLine":"echo hello_headless_demo"}`), and this
    /// host run ended in a permission denial. A matching init is prepended so
    /// the step frames name the conversation the stream opened.
    const HOST_COMMAND_NDJSON: &str = r#"{"event": "init", "conversation_id": "c275bb24-1c41-4414-891c-6c7f05fa6922"}
{"event": "step_update", "step_update": {"conversation_id": "c275bb24-1c41-4414-891c-6c7f05fa6922", "step_index": 2, "state": "ACTIVE", "step_type": "tool", "tool_name": "run_command", "tool_info": {"name": "run_command", "parameters": {"CommandLine": "grep -n \"REVIEW_TOKEN\" input.txt"}}}}
{"event": "step_update", "step_update": {"conversation_id": "c275bb24-1c41-4414-891c-6c7f05fa6922", "step_index": 2, "state": "ERROR", "step_type": "tool", "tool_name": "run_command", "tool_info": {"name": "run_command", "parameters": {"CommandLine": "grep -n \"REVIEW_TOKEN\" input.txt"}, "error": {"type": "TOOL_ERROR", "message": "permission check failed for unsandboxed command: user denied permission to run command"}}}}"#;

    /// The real `host-write-summary.json` / `host-workspace-write-summary.json`
    /// sequence: a `write_to_file` step publishes only `TargetFile`, the CLI's own
    /// permission policy denies it, and the final `result` is `SUCCESS` with an
    /// empty response. The temp path is the captured one; the file never existed.
    const HOST_WRITE_DENIED_NDJSON: &str = r#"{"event": "init", "conversation_id": "34fe1dd8-fa5c-411f-9096-e60504396178"}
{"event": "step_update", "step_update": {"conversation_id": "34fe1dd8-fa5c-411f-9096-e60504396178", "step_index": 0, "state": "DONE", "step_type": "user_input"}}
{"event": "step_update", "step_update": {"conversation_id": "34fe1dd8-fa5c-411f-9096-e60504396178", "step_index": 1, "state": "DONE", "step_type": "agent_response"}}
{"event": "step_update", "step_update": {"conversation_id": "34fe1dd8-fa5c-411f-9096-e60504396178", "step_index": 2, "state": "ACTIVE", "step_type": "tool", "tool_name": "write_to_file", "tool_info": {"name": "write_to_file", "parameters": {"TargetFile": "/private/var/folders/ky/57_m8sjj3pqcd2z3w_1b_97w0000gn/T/agy-host-write-review-ped3i0as/output.txt"}}}}
{"event": "step_update", "step_update": {"conversation_id": "34fe1dd8-fa5c-411f-9096-e60504396178", "step_index": 2, "state": "ERROR", "step_type": "tool", "tool_name": "write_to_file", "tool_info": {"name": "write_to_file", "parameters": {"TargetFile": "/private/var/folders/ky/57_m8sjj3pqcd2z3w_1b_97w0000gn/T/agy-host-write-review-ped3i0as/output.txt"}, "error": {"type": "TOOL_ERROR", "message": "permission check failed for write_file: user denied permission for write_file"}}}}
{"event": "result", "result": {"conversation_id": "34fe1dd8-fa5c-411f-9096-e60504396178", "status": "SUCCESS", "response": ""}}"#;

    fn fixture() -> (
        tempfile::TempDir,
        AntigravityAdapter,
        Arc<ProcessSupervisor>,
    ) {
        fixture_with_timeout(Duration::from_secs(30))
    }

    fn fixture_with_timeout(
        timeout: Duration,
    ) -> (
        tempfile::TempDir,
        AntigravityAdapter,
        Arc<ProcessSupervisor>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("agy");
        std::fs::write(directory.path().join("help.txt"), HOST_HELP).unwrap();
        std::fs::write(directory.path().join("models.txt"), HOST_MODELS).unwrap();
        std::fs::write(
            &executable,
            r#"#!/bin/sh
case "$1" in
  --help) cat "$(dirname "$0")/help.txt" >&2; exit 0;;
  --version) echo 'agy 1.2.12'; exit 0;;
  models) cat "$(dirname "$0")/models.txt"; exit 0;;
esac
printf '%s\n' "$@" > args.txt
printf '%s\n%s\n%s\n' "$AGY_CLI_DISABLE_AUTO_UPDATE" "$NO_PROXY" "$no_proxy" > child-env.txt
session="e2fcb976-501d-4873-bbca-a7e98d5d6c83"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --conversation) session="$2"; shift;;
  esac
  shift
done
if [ -e drift ]; then session='00000000-0000-0000-0000-000000000000'; fi
# The real CLI announces its session before it reads any user input.
if [ -e no-init ]; then exec sleep 60; fi
printf '{"event":"init","conversation_id":"%s"}\n' "$session"
# The task arrives only after init; a rejected handshake closes stdin empty.
cat > stdin.json
if [ -e slow ]; then exec sleep 60; fi
if grep -q PRIVATE_TASK_MARKER stdin.json 2>/dev/null; then : > task-wrote.txt; fi
if [ -e deny-write ]; then
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"TargetFile":"/tmp/agy-deny/output.txt"}}}}'
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"ERROR","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"TargetFile":"/tmp/agy-deny/output.txt"},"error":{"type":"TOOL_ERROR","message":"permission check failed: user denied permission for write_file"}}}}'
  printf '{"event":"result","result":{"conversation_id":"%s","status":"SUCCESS","response":""}}\n' "$session"
  exit 0
fi
if [ -e recover ]; then
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"TargetFile":"/tmp/agy-recover/output.txt"}}}}'
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"ERROR","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"TargetFile":"/tmp/agy-recover/output.txt"},"error":{"type":"TOOL_ERROR","message":"permission check failed: user denied permission for write_file"}}}}'
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":3,"state":"ACTIVE","step_type":"tool","tool_name":"view_file","tool_info":{"name":"view_file","parameters":{"AbsolutePath":"/tmp/agy-recover/input.txt"}}}}'
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":3,"state":"DONE","step_type":"tool","tool_name":"view_file","tool_info":{"name":"view_file","parameters":{"AbsolutePath":"/tmp/agy-recover/input.txt"},"output":"ok"}}}'
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":4,"state":"ACTIVE","step_type":"agent_response","text_delta":"RELAY_AGY_RECOVERED"}}'
  printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":4,"state":"DONE","step_type":"agent_response","text_delta":"\n"}}'
  printf '{"event":"result","result":{"conversation_id":"%s","status":"SUCCESS","response":"RELAY_AGY_RECOVERED"}}\n' "$session"
  exit 0
fi
printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":0,"state":"DONE","step_type":"user_input"}}'
printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":1,"state":"ACTIVE","step_type":"tool","tool_name":"write_to_file","tool_info":{"parameters":{"path":"out.txt","content":"hi"}}}}'
printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":1,"state":"DONE","step_type":"tool","tool_name":"write_to_file","tool_info":{"output":"ok"}}}'
printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"RELAY_"}}'
printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"AGY_OK"}}'
printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"'"$session"'","step_index":2,"state":"DONE","step_type":"agent_response","text_delta":"\n","usage":{"input_tokens":10,"output_tokens":2,"signature":"PRIVATE_SIGNATURE"}}}'
printf '{"event":"result","result":{"conversation_id":"%s","status":"SUCCESS","response":"RELAY_AGY_OK","duration_seconds":1.5,"num_turns":1,"usage":{"input_tokens":10,"output_tokens":2,"signature":"PRIVATE_SIGNATURE"}}}\n' "$session"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let supervisor = Arc::new(ProcessSupervisor::new());
        let adapter = AntigravityAdapter::with_executable(executable.to_string_lossy())
            .with_supervisor(Arc::clone(&supervisor))
            .with_handshake_timeout(timeout);
        (directory, adapter, supervisor)
    }

    fn input(cwd: &std::path::Path) -> StartInput {
        StartInput {
            run_id: "run-test".into(),
            worker_session_id: "worker-test".into(),
            task: "PRIVATE_TASK_MARKER".into(),
            cwd: cwd.to_string_lossy().into_owned(),
            access_mode: AccessMode::Write,
            executable_path: None,
            model: Some("gemini-3.8-flash-high".into()),
            reasoning: Some("high".into()),
            instructions: Some("PRIVATE_INSTRUCTIONS_MARKER".into()),
        }
    }

    async fn drain(handle: &mut WorkerHandle) -> Vec<AdapterEvent> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut events = Vec::new();
            while let Some(event) = handle.events.recv().await {
                events.push(event);
            }
            events
        })
        .await
        .expect("worker must drain without startup/channel deadlock")
    }

    #[tokio::test]
    async fn launch_streams_the_task_on_stdin_merges_text_and_applies_selection() {
        let (directory, adapter, supervisor) = fixture();
        let parent_no_proxy = std::env::var_os("NO_PROXY");
        let mut handle = adapter.start(input(directory.path())).await.unwrap();
        assert_eq!(handle.native_session_id.as_deref(), Some(CONVERSATION));

        let events = drain(&mut handle).await;
        let terminal = events.last().unwrap();
        assert_eq!(terminal.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(terminal.data["summary"], "RELAY_AGY_OK");

        // Deltas are merged into one message, not replayed token by token.
        let finals: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| {
                event.event_type == RelayEventType::WorkerMessage && event.data["kind"] == "final"
            })
            .collect();
        assert_eq!(finals.len(), 1);
        assert_eq!(finals[0].data["text"], "RELAY_AGY_OK");

        // Tool calls normalize and carry the real file path for the Changes view.
        // The active and terminal updates do not produce two copies of the call.
        let edits: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolEdit)
            .collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].data["path"], "out.txt");
        let results: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolResult)
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data["output"], "ok");

        // Private usage fields never reach a normalized event.
        assert!(!format!("{events:?}").contains("PRIVATE_SIGNATURE"));

        // The task went over stdin, never the argument list.
        let args = std::fs::read_to_string(directory.path().join("args.txt")).unwrap();
        assert!(
            !args.contains("PRIVATE"),
            "task must stay out of the process args"
        );
        assert!(!args.contains("-p"));
        assert!(args.contains("--input-format\nstream-json"));
        assert!(args.contains("--output-format\nstream-json"));
        assert!(args.contains("--mode\naccept-edits"));
        assert!(args.contains("--model\ngemini-3.8-flash-high"));
        assert!(args.contains("--effort\nhigh"));
        let stdin = std::fs::read_to_string(directory.path().join("stdin.json")).unwrap();
        let payload: Value = serde_json::from_str(stdin.trim()).unwrap();
        assert_eq!(payload["event"], "user");
        let content = payload["message"]["content"].as_str().unwrap();
        assert!(content.contains("PRIVATE_TASK_MARKER"));
        assert!(content.contains("PRIVATE_INSTRUCTIONS_MARKER"));

        // The child was launched with the proxy bypass and auto-update off; the
        // parent environment is untouched.
        let child_env = std::fs::read_to_string(directory.path().join("child-env.txt")).unwrap();
        let lines = child_env.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "1");
        assert_eq!(lines[1], lines[2]);
        assert!(lines[1].split(',').any(|value| value == "127.0.0.1"));
        assert!(lines[1].split(',').any(|value| value == "::1"));
        assert_eq!(std::env::var_os("NO_PROXY"), parent_no_proxy);
        assert!(supervisor.tracked_keys().is_empty());
    }

    /// The real host denial shape through the whole pipe: a denied tool, an
    /// empty `SUCCESS` result and exit 0 must end in `WorkerFailed` with the
    /// permission reason — never an empty completion or a login-failure hint.
    #[tokio::test]
    async fn a_denied_tool_with_an_empty_success_fails_the_run() {
        let (directory, adapter, _) = fixture();
        std::fs::write(directory.path().join("deny-write"), "").unwrap();
        let mut handle = adapter.start(input(directory.path())).await.unwrap();
        let events = drain(&mut handle).await;
        let terminal = events.last().unwrap();
        assert_eq!(terminal.event_type, RelayEventType::WorkerFailed);
        assert!(terminal.data.get("summary").is_none());
        let message = terminal.data["message"].as_str().unwrap();
        assert!(message.contains("permission"));
        assert!(message.contains("not a login failure"));
        // The failed tool call and its result are still published, with the path
        // normalized from `TargetFile`.
        let edits: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolEdit)
            .collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].data["path"], "/tmp/agy-deny/output.txt");
        let results: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolResult)
            .collect();
        assert_eq!(results.len(), 1);
        assert!(!results[0].data["error"].is_null());
    }

    /// A denied tool followed by a clean tool and a real answer is a recovery:
    /// the later clean terminal supersedes the denial and the run completes.
    #[tokio::test]
    async fn a_denied_tool_recovered_by_a_later_clean_tool_still_completes() {
        let (directory, adapter, _) = fixture();
        std::fs::write(directory.path().join("recover"), "").unwrap();
        let mut handle = adapter.start(input(directory.path())).await.unwrap();
        let events = drain(&mut handle).await;
        let terminal = events.last().unwrap();
        assert_eq!(terminal.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(terminal.data["summary"], "RELAY_AGY_RECOVERED");
    }

    #[tokio::test]
    async fn resume_uses_the_native_conversation_and_rejects_drift() {
        let (directory, adapter, _) = fixture();
        let resume = ResumeInput {
            run_id: "run-test".into(),
            worker_session_id: "worker-test".into(),
            native_session_id: CONVERSATION.into(),
            task: "PRIVATE_TASK_MARKER follow up".into(),
            cwd: directory.path().to_string_lossy().into_owned(),
            access_mode: AccessMode::Write,
            executable_path: None,
            model: Some("ignored".into()),
            reasoning: Some("ignored".into()),
            instructions: None,
        };
        let mut handle = adapter.resume(resume.clone()).await.unwrap();
        assert_eq!(handle.native_session_id.as_deref(), Some(CONVERSATION));
        assert_eq!(
            drain(&mut handle).await.last().unwrap().event_type,
            RelayEventType::WorkerCompleted
        );
        let args = std::fs::read_to_string(directory.path().join("args.txt")).unwrap();
        assert!(args.contains(&format!("--conversation\n{CONVERSATION}")));
        // The native session keeps its own settings.
        assert!(!args.contains("--model") && !args.contains("--effort"));
        // The task was delivered only after the matching init.
        assert!(std::fs::read_to_string(directory.path().join("stdin.json"))
            .unwrap()
            .contains("PRIVATE_TASK_MARKER"));

        // Clear the successful run's artifacts so the rejected run is judged on
        // its own.
        std::fs::remove_file(directory.path().join("task-wrote.txt")).unwrap();
        std::fs::remove_file(directory.path().join("stdin.json")).unwrap();

        // A resume that returns a different conversation must fail, not drift,
        // and the task must never reach the new conversation.
        std::fs::write(directory.path().join("drift"), "").unwrap();
        let mut handle = adapter.resume(resume).await.unwrap();
        let terminal = drain(&mut handle).await.pop().unwrap();
        assert_eq!(terminal.event_type, RelayEventType::WorkerFailed);
        assert!(terminal.data["message"]
            .as_str()
            .unwrap()
            .contains("instead of the requested"));
        assert!(
            !directory.path().join("task-wrote.txt").exists(),
            "a rejected resume must not run the task"
        );
        let stdin =
            std::fs::read_to_string(directory.path().join("stdin.json")).unwrap_or_default();
        assert!(
            stdin.trim().is_empty(),
            "a rejected resume must not receive the task on stdin"
        );
    }

    #[tokio::test]
    async fn cancellation_is_reachable_before_output_and_reports_cancelled() {
        let (directory, adapter, supervisor) = fixture();
        std::fs::write(directory.path().join("slow"), "").unwrap();
        let mut handle = adapter.start(input(directory.path())).await.unwrap();
        // The worker key is registered at spawn, before any output is parsed.
        assert!(supervisor.is_tracked("worker-test"));
        let native = handle.native_session_id.clone().unwrap();
        adapter.cancel(&native).await.unwrap();
        assert_eq!(
            drain(&mut handle).await.last().unwrap().event_type,
            RelayEventType::WorkerCancelled
        );
        assert!(supervisor.tracked_keys().is_empty());
    }

    #[tokio::test]
    async fn a_read_only_workspace_mode_is_refused_rather_than_faked() {
        let (directory, adapter, _) = fixture();
        let mut request = input(directory.path());
        request.access_mode = AccessMode::ReadOnly;
        let error = adapter.start(request).await.err().unwrap();
        assert_eq!(error.code(), "ADAPTER_FAILURE");
        assert!(error.message().contains("read-only"));
    }

    #[tokio::test]
    async fn options_come_from_the_registered_executable_catalogue() {
        let (directory, _, _) = fixture();
        let adapter = AntigravityAdapter::with_executable("/missing/other-agy");
        let runtime = Runtime {
            id: "manual:agy".into(),
            adapter_id: ADAPTER_ID.into(),
            executable_path: directory.path().join("agy").to_string_lossy().into_owned(),
            version: None,
            health: RuntimeHealth::Available,
            capabilities: adapter.capabilities(),
        };
        let options = adapter.report_options(&runtime).await;
        assert_eq!(options.runtime_id, "manual:agy");
        assert_eq!(options.source, OptionsSource::Cli);
        // The real 14-row `agy models` table, with the label preserved.
        assert_eq!(options.models.len(), 14);
        assert_eq!(options.models[0].value, "gemini-3.8-flash-high");
        assert_eq!(
            options.models[0].label.as_deref(),
            Some("Gemini 3.8 Flash (High)")
        );
        assert_eq!(options.models[13].value, "gpt-oss-120b-medium");
        assert_eq!(options.model_flag.as_deref(), Some("--model"));
        assert_eq!(options.reasoning_flag.as_deref(), Some("--effort"));
        // The four values the CLI's own help enumerates, not suffixes of ids.
        assert_eq!(
            options
                .levels
                .iter()
                .map(|level| level.value.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "medium", "high", "max"]
        );
        assert!(!options
            .diagnostics
            .iter()
            .any(|note| note.contains("model names")));
    }

    /// The complete real help must yield exactly the four `--effort` values, and
    /// each one must reach the child's argument list.
    #[test]
    fn the_real_help_reports_every_effort_level_and_it_is_applied() {
        let evidence = crate::probe::HelpEvidence {
            text: HOST_HELP.to_string(),
            executable_path: "/usr/bin/agy".to_string(),
        };
        let (_, options) = probe_runtime_options(
            AntigravityAdapter::new().capabilities(),
            &evidence,
            RUNTIME_ID,
            ADAPTER_ID,
        );
        assert_eq!(options.reasoning_flag.as_deref(), Some("--effort"));
        assert_eq!(
            options
                .levels
                .iter()
                .map(|level| level.value.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "medium", "high", "max"]
        );
        for level in ["low", "medium", "high", "max"] {
            let args = with_selection_args(
                &["--output-format".to_string(), "stream-json".to_string()],
                &Selection {
                    model: None,
                    reasoning: Some(level.to_string()),
                },
                &options,
            );
            assert_eq!(
                args,
                vec!["--output-format", "stream-json", "--effort", level]
            );
        }
    }

    /// The real `view_file` step publishes `AbsolutePath`; it must land under the
    /// canonical `path` the console reads, while the raw parameters stay intact.
    #[test]
    fn the_real_tool_sample_maps_absolute_path_to_the_canonical_path() {
        let mut state = StreamState::default();
        let mut events = Vec::new();
        for line in HOST_TOOLS_NDJSON.lines() {
            let parsed = state.parse(line);
            assert!(
                parsed.error_message.is_none(),
                "unexpected rejection: {:?}",
                parsed.error_message
            );
            events.extend(parsed.events);
        }
        let reads: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolRead)
            .collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].data["tool"], "view_file");
        assert_eq!(reads[0].data["path"], EVIDENCE_PATH);
        assert_eq!(
            reads[0].data["parameters"]["AbsolutePath"], EVIDENCE_PATH,
            "the original parameters must be preserved"
        );
    }

    /// The real `run_command` step publishes the command as `CommandLine`; it
    /// must land under the canonical `command` the console reads, and the public
    /// tool error must still be recorded as a permission denial.
    #[test]
    fn the_real_run_command_sample_maps_command_line_and_records_the_denial() {
        let mut stream = StreamState::default();
        let mut events = Vec::new();
        let mut last_terminal = None;
        for line in HOST_COMMAND_NDJSON.lines() {
            let parsed = stream.parse(line);
            assert!(
                parsed.error_message.is_none(),
                "unexpected rejection: {:?}",
                parsed.error_message
            );
            if parsed.tool_terminal.is_some() {
                last_terminal = parsed.tool_terminal;
            }
            events.extend(parsed.events);
        }
        let commands: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolCommand)
            .collect();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].data["tool"], "run_command");
        assert_eq!(
            commands[0].data["command"],
            "grep -n \"REVIEW_TOKEN\" input.txt"
        );
        assert_eq!(
            commands[0].data["parameters"]["CommandLine"], "grep -n \"REVIEW_TOKEN\" input.txt",
            "the original parameters must be preserved"
        );
        let results: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolResult)
            .collect();
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].data["error"].is_null(),
            "the failed ToolResult must be preserved"
        );
        assert_eq!(last_terminal, Some(ToolTerminalState::PermissionDenied));
    }

    /// The real `host-write-summary.json` sequence: a `write_to_file` step whose
    /// only path field is `TargetFile`, denied by the CLI's own permission
    /// policy, followed by `SUCCESS` with an empty response. The path must be
    /// normalized and the outcome must be a failure, not an empty completion.
    #[test]
    fn the_real_write_denial_sample_is_normalized_and_never_an_empty_success() {
        let mut stream = StreamState::default();
        let mut events = Vec::new();
        let mut last_terminal = None;
        let mut final_text = None;
        let mut result_status = None;
        for line in HOST_WRITE_DENIED_NDJSON.lines() {
            let parsed = stream.parse(line);
            assert!(
                parsed.error_message.is_none(),
                "unexpected rejection: {:?}",
                parsed.error_message
            );
            if parsed.tool_terminal.is_some() {
                last_terminal = parsed.tool_terminal;
            }
            if let Some(text) = parsed.final_text {
                final_text = Some(text);
            }
            if let Some(status) = parsed.result_status {
                result_status = Some(status);
            }
            events.extend(parsed.events);
        }
        let edits: Vec<&AdapterEvent> = events
            .iter()
            .filter(|event| event.event_type == RelayEventType::ToolEdit)
            .collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].data["path"], WRITE_TARGET);
        assert_eq!(
            edits[0].data["parameters"]["TargetFile"], WRITE_TARGET,
            "the original parameters must be preserved"
        );
        assert_eq!(result_status.as_deref(), Some("SUCCESS"));
        assert_eq!(final_text.as_deref(), Some(""));
        assert_eq!(last_terminal, Some(ToolTerminalState::PermissionDenied));

        // The exact outcome the run produces must fail, not report an empty
        // success with no answer.
        let event = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text,
            result_status,
            tool_terminal: last_terminal,
            ..StreamOutcome::default()
        });
        assert_eq!(event.event_type, RelayEventType::WorkerFailed);
        assert!(event.data.get("summary").is_none());
        let message = event.data["message"].as_str().unwrap();
        assert!(message.contains("permission"));
        assert!(message.contains("not a login failure"));
    }

    /// The terminal decision for an empty `SUCCESS` depends on how the last tool
    /// ended: a denied or failed tool fails, a clean tool or an actual answer
    /// does not.
    #[test]
    fn an_empty_success_after_a_failed_tool_is_not_a_completion() {
        let denied = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some(String::new()),
            result_status: Some("SUCCESS".into()),
            tool_terminal: Some(ToolTerminalState::PermissionDenied),
            ..StreamOutcome::default()
        });
        assert_eq!(denied.event_type, RelayEventType::WorkerFailed);
        assert!(denied.data.get("summary").is_none());
        let message = denied.data["message"].as_str().unwrap();
        assert!(message.contains("permission"));
        assert!(message.contains("not a login failure"));

        // A plain tool error is also a failure, with its own non-login reason.
        let failed = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some(String::new()),
            result_status: Some("SUCCESS".into()),
            tool_terminal: Some(ToolTerminalState::Error),
            ..StreamOutcome::default()
        });
        assert_eq!(failed.event_type, RelayEventType::WorkerFailed);
        assert!(failed.data["message"]
            .as_str()
            .unwrap()
            .contains("tool error"));

        // A clean, completed tool with no error keeps the empty success.
        let clean = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some(String::new()),
            result_status: Some("SUCCESS".into()),
            tool_terminal: Some(ToolTerminalState::Ok),
            ..StreamOutcome::default()
        });
        assert_eq!(clean.event_type, RelayEventType::WorkerCompleted);

        // A recovered run that produced an actual answer is still a success.
        let recovered = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some("recovered".into()),
            result_status: Some("SUCCESS".into()),
            tool_terminal: Some(ToolTerminalState::PermissionDenied),
            ..StreamOutcome::default()
        });
        assert_eq!(recovered.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(recovered.data["summary"], "recovered");
    }

    /// `host-missing-conversation.ndjson`: an unknown resume id silently starts a
    /// new conversation, so the requested id must be rejected before the task.
    #[test]
    fn the_real_missing_conversation_sample_is_rejected_not_resumed() {
        let mut state = StreamState {
            requested_conversation: Some(MISSING_CONVERSATION.to_string()),
            ..StreamState::default()
        };
        let init = state.parse(
            r#"{"event": "init", "conversation_id": "59a37ee9-2341-4ae5-8c2c-078ec3629139"}"#,
        );
        assert!(init
            .error_message
            .as_deref()
            .unwrap()
            .contains("instead of the requested"));
        assert!(init.session_id.is_none());
        // The wrong id never becomes the session, even if the rest of the real
        // stream follows it.
        let result = state.parse(
            r#"{"event": "result", "result": {"conversation_id": "59a37ee9-2341-4ae5-8c2c-078ec3629139", "status": "SUCCESS", "response": "RELAY_AGY_MISSING_PROBE_OK\n"}}"#,
        );
        assert!(result.session_id.is_none());
        assert!(result.final_text.is_none());
    }

    /// A frame that changes the conversation mid-stream is fatal, and the
    /// rejection is sticky so no later frame can revive the wrong session.
    #[test]
    fn a_mid_stream_conversation_change_is_rejected() {
        let mut state = StreamState {
            requested_conversation: Some(CONVERSATION.to_string()),
            ..StreamState::default()
        };
        let init = state.parse(&format!(
            r#"{{"event":"init","conversation_id":"{CONVERSATION}"}}"#
        ));
        assert!(init.error_message.is_none());
        let switched = state.parse(&format!(
            r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":0,"state":"ACTIVE","step_type":"agent_response","text_delta":"x"}}}}"#
        ));
        assert!(switched
            .error_message
            .as_deref()
            .unwrap()
            .contains("switched"));
        let revived = state.parse(&format!(
            r#"{{"event":"result","result":{{"conversation_id":"{CONVERSATION}","status":"SUCCESS","response":"ok"}}}}"#
        ));
        assert!(revived.session_id.is_none());
        assert!(revived.final_text.is_none());
    }

    /// A CLI that never announces a session must fail cleanly: no task, no stuck
    /// worker, and the child is reaped.
    #[tokio::test]
    async fn a_missing_init_never_delivers_the_task_and_reaps_the_child() {
        let (directory, adapter, supervisor) = fixture_with_timeout(Duration::from_millis(300));
        std::fs::write(directory.path().join("no-init"), "").unwrap();
        let error = adapter.start(input(directory.path())).await.err().unwrap();
        assert_eq!(error.code(), "ADAPTER_FAILURE");
        assert!(
            !directory.path().join("task-wrote.txt").exists(),
            "a worker that never initialized must not run the task"
        );
        wait_untracked(&supervisor).await;
    }

    /// Cancelling by worker key before the CLI announces anything must stop it
    /// without deadlocking, even though no handle has been returned yet.
    #[tokio::test]
    async fn cancellation_reaches_a_worker_before_any_output() {
        let (directory, adapter, supervisor) = fixture_with_timeout(Duration::from_secs(5));
        std::fs::write(directory.path().join("no-init"), "").unwrap();
        let request = input(directory.path());
        let adapter = Arc::new(adapter);
        let starting = {
            let adapter = Arc::clone(&adapter);
            tokio::spawn(async move { adapter.start(request).await })
        };
        for _ in 0..100 {
            if supervisor.is_tracked("worker-test") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(supervisor.is_tracked("worker-test"));
        supervisor.terminate("worker-test");
        let result = tokio::time::timeout(Duration::from_secs(5), starting)
            .await
            .expect("cancel before output must not deadlock")
            .unwrap();
        assert!(result.is_err());
        wait_untracked(&supervisor).await;
        assert!(!directory.path().join("task-wrote.txt").exists());
    }

    async fn wait_untracked(supervisor: &ProcessSupervisor) {
        for _ in 0..200 {
            if supervisor.tracked_keys().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("worker process was not reaped");
    }

    #[test]
    fn completion_requires_success_zero_exit_and_a_result() {
        let success = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some("done".into()),
            result_status: Some("SUCCESS".into()),
            ..StreamOutcome::default()
        });
        assert_eq!(success.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(success.data["summary"], "done");

        // A zero exit without a result frame is not success.
        let missing = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some("partial".into()),
            ..StreamOutcome::default()
        });
        assert_eq!(missing.event_type, RelayEventType::WorkerFailed);
        // The failure carries its reason where the console reads it, and no
        // tempting empty `summary` that would hide it.
        assert!(missing.data.get("summary").is_none());
        assert!(missing.data["message"]
            .as_str()
            .unwrap()
            .contains("without a successful result"));

        // Nor is a result that failed, or a nonzero exit.
        assert_eq!(
            terminal_event(&StreamOutcome {
                exit_code: Some(1),
                result_status: Some("SUCCESS".into()),
                ..StreamOutcome::default()
            })
            .event_type,
            RelayEventType::WorkerFailed
        );
        assert_eq!(
            terminal_event(&StreamOutcome {
                exit_code: Some(0),
                result_status: Some("ERROR".into()),
                ..StreamOutcome::default()
            })
            .event_type,
            RelayEventType::WorkerFailed
        );
        // A real SIGTERM makes the CLI wind down with `ERROR` and exit 1, but an
        // ordinary error must not be mistaken for a user cancellation.
        let ordinary_error = terminal_event(&StreamOutcome {
            exit_code: Some(1),
            result_status: Some("ERROR".into()),
            ..StreamOutcome::default()
        });
        assert_eq!(ordinary_error.event_type, RelayEventType::WorkerFailed);
        assert_ne!(ordinary_error.event_type, RelayEventType::WorkerCancelled);
        assert_eq!(
            terminal_event(&StreamOutcome {
                exit_code: Some(0),
                result_status: Some("SUCCESS".into()),
                error_message: Some("drifted".into()),
                ..StreamOutcome::default()
            })
            .event_type,
            RelayEventType::WorkerFailed
        );
        // A SUCCESS result and exit 0 are still not success when reading the
        // process failed: the answer cannot be trusted.
        let unreadable = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            final_text: Some("looks complete".into()),
            result_status: Some("SUCCESS".into()),
            spawn_error: Some("worker stdout read failed".into()),
            ..StreamOutcome::default()
        });
        assert_eq!(unreadable.event_type, RelayEventType::WorkerFailed);
        assert!(unreadable.data.get("summary").is_none());
        assert!(unreadable.data["message"]
            .as_str()
            .unwrap()
            .contains("worker stdout read failed"));
    }

    #[test]
    fn cancellation_covers_signals_and_shell_exit_codes() {
        let cancelled = terminal_event(&StreamOutcome {
            exit_code: Some(143),
            ..StreamOutcome::default()
        });
        assert_eq!(cancelled.event_type, RelayEventType::WorkerCancelled);
        assert!(cancelled.data.get("summary").is_none());
        for status in ["CANCELED", "CANCELLED", "INTERRUPTED"] {
            assert_eq!(
                terminal_event(&StreamOutcome {
                    exit_code: Some(0),
                    result_status: Some(status.into()),
                    ..StreamOutcome::default()
                })
                .event_type,
                RelayEventType::WorkerCancelled
            );
        }
        for exit_code in [130, 143] {
            assert_eq!(
                terminal_event(&StreamOutcome {
                    exit_code: Some(exit_code),
                    ..StreamOutcome::default()
                })
                .event_type,
                RelayEventType::WorkerCancelled
            );
        }
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGKILL] {
            assert_eq!(
                terminal_event(&StreamOutcome {
                    exit_code: None,
                    signal: Some(signal),
                    ..StreamOutcome::default()
                })
                .event_type,
                RelayEventType::WorkerCancelled
            );
        }
    }

    #[test]
    fn malformed_output_fails_without_forwarding_raw_data() {
        let mut state = StreamState::default();
        let parsed = state.parse("PRIVATE secret invalid JSON");
        assert_eq!(
            parsed.error_message.as_deref(),
            Some("Antigravity emitted malformed JSON")
        );
        assert!(parsed.events.is_empty());
    }

    #[test]
    fn the_host_sample_is_parsed_into_the_expected_shape() {
        let mut state = StreamState::default();
        let init = state.parse(&format!(
            r#"{{"event":"init","conversation_id":"{OTHER_CONVERSATION}"}}"#
        ));
        assert_eq!(init.session_id.as_deref(), Some(OTHER_CONVERSATION));
        let delta = state.parse(&format!(
            r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"RELAY_AGY_HOST_REVIEW_OK"}}}}"#
        ));
        assert_eq!(delta.events.len(), 1);
        assert_eq!(delta.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(delta.events[0].data["kind"], "delta");
        assert_eq!(delta.events[0].data["text"], "RELAY_AGY_HOST_REVIEW_OK");
        assert_eq!(
            delta.events[0].data["messageStart"], true,
            "the first increment of a step opens one assistant message"
        );
        let continuation = state.parse(&format!(
            r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":" RELAY_AGY"}}}}"#
        ));
        assert_eq!(continuation.events[0].data["text"], " RELAY_AGY");
        assert!(
            continuation.events[0].data.get("messageStart").is_none(),
            "later increments of the same step extend that message"
        );
        let next_step = state.parse(&format!(
            r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"SECOND"}}}}"#
        ));
        assert_eq!(
            next_step.events[0].data["messageStart"], true,
            "a new agent_response step is a new message"
        );
        let result = state.parse(&format!(
            r#"{{"event":"result","result":{{"conversation_id":"{OTHER_CONVERSATION}","status":"SUCCESS","response":"RELAY_AGY_HOST_REVIEW_OK"}}}}"#
        ));
        assert_eq!(
            result.final_text.as_deref(),
            Some("RELAY_AGY_HOST_REVIEW_OK")
        );
        assert_eq!(result.result_status.as_deref(), Some("SUCCESS"));
        assert_eq!(result.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(result.events[0].data["kind"], "final");
    }

    /// The `text_delta` frames reach the shared assistant-message path: the
    /// chunks of one `agent_response` step merge into one message, and a new
    /// step opens another.
    #[test]
    fn agent_response_chunks_reach_the_shared_assistant_message_path() {
        let mut state = StreamState::default();
        let mut seq = 0;
        let mut events = Vec::new();
        for frame in [
            format!(r#"{{"event":"init","conversation_id":"{OTHER_CONVERSATION}"}}"#),
            format!(
                r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"RELAY_"}}}}"#
            ),
            format!(
                r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"AGY_OK"}}}}"#
            ),
            format!(
                r#"{{"event":"step_update","step_update":{{"conversation_id":"{OTHER_CONVERSATION}","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"SECOND"}}}}"#
            ),
        ] {
            for event in state.parse(&frame).events {
                seq += 1;
                events.push(crate::test_support::relay_event(seq, event));
            }
        }
        assert_eq!(
            crate::test_support::assistant_messages(&events),
            vec!["RELAY_AGY_OK".to_string(), "SECOND".to_string()]
        );
    }

    /// An init that is not the requested conversation, and one that is not even a
    /// UUID, are both rejected before any session is persisted.
    #[test]
    fn a_non_uuid_or_missing_init_id_is_rejected() {
        let mut state = StreamState {
            requested_conversation: Some(CONVERSATION.to_string()),
            ..StreamState::default()
        };
        let non_uuid = state.parse(r#"{"event":"init","conversation_id":"c-1"}"#);
        assert!(non_uuid.session_id.is_none());
        assert!(non_uuid.error_message.is_some());

        let mut state = StreamState::default();
        let missing = state.parse(r#"{"event":"init"}"#);
        assert!(missing.session_id.is_none());
        assert!(missing.error_message.is_some());
    }
}
