//! GLM / Z.ai (`zai-cli`).
//!
//! This CLI prints one JSON document at exit rather than a stream, so its parser
//! runs on the whole output.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, DetectionResult, EnforcementSet, RelayError,
    RelayEventType, Result, Runtime, RuntimeHealth, RuntimeOptions, StartInput, WorkerHandle,
};

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::probe::{
    discover_executable, probe_runtime_options, probe_target, read_help, version_of,
    with_selection_args, Selection,
};

pub const ADAPTER_ID: &str = "zai-cli";
pub const RUNTIME_ID: &str = "runtime:zai-cli";

fn find_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(object) => {
            for key in ["response", "content", "message", "text", "output"] {
                if let Some(serde_json::Value::String(text)) = object.get(key) {
                    return Some(text.clone());
                }
            }
            object.get("data").and_then(find_text)
        }
        _ => None,
    }
}

pub fn parse_output(output: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(output) else {
        parsed.events.push(AdapterEvent::with_native(
            RelayEventType::WorkerStatus,
            serde_json::json!({ "message": "GLM / Z.ai CLI returned non-JSON output" }),
            serde_json::Value::String(output.chars().take(32_768).collect()),
        ));
        parsed.final_text = Some(output.trim().to_string());
        return parsed;
    };

    if let Some(error) = raw.get("error") {
        parsed.error_message = match error {
            serde_json::Value::String(message) => Some(message.clone()),
            serde_json::Value::Object(object) => object
                .get("message")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            _ => None,
        };
    }
    if let Some(text) = find_text(&raw) {
        parsed.events.push(AdapterEvent::with_native(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "final", "text": text }),
            raw,
        ));
        parsed.final_text = Some(text);
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
            .unwrap_or_else(|| "GLM / Z.ai CLI exited unsuccessfully".to_string());
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

pub struct ZaiAdapter {
    configured_executable: Option<String>,
    prefix_args: Vec<String>,
    environment: Vec<(String, String)>,
    supervisor: Arc<ProcessSupervisor>,
    options: Mutex<Option<RuntimeOptions>>,
}

impl Default for ZaiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ZaiAdapter {
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
        discover_executable("zai-cli", &[]).map(|path| path.to_string_lossy().to_string())
    }
}

#[async_trait]
impl AgentAdapter for ZaiAdapter {
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
            enforcement: EnforcementSet::default(),
        }
    }

    async fn detect(&self) -> DetectionResult {
        let Some(executable) = self.executable() else {
            return DetectionResult {
                runtimes: Vec::new(),
                diagnostics: vec!["GLM / Z.ai CLI executable `zai-cli` was not found".to_string()],
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
                vec!["Authentication is validated by GLM / Z.ai CLI when a run starts".to_string()]
            } else {
                vec!["GLM / Z.ai CLI version check failed".to_string()]
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
        let executable = input
            .executable_path
            .clone()
            .or_else(|| self.executable())
            .ok_or_else(|| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    "GLM / Z.ai CLI executable `zai-cli` was not found",
                )
            })?;
        let evidence = read_help(&executable, &self.prefix_args).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options.clone());

        let base = [
            self.prefix_args.clone(),
            vec![
                "chat".to_string(),
                crate::instructions::enveloped(&input.task, input.instructions.as_deref()),
                "--output".to_string(),
                "json".to_string(),
                "--quiet".to_string(),
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
                stdin_gate: None,
                supervisor_key: input.worker_session_id.clone(),
                cleanup: None,
            },
            StreamMode::WholeOutput,
            Arc::new(parse_output),
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
    fn a_json_document_becomes_the_final_message() {
        let parsed = parse_output(r#"{"response":"the answer"}"#);
        assert_eq!(parsed.final_text.as_deref(), Some("the answer"));
        assert_eq!(parsed.events[0].event_type, RelayEventType::WorkerMessage);
    }

    #[test]
    fn nested_and_alternative_shapes_are_understood() {
        assert_eq!(
            parse_output(r#"{"data":{"content":"nested"}}"#)
                .final_text
                .as_deref(),
            Some("nested")
        );
        assert_eq!(
            parse_output(r#"{"output":"plain"}"#).final_text.as_deref(),
            Some("plain")
        );
    }

    #[test]
    fn an_error_document_reports_failure() {
        let parsed = parse_output(r#"{"error":{"message":"rate limited"},"response":"partial"}"#);
        assert_eq!(parsed.error_message.as_deref(), Some("rate limited"));
        assert_eq!(parsed.final_text.as_deref(), Some("partial"));
        let event = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            error_message: Some("rate limited".to_string()),
            ..StreamOutcome::default()
        });
        assert_eq!(event.event_type, RelayEventType::WorkerFailed);
    }

    /// GLM / Z.ai has no system-prompt flag, so the envelope is how a profile's
    /// instructions reach the child process. This asserts on the real argv.
    #[tokio::test]
    async fn profile_instructions_reach_the_child_process() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("args.txt");
        let script = directory.path().join("zai-cli");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nprintf '%s' '{{\"response\":\"ok\"}}'\n",
                record.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let adapter = ZaiAdapter::with_executable(script.display().to_string());
        let handle = adapter
            .start(StartInput {
                run_id: "run:1".to_string(),
                worker_session_id: "worker:1".to_string(),
                task: "do it".to_string(),
                cwd: std::env::temp_dir().display().to_string(),
                access_mode: relay_core::AccessMode::ReadOnly,
                executable_path: Some(script.display().to_string()),
                model: None,
                reasoning: None,
                instructions: Some("You own the engineering work.".to_string()),
            })
            .await
            .unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let args = std::fs::read_to_string(&record).unwrap();
        assert!(args.contains("You own the engineering work."), "{args}");
        assert!(
            args.contains("[Relay agent profile instructions]"),
            "{args}"
        );
        assert!(args.contains("do it"), "{args}");
    }

    #[test]
    fn non_json_output_is_kept_as_a_status_and_the_text_survives() {
        let parsed = parse_output("just text");
        assert_eq!(parsed.events[0].event_type, RelayEventType::WorkerStatus);
        assert_eq!(parsed.final_text.as_deref(), Some("just text"));
    }
}
