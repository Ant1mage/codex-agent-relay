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
use std::time::Duration;

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, DetectionResult, EnforcementSet, ModelOption,
    OptionsSource, RelayError, RelayEventType, Result, ResumeInput, Runtime, RuntimeHealth,
    RuntimeOptions, StartInput, WorkerHandle,
};

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::probe::{
    capture_with, discover_executable, probe_runtime_options, probe_target, read_help, version_of,
    with_selection_args, Selection,
};

pub const ADAPTER_ID: &str = "deepseek-harness";
pub const RUNTIME_ID: &str = "runtime:deepseek-harness";
/* ------------------------------------------------------------------ */
/* How a model choice reaches a dsh run                                */
/* ------------------------------------------------------------------ */

/// Composing a whole profile is heavier than answering `--help`.
const DUMP_TIMEOUT: Duration = Duration::from_secs(15);

/// What one dsh profile says about its own model selection.
///
/// Read from `dsh --profile <name> --dump-config`, the CLI's own report of its
/// composed configuration: Relay never guesses a key or invents a model name.
#[derive(Debug, Default, PartialEq)]
pub struct DshComposition {
    /// The provider route the profile's default model selection points at.
    pub provider: Option<String>,
    /// The catalogue that route declares, when the profile declares one.
    pub models: Vec<ModelOption>,
    /// The composed system-prompt persona, when the profile states it inline.
    pub persona_prefix: Option<String>,
    pub persona_suffix: Option<String>,
}

impl DshComposition {
    /// The composition row in which a provider route keeps its catalogue.
    fn catalogue_row(provider: &str) -> Option<&'static str> {
        match provider {
            "deepseek-official" => Some("llm-deepseek"),
            _ => None,
        }
    }

    /// The persona Relay can restate in an overlay.
    ///
    /// A block scalar is deliberately *not* rewritten: Relay cannot reproduce it
    /// byte for byte, and a wrong rewrite would damage the profile's own prompt.
    fn persona(&self) -> Option<(String, String)> {
        Some((
            self.persona_prefix.clone().unwrap_or_default(),
            self.persona_suffix.clone().unwrap_or_default(),
        ))
    }
}

/// True when a dumped scalar is a block scalar Relay will not restate.
fn is_block_scalar(value: &str) -> bool {
    matches!(value.trim(), "|" | ">" | "|-" | ">-" | "|+" | ">+")
}

fn scalar(value: &str) -> String {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .unwrap_or(trimmed);
    unquoted.trim().to_string()
}

/// Reads the two facts Relay needs out of a composed profile dump.
///
/// The dump is machine-generated YAML with a fixed two-space shape, so a small
/// line reader is enough. It is tested against a real dump below.
pub fn parse_composition(text: &str) -> DshComposition {
    let mut provider: Option<String> = None;
    let mut declared: Vec<ModelOption> = Vec::new();
    let mut persona_prefix: Option<String> = None;
    let mut persona_suffix: Option<String> = None;
    let mut entry = String::new();
    let mut in_config = false;
    let mut in_models = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.starts_with("- id:") {
            entry = scalar(trimmed.trim_start_matches("- id:"));
            in_config = false;
            in_models = false;
            continue;
        }
        if !in_config && !(indent == 2 && trimmed == "config:") {
            continue;
        }
        if indent == 2 && trimmed == "config:" {
            in_config = true;
            in_models = false;
            continue;
        }
        match (entry.as_str(), indent) {
            ("agent-default-model", 4) if trimmed.starts_with("provider:") => {
                provider = Some(scalar(trimmed.trim_start_matches("provider:")));
            }
            ("system-prompt", 4) if trimmed.starts_with("personaPrefix:") => {
                let value = trimmed.trim_start_matches("personaPrefix:");
                if !is_block_scalar(value) {
                    persona_prefix = Some(scalar(value));
                }
            }
            ("system-prompt", 4) if trimmed.starts_with("personaSuffix:") => {
                let value = trimmed.trim_start_matches("personaSuffix:");
                if !is_block_scalar(value) {
                    persona_suffix = Some(scalar(value));
                }
            }
            ("llm-deepseek", 4) if trimmed == "models:" => in_models = true,
            ("llm-deepseek", 6) if in_models && trimmed.starts_with("- id:") => {
                declared.push(ModelOption {
                    value: scalar(trimmed.trim_start_matches("- id:")),
                    label: None,
                });
            }
            ("llm-deepseek", 8) if in_models && trimmed.starts_with("name:") => {
                if let Some(last) = declared.last_mut() {
                    last.label = Some(scalar(trimmed.trim_start_matches("name:")));
                }
            }
            _ => {}
        }
    }
    let models = match provider.as_deref().and_then(DshComposition::catalogue_row) {
        Some("llm-deepseek") => declared,
        _ => Vec::new(),
    };
    DshComposition {
        provider,
        models,
        persona_prefix,
        persona_suffix,
    }
}

/// Composes the headless profile exactly the way a run would, and reads back the
/// two facts Relay needs.
async fn read_composition(
    executable: &str,
    prefix: &[String],
    env: &[(String, String)],
) -> Option<DshComposition> {
    let mut args = prefix.to_vec();
    args.extend(["--profile".to_string(), "headless".to_string()]);
    args.push("--dump-config".to_string());
    let (code, stdout, _) = capture_with(executable, &args, env, DUMP_TIMEOUT).await?;
    (code == 0).then(|| parse_composition(&stdout))
}

/// Writes the one-run overlay that makes an Agent Profile real.
///
/// The launcher applies \`--patch\` files after the profile layer, so both rows
/// replace exactly what the profile composed: the default model selection, and the
/// system-prompt persona when the profile's instructions need their native slot.
/// Every value is a double-quoted YAML scalar with escaped newlines, so a model id
/// or instruction text can never change the shape of the document.
fn write_overlay(
    model: Option<&(String, String)>,
    persona: Option<&(String, String)>,
) -> std::io::Result<PathBuf> {
    let mut body = String::from("# Written by Relay for a single run.\n");
    if let Some((provider, value)) = model {
        body.push_str("- id: agent-default-model\n  config:\n    provider: ");
        body.push_str(&quoted(provider));
        body.push_str("\n    model: ");
        body.push_str(&quoted(value));
        body.push('\n');
    }
    if let Some((prefix, suffix)) = persona {
        body.push_str("- id: system-prompt\n  config:\n    personaPrefix: ");
        body.push_str(&quoted(prefix));
        body.push_str("\n    personaSuffix: ");
        body.push_str(&quoted(suffix));
        body.push('\n');
    }
    let directory = std::env::temp_dir().join("relay-run-overlays");
    std::fs::create_dir_all(&directory)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let path = directory.join(format!("run-{}-{stamp}.yml", std::process::id()));
    std::fs::write(&path, body)?;
    Ok(path)
}

/// A YAML double-quoted scalar: the only shape Relay writes.
fn quoted(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

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

/// A run's task plus the overlay that carries its per-run choices.
struct PreparedRun {
    task: String,
    overlay: Option<PathBuf>,
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

    /// The environment a run starts with.
    ///
    /// Relay's access mode is not advice for this runtime: the sandbox row reads
    /// DSH_PERMISSION_MODE, so a read-only run really is read-only. An explicit
    /// deployment override in the adapter's own environment wins.
    fn run_environment(&self, access_mode: relay_core::AccessMode) -> Vec<(String, String)> {
        let mut environment = self.environment.clone();
        if environment
            .iter()
            .any(|(key, _)| key == "DSH_PERMISSION_MODE")
        {
            return environment;
        }
        let mode = match access_mode {
            relay_core::AccessMode::ReadOnly | relay_core::AccessMode::Propose => "read-only",
            relay_core::AccessMode::Write => "workspace-write",
        };
        environment.push(("DSH_PERMISSION_MODE".to_string(), mode.to_string()));
        environment
    }

    /// Everything one run needs beyond its bare task.
    ///
    /// DeepSeek Harness applies per-run choices through a --patch overlay, so this
    /// is the whole mechanism behind the model picker and behind the profile's
    /// instructions: both become rows the launcher layers over the profile.
    async fn prepare_run(&self, executable: &str, input: &StartInput) -> Result<PreparedRun> {
        let model = input
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let instructions = input
            .instructions
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if model.is_none() && instructions.is_none() {
            return Ok(PreparedRun {
                task: input.task.clone(),
                overlay: None,
            });
        }

        let composition = read_composition(executable, &self.prefix_args, &self.environment).await;

        // The profile is the only authority for which model ids its provider route
        // accepts, so a choice outside the catalogue is refused instead of being
        // launched and silently ignored.
        let mut selection = None;
        if let Some(model) = &model {
            let Some(composition) = composition.as_ref() else {
                return Err(RelayError::new(
                    "OPERATION_UNSUPPORTED",
                    "This DeepSeek Harness installation did not report a composed configuration, so Relay cannot apply a model to it",
                ));
            };
            let Some(provider) = composition.provider.clone() else {
                return Err(RelayError::new(
                    "OPERATION_UNSUPPORTED",
                    "This DeepSeek Harness profile declares no default model selection, so Relay cannot apply a model to it",
                ));
            };
            if !composition.models.is_empty()
                && !composition
                    .models
                    .iter()
                    .any(|option| option.value == *model)
            {
                return Err(RelayError::new(
                    "INVALID_REQUEST",
                    format!(
                        "This dsh profile does not declare the model {model}; Relay will not pass a model the runtime cannot apply"
                    ),
                ));
            }
            selection = Some((provider, model.clone()));
        }

        // Instructions belong in the runtime's own system prompt, not appended to
        // the task. A profile that states its persona as a block scalar is not
        // restated — the envelope covers that case instead of dropping the text.
        let mut task = input.task.clone();
        let mut persona = None;
        if let Some(instructions) = &instructions {
            match composition
                .as_ref()
                .and_then(|composition| composition.persona())
            {
                Some((prefix, suffix)) => {
                    let suffix = if suffix.trim().is_empty() {
                        instructions.clone()
                    } else {
                        format!(
                            "{suffix}

{instructions}"
                        )
                    };
                    persona = Some((prefix, suffix));
                }
                None => {
                    task = crate::instructions::enveloped(&input.task, Some(instructions));
                }
            }
        }

        let overlay = write_overlay(selection.as_ref(), persona.as_ref()).map_err(|error| {
            RelayError::new(
                "ADAPTER_FAILURE",
                format!("cannot write the run overlay: {error}"),
            )
        })?;
        Ok(PreparedRun {
            task,
            overlay: Some(overlay),
        })
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
        let prepared = self.prepare_run(&executable, &input).await?;

        if !features.json {
            return self
                .launch_plain(&executable, input, resume_session_id, prepared)
                .await;
        }

        let mut args = self.prefix_args.clone();
        args.extend(["--profile".to_string(), "headless".to_string()]);
        if let Some(path) = &prepared.overlay {
            args.push("--patch".to_string());
            args.push(path.display().to_string());
        }
        args.push("--json".to_string());
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
                env: self.run_environment(input.access_mode),
                stdin: Some(prepared.task.clone()),
                supervisor_key: input.worker_session_id.clone(),
                cleanup: prepared.overlay,
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
        prepared: PreparedRun,
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
        if let Some(path) = &prepared.overlay {
            args.push("--patch".to_string());
            args.push(path.display().to_string());
        }
        if let Some(session_id) = &resume_session_id {
            args.push("--session-id".to_string());
            args.push(session_id.clone());
        }
        args.push(prepared.task.clone());
        let args = with_selection_args(&args, &self.selection(&input), &options);

        run_cli(
            StreamSpec {
                executable: executable.to_string(),
                args,
                cwd: input.cwd.clone(),
                env: self.run_environment(input.access_mode),
                stdin: None,
                supervisor_key: input.worker_session_id.clone(),
                cleanup: prepared.overlay,
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
            // DeepSeek Harness has a real sandbox: the sandbox-policy row reads its
            // mode from DSH_PERMISSION_MODE, which is exactly the deployment
            // override Relay maps an access mode onto. It has no way to remove
            // shell or network access, so those stay unenforced.
            enforcement: EnforcementSet {
                workspace: true,
                commands: false,
                network: false,
            },
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

    /// What a run of this runtime can really apply.
    ///
    /// DeepSeek Harness's headless profile has neither a `--model` nor a
    /// `--reasoning` flag — its own `--help` proves it — so Relay never reports a
    /// flag for it. A model is applied through a one-run `--patch` overlay instead,
    /// and the catalogue comes from the profile's own composed configuration. A
    /// model list is therefore offered only when Relay found both a provider route
    /// and a catalogue for it, because only then is a choice real. Reasoning is
    /// never offered: dsh keeps `reasoningEffort` in the user's global settings
    /// document, which Relay does not write.
    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions {
        let (executable, mut diagnostics) = probe_target(runtime, || self.executable());
        let Some(executable) = executable else {
            return RuntimeOptions::empty(&runtime.id, ADAPTER_ID, diagnostics.join("; "));
        };
        let mut options = self.refresh_options(&executable).await;
        options.runtime_id = runtime.id.clone();
        diagnostics.append(&mut options.diagnostics);
        options.diagnostics = diagnostics;

        // The profile is the only authority for which models the route accepts.
        match read_composition(&executable, &self.prefix_args, &self.environment).await {
            Some(composition) => match composition.provider {
                Some(provider) if !composition.models.is_empty() => {
                    options.models = composition.models;
                    options.source = OptionsSource::Cli;
                    options.diagnostics.push(format!(
                        "DeepSeek Harness applies a model through a one-run --patch overlay on the {provider} route, not a command line flag; Relay writes that overlay for every run"
                    ));
                }
                Some(provider) => options.diagnostics.push(format!(
                    "This dsh profile declares no model catalogue for the {provider} route, so Relay cannot offer a model list"
                )),
                None => options.diagnostics.push(
                    "This dsh profile declares no default model selection, so Relay cannot offer a model list"
                        .to_string(),
                ),
            },
            None => options.diagnostics.push(
                "dsh did not report a composed configuration, so Relay cannot offer a model list"
                    .to_string(),
            ),
        }
        options.diagnostics.push(
            "DeepSeek Harness keeps reasoning effort in the user's global settings document rather than in a per-run flag or overlay, so Relay does not offer reasoning selection for it"
                .to_string(),
        );
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

    /* ---------------- model selection for DeepSeek Harness ------------- */

    /// The dump shape is what `dsh --dump-config` actually prints; the catalogue
    /// only appears when the profile declares one.
    const DUMP: &str = "\
# == @deepseek-ai/dsh-base
- id: timer
  name: '@deepseek-ai/cordis-plugin-timer'
- id: agent-default-model
  name: '@deepseek-ai/dsh-agent-default-model'
  config:
    provider: deepseek-official
    model: deepseek-flash
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek'
  config:
    models:
      - id: deepseek-v4-pro
        name: DeepSeek-V4-Pro
      - id: deepseek-flash
        name: DeepSeek-V41-Flash
- id: system-prompt
  name: '@deepseek-ai/dsh-system-prompt'
  config:
    personaPrefix: You are a coding agent powered by the {{model}} model.
    personaSuffix: Your working directory is {{cwd}}.
# == @deepseek-ai/dsh-headless
- id: code-runtime
  name: '@deepseek-ai/dsh-code-runtime-worker-thread'
";

    const DUMP_WITHOUT_CATALOGUE: &str = "\
- id: agent-default-model
  name: '@deepseek-ai/dsh-agent-default-model'
  config:
    provider: deepseek-official
    model: deepseek-flash
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek'
";

    #[test]
    fn a_profile_dump_yields_the_provider_and_its_catalogue() {
        let composition = parse_composition(DUMP);
        assert_eq!(composition.provider.as_deref(), Some("deepseek-official"));
        assert_eq!(composition.models.len(), 2);
        assert_eq!(composition.models[0].value, "deepseek-v4-pro");
        assert_eq!(
            composition.models[0].label.as_deref(),
            Some("DeepSeek-V4-Pro")
        );
        assert_eq!(composition.models[1].value, "deepseek-flash");
    }

    #[test]
    fn a_profile_without_a_catalogue_reports_none() {
        let composition = parse_composition(DUMP_WITHOUT_CATALOGUE);
        assert_eq!(composition.provider.as_deref(), Some("deepseek-official"));
        assert!(composition.models.is_empty());
    }

    /// The headless profile of the real CLI has no model and no reasoning flag, so
    /// Relay must never claim one — and must never offer what it cannot apply.
    fn headless_help() -> &'static str {
        "Usage: dsh --profile headless [options] [task...]\n\nArguments:\n  task        the task text\n\nOptions:\n  -h, --help  show this help\n"
    }

    fn fake_dsh(directory: &std::path::Path, dump: &str) -> String {
        let script = directory.join("dsh");
        let body = "#!/bin/sh\n\
             for arg in \"$@\"; do\n\
               if [ \"$arg\" = \"--dump-config\" ]; then cat \"$DSH_DUMP\"; exit 0; fi\n\
               if [ \"$arg\" = \"--help\" ]; then cat \"$DSH_HELP\"; exit 0; fi\n\
               if [ \"$arg\" = \"--version\" ]; then printf '0.1.5-rc.3\\n'; exit 0; fi\n\
             done\n\
             prev=\"\"\n\
             for arg in \"$@\"; do\n\
               if [ \"$prev\" = \"--patch\" ]; then\n\
                 echo \"PATCH-PATH $arg\" >> \"$DSH_RECORD\"\n\
                 cat \"$arg\" >> \"$DSH_RECORD\"\n\
               fi\n\
               prev=\"$arg\"\n\
             done\n\
             echo \"ARGV $*\" >> \"$DSH_RECORD\"\n\
             echo \"PERMISSION $DSH_PERMISSION_MODE\" >> \"$DSH_RECORD\"\n\
             printf '%s\\n' '{\"type\":\"session\",\"sessionId\":\"s-1\"}'\n\
             printf '%s\\n' '{\"type\":\"final\",\"text\":\"ok\"}'\n";
        std::fs::write(&script, body).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        std::fs::write(directory.join("dump.yml"), dump).unwrap();
        std::fs::write(directory.join("help.txt"), headless_help()).unwrap();
        script.display().to_string()
    }

    fn fixture_environment(
        directory: &std::path::Path,
        record: &std::path::Path,
    ) -> Vec<(String, String)> {
        vec![
            (
                "DSH_DUMP".to_string(),
                directory.join("dump.yml").display().to_string(),
            ),
            (
                "DSH_HELP".to_string(),
                directory.join("help.txt").display().to_string(),
            ),
            ("DSH_RECORD".to_string(), record.display().to_string()),
        ]
    }

    fn runtime_with(executable: &str) -> Runtime {
        Runtime {
            id: RUNTIME_ID.to_string(),
            adapter_id: ADAPTER_ID.to_string(),
            executable_path: executable.to_string(),
            version: None,
            health: RuntimeHealth::Available,
            capabilities: AdapterCapabilities::default(),
        }
    }

    fn start_input(executable: &str, model: Option<&str>) -> StartInput {
        StartInput {
            run_id: "run:1".to_string(),
            worker_session_id: "worker:1".to_string(),
            task: "do it".to_string(),
            cwd: std::env::temp_dir().display().to_string(),
            access_mode: relay_core::AccessMode::ReadOnly,
            executable_path: Some(executable.to_string()),
            model: model.map(str::to_string),
            reasoning: None,
            instructions: None,
        }
    }

    /// The registered executable is the one probed — never a second CLI that
    /// discovery happens to find first.
    #[tokio::test]
    async fn options_are_read_from_the_registered_executable() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        // The adapter's own discovery points somewhere useless; the runtime wins.
        let adapter = DeepSeekAdapter::with_executable("/nonexistent/dsh")
            .with_environment(fixture_environment(directory.path(), &record));

        let options = adapter.report_options(&runtime_with(&executable)).await;
        assert_eq!(options.runtime_id, RUNTIME_ID);
        assert_eq!(options.model_flag, None, "the headless CLI has no --model");
        assert_eq!(options.reasoning_flag, None);
        assert_eq!(options.models.len(), 2, "{:?}", options.diagnostics);
        assert_eq!(options.models[0].value, "deepseek-v4-pro");
        assert!(options
            .diagnostics
            .iter()
            .any(|line| line.contains("--patch overlay")));
        assert!(options
            .diagnostics
            .iter()
            .any(|line| line.contains("reasoning effort")));
    }

    /// A model the profile does not declare must never be launched: that is
    /// exactly the "picker that does nothing" Relay refuses to offer.
    #[tokio::test]
    async fn a_run_refuses_a_model_the_profile_cannot_apply() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let error = adapter
            .start(start_input(&executable, Some("not-a-model")))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "INVALID_REQUEST");
        assert!(error.message().contains("not-a-model"));
    }

    /// The whole point: a model that is offered is a model that is applied. The
    /// overlay reaches the child process, names the profile's own provider, and is
    /// gone once the run ends.
    #[tokio::test]
    async fn a_selected_model_reaches_the_child_process_and_is_cleaned_up() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let handle = adapter
            .start(start_input(&executable, Some("deepseek-v4-pro")))
            .await
            .unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(
            log.contains("--patch"),
            "the overlay never reached dsh: {log}"
        );
        assert!(log.contains("provider: \"deepseek-official\""), "{log}");
        assert!(log.contains("model: \"deepseek-v4-pro\""), "{log}");

        let patch_path = log
            .lines()
            .find_map(|line| line.strip_prefix("PATCH-PATH "))
            .expect("the run recorded the overlay path")
            .trim()
            .to_string();
        assert!(
            !std::path::Path::new(&patch_path).exists(),
            "the overlay outlived the run: {patch_path}"
        );
    }

    /// Instructions are the profile's whole point, so they must reach the child
    /// process — through the runtime's own system prompt, not appended to a task.
    #[tokio::test]
    async fn profile_instructions_reach_the_runtime_system_prompt() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut input = start_input(&executable, None);
        input.instructions = Some("You own the engineering work.".to_string());
        let handle = adapter.start(input).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("system-prompt"), "{log}");
        assert!(log.contains("You own the engineering work."), "{log}");
        // The profile's own persona survives: Relay restates what it composed
        // instead of replacing it.
        assert!(log.contains("{{cwd}}"), "{log}");
        assert!(log.contains("{{model}}"), "{log}");
        // The task itself is untouched: the instructions are not duplicated.
        assert!(!log.contains("[Relay agent profile instructions]"), "{log}");
    }

    /// Relay's access mode is a real boundary for this runtime: the sandbox row
    /// reads DSH_PERMISSION_MODE, so a read-only run really cannot write.
    #[tokio::test]
    async fn the_access_mode_reaches_the_runtime_sandbox() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut read_only = start_input(&executable, None);
        read_only.access_mode = relay_core::AccessMode::ReadOnly;
        let handle = adapter.start(read_only).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}
        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("PERMISSION read-only"), "{log}");
    }

    #[tokio::test]
    async fn a_write_run_gets_the_workspace_sandbox() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut write = start_input(&executable, None);
        write.access_mode = relay_core::AccessMode::Write;
        let handle = adapter.start(write).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}
        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("PERMISSION workspace-write"), "{log}");
    }

    /// An explicit deployment override is never overridden by Relay's mapping.
    #[test]
    fn an_explicit_permission_override_wins() {
        let adapter = DeepSeekAdapter::new().with_environment(vec![(
            "DSH_PERMISSION_MODE".to_string(),
            "danger-full-access".to_string(),
        )]);
        let environment = adapter.run_environment(relay_core::AccessMode::ReadOnly);
        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0].1, "danger-full-access");
    }

    /// A profile that declares no catalogue still applies what a profile carries.
    #[tokio::test]
    async fn a_profile_without_a_catalogue_still_applies_its_model() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP_WITHOUT_CATALOGUE);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let handle = adapter
            .start(start_input(&executable, Some("deepseek-flash")))
            .await
            .unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}
        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("model: \"deepseek-flash\""), "{log}");
    }
}
