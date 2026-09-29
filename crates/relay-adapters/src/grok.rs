//! Grok Build's native headless CLI, using its existing login and session store.
//!
//! Native `text` chunks stream through Relay's unified worker-text contract
//! ([`relay_core::worker_text`]): each chunk is an assistant-text increment, one
//! message per model response, so every surface merges them the same way.

mod catalog;
mod environment;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, AssistantTextDelta, DetectionResult,
    EnforcementSet, OptionsSource, RelayError, RelayEventType, Result, ResumeInput, Runtime,
    RuntimeHealth, RuntimeOptions, StartInput, WorkerHandle,
};
use serde_json::{json, Value};

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::probe::{
    capture_with, discover_executable, probe_runtime_options, probe_target, read_help_with,
    version_of, with_selection_args, Selection,
};

pub const ADAPTER_ID: &str = "grok-cli";
pub const RUNTIME_ID: &str = "runtime:grok-cli";

#[derive(Default)]
struct StreamState {
    /// The merged text of the response currently streaming, kept for the
    /// terminal summary.
    text: String,
    /// Set when the previous response ended: the next chunk clears the merged
    /// text and opens a new assistant message.
    next_response: bool,
}

impl StreamState {
    fn parse(&mut self, line: &str) -> ParsedOutput {
        let mut parsed = ParsedOutput::default();
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            parsed.error_message = Some("Grok emitted malformed JSON".into());
            return parsed;
        };
        match raw.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = raw.get("data").and_then(Value::as_str) {
                    if self.next_response {
                        self.text.clear();
                        self.next_response = false;
                    }
                    // Chunks stream as they arrive; the shared aggregation merges
                    // them into one message per response.
                    let starts_message = self.text.is_empty();
                    self.text.push_str(text);
                    if !text.is_empty() {
                        parsed
                            .events
                            .push(AdapterEvent::text_delta(if starts_message {
                                AssistantTextDelta::message(text)
                            } else {
                                AssistantTextDelta::chunk(text)
                            }));
                    }
                }
            }
            Some("thought" | "thinking") => {
                parsed.events.push(AdapterEvent::with_native(
                    RelayEventType::WorkerReasoning,
                    json!({"text":raw.get("data")}),
                    raw,
                ));
            }
            Some("tool_call") => {
                let event_type = match raw.get("kind").and_then(Value::as_str) {
                    Some("read") => RelayEventType::ToolRead,
                    Some("search" | "fetch") => RelayEventType::ToolSearch,
                    Some("write" | "edit" | "delete" | "move") => RelayEventType::ToolEdit,
                    _ => RelayEventType::ToolCommand,
                };
                parsed.events.push(AdapterEvent::with_native(event_type, json!({"callId":raw.get("toolCallId"), "tool":raw.get("toolName").or_else(|| raw.get("title")), "arguments":raw.get("rawInput"), "status":raw.get("status"), "path":raw.pointer("/rawInput/path").or_else(|| raw.pointer("/rawInput/file_path")).or_else(|| raw.pointer("/rawInput/target_file")), "command":raw.pointer("/rawInput/command"), "query":raw.pointer("/rawInput/query")}), raw));
            }
            Some("tool_call_update") => {
                let terminal = matches!(
                    raw.get("status").and_then(Value::as_str),
                    Some("completed" | "failed")
                );
                parsed.events.push(AdapterEvent::with_native(if terminal { RelayEventType::ToolResult } else { RelayEventType::WorkerStatus }, json!({"callId":raw.get("toolCallId"), "status":raw.get("status"), "output":raw.get("rawOutput").or_else(|| raw.get("content")), "isError":raw.get("status").and_then(Value::as_str) == Some("failed")}), raw));
            }
            Some("usage") => {
                self.next_response = true;
                // Provider signatures authenticate usage upstream; they have no
                // role in Relay's event log and must not be copied into it.
                parsed.events.push(AdapterEvent::new(RelayEventType::WorkerStatus, json!({"kind":"usage", "usage":raw.get("usage"), "stopReason":raw.get("stopReason")})));
            }
            Some("end") => {
                parsed.session_id = raw
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                parsed.turn_end_kind = raw
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                // Every chunk was already emitted; the merged text is the
                // terminal summary, not a second copy of the answer.
                parsed.final_text = Some(self.text.clone());
                parsed.events.push(AdapterEvent::new(RelayEventType::WorkerStatus, json!({"kind":"result", "sessionId":parsed.session_id, "stopReason":parsed.turn_end_kind, "usage":raw.get("usage"), "numTurns":raw.get("num_turns"), "totalCostUsd":raw.get("total_cost_usd"), "usageIncomplete":raw.get("usage_is_incomplete")})));
            }
            Some("error") => {
                parsed.error_message = Some(
                    raw.get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Grok reported an error")
                        .chars()
                        .take(4096)
                        .collect(),
                );
            }
            // Command menus and compaction updates are not transcript messages.
            _ => {}
        }
        parsed
    }
}

fn terminal_event(outcome: &StreamOutcome) -> AdapterEvent {
    let reason = outcome.turn_end_kind.as_deref();
    let event_type = if reason == Some("cancelled")
        || matches!(
            outcome.signal,
            Some(libc::SIGTERM | libc::SIGINT | libc::SIGKILL)
        )
        || matches!(outcome.exit_code, Some(130 | 143))
    {
        RelayEventType::WorkerCancelled
    } else if outcome.exited_cleanly() && reason == Some("end_turn") {
        RelayEventType::WorkerCompleted
    } else {
        RelayEventType::WorkerFailed
    };
    let message = outcome
        .spawn_error
        .clone()
        .or_else(|| outcome.error_message.clone())
        .unwrap_or_else(|| match reason {
            Some(reason) => format!("Grok stopped with {reason}"),
            None => {
                "Grok exited without a successful end event; check the CLI login and connection"
                    .into()
            }
        });
    AdapterEvent::new(
        event_type,
        json!({
            "summary":outcome.final_text.clone().unwrap_or_default(),
            "message":if event_type == RelayEventType::WorkerFailed { Some(message) } else { None },
            "stopReason":reason, "exitCode":outcome.exit_code, "signal":outcome.signal,
        }),
    )
}

pub struct GrokAdapter {
    configured_executable: Option<String>,
    supervisor: Arc<ProcessSupervisor>,
}

impl Default for GrokAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GrokAdapter {
    pub fn new() -> Self {
        Self {
            configured_executable: None,
            supervisor: Arc::new(ProcessSupervisor::new()),
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
        if let Some(path) = &self.configured_executable {
            return Some(path.clone());
        }
        let extra = dirs::home_dir()
            .map(|home| vec![home.join(".grok/bin/grok"), home.join(".local/bin/grok")])
            .unwrap_or_default();
        discover_executable("grok", &extra).map(|path| path.to_string_lossy().into_owned())
    }

    async fn launch(&self, input: StartInput, resume: Option<String>) -> Result<WorkerHandle> {
        let executable = input
            .executable_path
            .clone()
            .or_else(|| self.executable())
            .ok_or_else(|| {
                RelayError::new("ADAPTER_FAILURE", "Grok executable `grok` was not found")
            })?;
        let env = environment::launch_environment().await;
        let evidence = read_help_with(&executable, &[], &env).await;
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        for required in [
            "--prompt-file",
            "--output-format",
            "--sandbox",
            "--permission-mode",
            "--session-id",
            "--resume",
            "--no-subagents",
            "--disable-web-search",
        ] {
            if !evidence.text.contains(required) {
                return Err(RelayError::new(
                    "ADAPTER_FAILURE",
                    format!("This Grok CLI does not advertise required flag {required}"),
                ));
            }
        }
        if input.model.is_some() && options.model_flag.is_none()
            || input.reasoning.is_some() && options.reasoning_flag.is_none()
        {
            return Err(RelayError::new(
                "ADAPTER_FAILURE",
                "This Grok CLI cannot apply the selected model or reasoning strength",
            ));
        }
        let native_id = resume
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if uuid::Uuid::parse_str(&native_id).is_err() {
            return Err(RelayError::new(
                "ADAPTER_FAILURE",
                "Grok resume requires a native session UUID",
            ));
        }
        // Owner-only prompt file keeps the task/instructions out of process args.
        let mut prompt = tempfile::Builder::new()
            .prefix("relay-grok-")
            .suffix(".txt")
            .tempfile()
            .map_err(|error| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    format!("Could not create Grok prompt: {error}"),
                )
            })?;
        prompt
            .write_all(
                crate::instructions::enveloped(&input.task, input.instructions.as_deref())
                    .as_bytes(),
            )
            .map_err(|error| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    format!("Could not write Grok prompt: {error}"),
                )
            })?;
        let (_, prompt_path) = prompt.keep().map_err(|error| {
            RelayError::new(
                "ADAPTER_FAILURE",
                format!("Could not retain Grok prompt: {error}"),
            )
        })?;
        let (sandbox, permission) = if input.access_mode.is_write() {
            ("workspace", "bypassPermissions")
        } else {
            ("read-only", "dontAsk")
        };
        let base = vec![
            "--cwd".into(),
            input.cwd.clone(),
            "--prompt-file".into(),
            prompt_path.to_string_lossy().into_owned(),
            "--output-format".into(),
            "streaming-json".into(),
            "--sandbox".into(),
            sandbox.into(),
            "--permission-mode".into(),
            permission.into(),
            "--no-subagents".into(),
            "--disable-web-search".into(),
            if resume.is_some() {
                "--resume".into()
            } else {
                "--session-id".into()
            },
            native_id.clone(),
        ];
        let args = with_selection_args(
            &base,
            &Selection {
                model: input.model,
                reasoning: input.reasoning,
            },
            &options,
        );
        let state = Mutex::new(StreamState::default());
        let result = run_cli(
            StreamSpec {
                executable,
                args,
                cwd: input.cwd,
                env,
                stdin: None,
                stdin_gate: None,
                // Preallocate the native UUID so cancel is available before Grok
                // emits its terminal sessionId, without waiting on a full channel.
                supervisor_key: native_id.clone(),
                cleanup: Some(prompt_path.clone()),
            },
            StreamMode::Lines,
            Arc::new(move |line| state.lock().unwrap().parse(line)),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            false,
        )
        .await;
        match result {
            Ok(mut handle) => {
                handle.native_session_id = Some(native_id);
                Ok(handle)
            }
            Err(error) => {
                let _ = std::fs::remove_file(prompt_path);
                Err(error)
            }
        }
    }
}

#[async_trait]
impl AgentAdapter for GrokAdapter {
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
            child_sessions: false,
            model_selection: Some(true),
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
                runtimes: vec![],
                diagnostics: vec!["Grok executable `grok` was not found".into()],
            };
        };
        let version = version_of(&executable, &[]).await;
        let env = environment::launch_environment().await;
        let evidence = read_help_with(&executable, &[], &env).await;
        let (capabilities, _) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        let available = version.is_some()
            && evidence.text.contains("--prompt-file")
            && evidence.text.contains("--sandbox");
        DetectionResult {
            runtimes: vec![Runtime {
                id: RUNTIME_ID.into(),
                adapter_id: ADAPTER_ID.into(),
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
                "Grok validates its existing CLI login when a run starts".into()
            } else {
                "Grok CLI version or headless capabilities could not be verified".into()
            }],
        }
    }

    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions {
        let (executable, mut diagnostics) = probe_target(runtime, || self.executable());
        let Some(executable) = executable else {
            return RuntimeOptions::empty(&runtime.id, ADAPTER_ID, diagnostics.join("; "));
        };
        let env = environment::launch_environment().await;
        let evidence = read_help_with(&executable, &[], &env).await;
        let (_, mut options) =
            probe_runtime_options(self.capabilities(), &evidence, &runtime.id, ADAPTER_ID);
        if options.model_flag.is_some() {
            let mut default_model = None;
            let captured = capture_with(
                &executable,
                &["models".into()],
                &env,
                Duration::from_secs(20),
            )
            .await;
            if let Some((0, stdout, stderr)) = captured {
                default_model = catalog::default_model(&stdout).map(str::to_owned);
                options.models = catalog::model_names(&stdout, &stderr);
            }
            if !options.models.is_empty() {
                options.source = OptionsSource::Cli;
                options
                    .diagnostics
                    .retain(|note| !note.contains("model names"));
                if options.reasoning_flag.is_some() {
                    let home = std::env::var_os("GROK_HOME")
                        .map(std::path::PathBuf::from)
                        .or_else(|| dirs::home_dir().map(|home| home.join(".grok")));
                    if let (Some(home), Some(version)) = (home, version_of(&executable, &[]).await)
                    {
                        catalog::enrich(
                            &mut options.models,
                            &home.join("models_cache.json"),
                            &version,
                        );
                    }
                    if let Some(model) = options
                        .models
                        .iter()
                        .find(|model| Some(&model.value) == default_model.as_ref())
                    {
                        options.levels = model.reasoning_levels.clone();
                    }
                    if options
                        .models
                        .iter()
                        .all(|model| !model.reasoning_levels.is_empty())
                    {
                        options
                            .diagnostics
                            .retain(|note| !note.contains("reasoning levels"));
                    }
                }
            } else {
                diagnostics.push("Grok could not confirm its live model catalogue; check the CLI login and connection".into());
            }
        }
        diagnostics.append(&mut options.diagnostics);
        options.diagnostics = diagnostics;
        options
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        self.launch(input, None).await
    }

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        self.launch(
            StartInput {
                run_id: input.run_id,
                worker_session_id: input.worker_session_id,
                task: input.task,
                cwd: input.cwd,
                access_mode: input.access_mode,
                executable_path: input.executable_path,
                model: input.model,
                reasoning: input.reasoning,
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

    fn fixture() -> (tempfile::TempDir, GrokAdapter, Arc<ProcessSupervisor>) {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("grok");
        std::fs::write(&executable, r#"#!/bin/sh
case "$1" in
  --help) echo '--prompt-file --output-format --sandbox --permission-mode --session-id --resume --no-subagents --disable-web-search --model --reasoning-effort'; exit 0;;
  --version) echo 'grok 1.0.41'; exit 0;;
  models) printf 'Default model: grok-new\nAvailable models:\n  * grok-new (default)\n'; exit 0;;
esac
printf '%s\n' "$@" > args.txt
printf '%s\n%s\n' "$NO_PROXY" "$no_proxy" > child-exclusions.txt
printf '%s' "$GROK_DISABLE_AUTOUPDATER" > child-autoupdate.txt
while [ "$#" -gt 0 ]; do
  case "$1" in
    --prompt-file) prompt="$2"; shift;;
    --session-id|--resume) session="$2"; shift;;
  esac
  shift
done
cat "$prompt" > prompt.copy
if [ -e slow ]; then exec sleep 60; fi
i=0
while [ "$i" -lt 300 ]; do
  echo '{"type":"tool_call_update","toolCallId":"c1","status":"in_progress"}'
  echo '{"type":"text","data":"x"}'
  i=$((i + 1))
done
printf '{"type":"end","sessionId":"%s","stopReason":"end_turn"}\n' "$session"
"#).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let supervisor = Arc::new(ProcessSupervisor::new());
        let adapter = GrokAdapter::with_executable(executable.to_string_lossy())
            .with_supervisor(Arc::clone(&supervisor));
        (directory, adapter, supervisor)
    }

    fn input(cwd: &std::path::Path) -> StartInput {
        StartInput {
            run_id: "run-test".into(),
            worker_session_id: "worker-test".into(),
            task: "PRIVATE_TASK_MARKER".into(),
            cwd: cwd.to_string_lossy().into_owned(),
            access_mode: AccessMode::ReadOnly,
            executable_path: None,
            model: Some("grok-new".into()),
            reasoning: Some("low".into()),
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
    async fn launch_keeps_prompts_private_applies_selection_and_resumes_without_rewind() {
        let (directory, adapter, supervisor) = fixture();
        let initial = input(directory.path());
        let parent_no_proxy = std::env::var_os("NO_PROXY");
        let parent_no_proxy_lower = std::env::var_os("no_proxy");
        let mut handle = adapter.start(initial.clone()).await.unwrap();
        let native_id = handle.native_session_id.clone().unwrap();
        assert!(uuid::Uuid::parse_str(&native_id).is_ok());
        let events = drain(&mut handle).await;
        let terminal = events.last().unwrap();
        assert_eq!(terminal.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(terminal.data["summary"], "x".repeat(300));
        let args = std::fs::read_to_string(directory.path().join("args.txt")).unwrap();
        assert!(!args.contains("PRIVATE"));
        assert!(args.contains("--model\ngrok-new\n--reasoning-effort\nlow"));
        assert!(args.contains("--sandbox\nread-only\n--permission-mode\ndontAsk"));
        let exclusions =
            std::fs::read_to_string(directory.path().join("child-exclusions.txt")).unwrap();
        let lines = exclusions.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], lines[1]);
        assert!(lines[0].split(',').any(|value| value == "127.0.0.1"));
        assert!(lines[0].split(',').any(|value| value == "::1"));
        assert_eq!(std::env::var_os("NO_PROXY"), parent_no_proxy);
        assert_eq!(std::env::var_os("no_proxy"), parent_no_proxy_lower);
        // The shared child environment keeps Grok's auto-updater switch.
        assert_eq!(
            std::fs::read_to_string(directory.path().join("child-autoupdate.txt")).unwrap(),
            "1"
        );
        let prompt_path = args
            .lines()
            .skip_while(|arg| *arg != "--prompt-file")
            .nth(1)
            .unwrap();
        assert!(!std::path::Path::new(prompt_path).exists());
        let prompt = std::fs::read_to_string(directory.path().join("prompt.copy")).unwrap();
        assert!(
            prompt.contains("PRIVATE_TASK_MARKER")
                && prompt.contains("PRIVATE_INSTRUCTIONS_MARKER")
        );
        assert!(supervisor.tracked_keys().is_empty());

        let mut handle = adapter
            .resume(ResumeInput {
                run_id: initial.run_id,
                worker_session_id: initial.worker_session_id,
                native_session_id: native_id.clone(),
                task: "follow up".into(),
                cwd: initial.cwd,
                access_mode: AccessMode::Write,
                executable_path: None,
                model: initial.model,
                reasoning: initial.reasoning,
                instructions: None,
            })
            .await
            .unwrap();
        assert_eq!(
            handle.native_session_id.as_deref(),
            Some(native_id.as_str())
        );
        assert_eq!(
            drain(&mut handle).await.last().unwrap().event_type,
            RelayEventType::WorkerCompleted
        );
        let args = std::fs::read_to_string(directory.path().join("args.txt")).unwrap();
        assert!(args.contains(&format!("--resume\n{native_id}")));
        assert!(args.contains("--sandbox\nworkspace\n--permission-mode\nbypassPermissions"));
        assert!(!args.contains("--session-id") && !args.contains("--restore-code"));
    }

    #[tokio::test]
    async fn native_id_cancels_before_first_output_and_cleans_up() {
        let (directory, adapter, supervisor) = fixture();
        std::fs::write(directory.path().join("slow"), "").unwrap();
        let mut handle = adapter.start(input(directory.path())).await.unwrap();
        let native_id = handle.native_session_id.clone().unwrap();
        assert!(supervisor.is_tracked(&native_id));
        adapter.cancel(&native_id).await.unwrap();
        assert_eq!(
            drain(&mut handle).await.last().unwrap().event_type,
            RelayEventType::WorkerCancelled
        );
        assert!(supervisor.tracked_keys().is_empty());
    }

    #[test]
    fn chunks_stream_as_deltas_and_usage_separates_model_responses() {
        let mut state = StreamState::default();
        let first = state.parse(r#"{"type":"text","data":"I will read."}"#);
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(first.events[0].data["kind"], "delta");
        assert_eq!(first.events[0].data["text"], "I will read.");
        assert_eq!(first.events[0].data["messageStart"], true);

        let usage = state.parse(
            r#"{"type":"usage","usage":{"output_tokens":3},"signature":"PRIVATE_SIGNATURE"}"#,
        );
        assert!(!format!("{:?}", usage.events).contains("PRIVATE_SIGNATURE"));

        let opening = state.parse(r#"{"type":"text","data":"RELAY_"}"#);
        assert_eq!(
            opening.events[0].data["messageStart"], true,
            "a new model response opens a new assistant message"
        );
        let continuation = state.parse(r#"{"type":"text","data":"GROK_OK"}"#);
        assert!(
            continuation.events[0].data.get("messageStart").is_none(),
            "later chunks extend that message"
        );

        let end = state.parse(r#"{"type":"end","sessionId":"s1","stopReason":"end_turn"}"#);
        assert_eq!(end.final_text.as_deref(), Some("RELAY_GROK_OK"));
        assert_eq!(end.session_id.as_deref(), Some("s1"));
        assert_eq!(end.turn_end_kind.as_deref(), Some("end_turn"));
        assert_eq!(end.events[0].event_type, RelayEventType::WorkerStatus);
        assert_eq!(end.events[0].data["kind"], "result");
    }

    /// The native chunks reach the shared assistant-message path: one response
    /// is one message, and its terminal summary is reported as a repeat rather
    /// than displayed again.
    #[test]
    fn text_chunks_reach_the_shared_assistant_message_path() {
        let mut state = StreamState::default();
        let mut seq = 0;
        let mut events = Vec::new();
        for frame in [
            r#"{"type":"text","data":"RELAY_"}"#,
            r#"{"type":"text","data":"GROK_OK"}"#,
            r#"{"type":"end","sessionId":"s1","stopReason":"end_turn"}"#,
        ] {
            for event in state.parse(frame).events {
                seq += 1;
                events.push(crate::test_support::relay_event(seq, event));
            }
        }
        assert_eq!(
            crate::test_support::assistant_messages(&events),
            vec!["RELAY_GROK_OK".to_string()]
        );
    }

    #[tokio::test]
    async fn options_use_the_registered_executable() {
        let (directory, _, _) = fixture();
        let adapter = GrokAdapter::with_executable("/missing/other-grok");
        let runtime = Runtime {
            id: "manual:grok".into(),
            adapter_id: ADAPTER_ID.into(),
            executable_path: directory.path().join("grok").to_string_lossy().into_owned(),
            version: None,
            health: RuntimeHealth::Available,
            capabilities: adapter.capabilities(),
        };
        let options = adapter.report_options(&runtime).await;
        assert_eq!(options.runtime_id, "manual:grok");
        assert_eq!(options.source, OptionsSource::Cli);
        assert_eq!(options.models[0].value, "grok-new");
        assert!(!options
            .diagnostics
            .iter()
            .any(|note| note.contains("model names")));
    }

    #[test]
    fn native_write_kind_and_real_file_arguments_reach_the_changes_view() {
        let mut state = StreamState::default();
        let read = state.parse(r#"{"type":"tool_call","toolCallId":"r","kind":"read","toolName":"read_file","rawInput":{"target_file":"fixture.txt"}}"#);
        assert_eq!(read.events[0].data["path"], "fixture.txt");
        let write = state.parse(r#"{"type":"tool_call","toolCallId":"w","kind":"write","toolName":"write","rawInput":{"file_path":"result.txt","content":"OK"}}"#);
        assert_eq!(write.events[0].event_type, RelayEventType::ToolEdit);
        assert_eq!(write.events[0].data["path"], "result.txt");
    }

    #[test]
    fn native_read_and_failed_tool_result_are_normalized() {
        let mut state = StreamState::default();
        let call = state.parse(r#"{"type":"tool_call","toolCallId":"c1","kind":"read","toolName":"read_file","rawInput":{"path":"fixture.txt"}}"#);
        assert_eq!(call.events[0].event_type, RelayEventType::ToolRead);
        assert_eq!(call.events[0].data["arguments"]["path"], "fixture.txt");
        let result = state.parse(r#"{"type":"tool_call_update","toolCallId":"c1","status":"failed","rawOutput":"denied"}"#);
        assert_eq!(result.events[0].event_type, RelayEventType::ToolResult);
        assert_eq!(result.events[0].data["isError"], true);
        assert!(result.error_message.is_none());
    }

    #[test]
    fn success_requires_clean_exit_and_native_end_turn() {
        let mut outcome = StreamOutcome {
            exit_code: Some(0),
            final_text: Some("done".into()),
            ..Default::default()
        };
        assert_eq!(
            terminal_event(&outcome).event_type,
            RelayEventType::WorkerFailed
        );
        for reason in ["max_tokens", "max_turn_requests", "refusal"] {
            outcome.turn_end_kind = Some(reason.into());
            assert_eq!(
                terminal_event(&outcome).event_type,
                RelayEventType::WorkerFailed
            );
        }
        outcome.turn_end_kind = Some("end_turn".into());
        outcome.stderr_tail = "tokenization warning 402".into();
        assert_eq!(
            terminal_event(&outcome).event_type,
            RelayEventType::WorkerCompleted
        );
        outcome.error_message = Some("API failed".into());
        assert_eq!(
            terminal_event(&outcome).event_type,
            RelayEventType::WorkerFailed
        );
        outcome.signal = Some(libc::SIGTERM);
        assert_eq!(
            terminal_event(&outcome).event_type,
            RelayEventType::WorkerCancelled
        );
    }

    #[test]
    fn malformed_output_fails_without_forwarding_raw_data() {
        let mut state = StreamState::default();
        let parsed = state.parse("PRIVATE secret invalid JSON");
        assert_eq!(
            parsed.error_message.as_deref(),
            Some("Grok emitted malformed JSON")
        );
        assert!(parsed.events.is_empty());
    }
}
