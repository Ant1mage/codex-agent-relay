//! Shared CLI plumbing.
//!
//! Every runtime is an opaque executable: Relay spawns it with a cwd, an
//! environment and a stdin payload, reads what it prints, and kills it when
//! asked. What the CLI is written in (Node, Rust, Python, Go) is not Relay's
//! business, and none of it leaks past this module.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_core::{AdapterEvent, RelayError, Result, WorkerHandle};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

/// Everything that decides how a worker process is launched.
#[derive(Debug, Clone)]
pub struct StreamSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    /// Written to the child's stdin, then stdin is closed. Keeps the delegated
    /// task out of the process list.
    pub stdin: Option<String>,
    /// Key the process is registered under so `cancel` can find it.
    pub supervisor_key: String,
    /// A file this launch created that must not outlive the process — a per-run
    /// configuration overlay, for instance. Removed once the child is reaped.
    pub cleanup: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamMode {
    /// One JSON (or text) document per line — `dsh --json`, Kimi stream-json.
    Lines,
    /// A single JSON document printed at exit — `zai-cli --output json`.
    WholeOutput,
}

/// What one parsed line (or the whole output) contributes.
#[derive(Debug, Default, Clone)]
pub struct ParsedOutput {
    pub events: Vec<AdapterEvent>,
    pub session_id: Option<String>,
    pub final_text: Option<String>,
    pub error_message: Option<String>,
    pub turn_end_kind: Option<String>,
    pub result_status: Option<String>,
}

/// Everything observed about a finished process, for the terminal event.
#[derive(Debug, Default, Clone)]
pub struct StreamOutcome {
    pub session_id: Option<String>,
    pub final_text: Option<String>,
    pub error_message: Option<String>,
    pub turn_end_kind: Option<String>,
    pub result_status: Option<String>,
    pub stderr_tail: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    /// Set when the process could not be spawned or read at all.
    pub spawn_error: Option<String>,
}

impl StreamOutcome {
    pub fn exited_cleanly(&self) -> bool {
        self.exit_code == Some(0) && self.spawn_error.is_none() && self.error_message.is_none()
    }
}

pub type ParseFn = Arc<dyn Fn(&str) -> ParsedOutput + Send + Sync>;
pub type TerminalFn = Arc<dyn Fn(&StreamOutcome) -> AdapterEvent + Send + Sync>;

/// Tracks the processes this process started, so `cancel` can reach them.
///
/// One child is one registration, however many names it answers to: its worker
/// session id from the moment it is spawned, plus its native session id once the
/// CLI reports one. Because the aliases belong to the same registration, an
/// exited child is forgotten completely — a leftover alias would let a later
/// `terminate_all` signal a pid the OS has already handed to another process.
#[derive(Default)]
pub struct ProcessSupervisor {
    inner: Mutex<Registrations>,
}

#[derive(Default)]
struct Registrations {
    /// A worker session id or a native session id, mapped to its pid.
    by_key: HashMap<String, i32>,
    /// Every key that names a pid, so one exit clears all of them.
    by_pid: HashMap<i32, Vec<String>>,
}

impl ProcessSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, key: &str, pid: i32) {
        let mut inner = self.inner.lock().unwrap();
        inner.by_key.insert(key.to_string(), pid);
        let keys = inner.by_pid.entry(pid).or_default();
        if !keys.iter().any(|existing| existing == key) {
            keys.push(key.to_string());
        }
    }

    /// Registers the same process under a second key (its native session id),
    /// so cancellation reaches it under either name.
    fn rebind(&self, from: &str, to: &str) {
        let pid = self.inner.lock().unwrap().by_key.get(from).copied();
        if let Some(pid) = pid {
            self.register(to, pid);
        }
    }

    /// Forgets a process and every alias it had. Called when the child exits.
    fn forget_process(&self, pid: i32) {
        let mut inner = self.inner.lock().unwrap();
        let Some(keys) = inner.by_pid.remove(&pid) else {
            return;
        };
        for key in keys {
            inner.by_key.remove(&key);
        }
    }

    /// SIGTERM to the worker's process group, and only while that process is
    /// still there: a pid Relay no longer tracks is never signalled.
    pub fn terminate(&self, key: &str) -> bool {
        let pid = self.inner.lock().unwrap().by_key.get(key).copied();
        let Some(pid) = pid else {
            return false;
        };
        if !process_alive(pid) {
            self.forget_process(pid);
            return false;
        }
        terminate_pid(pid);
        true
    }

    pub fn terminate_all(&self) {
        let pids: Vec<i32> = self.inner.lock().unwrap().by_pid.keys().copied().collect();
        for pid in pids {
            if process_alive(pid) {
                terminate_pid(pid);
            }
            self.forget_process(pid);
        }
    }

    pub fn is_tracked(&self, key: &str) -> bool {
        self.inner.lock().unwrap().by_key.contains_key(key)
    }

    /// Every name Relay currently believes belongs to a live worker. Sorted, so a
    /// test can assert on the whole set.
    pub fn tracked_keys(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let mut keys: Vec<String> = inner.by_key.keys().cloned().collect();
        keys.sort();
        keys
    }
}

fn process_alive(pid: i32) -> bool {
    // Signal 0 only performs the permission/existence check.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn terminate_pid(pid: i32) {
    // The child runs in its own process group, so the negative pid reaches the
    // CLI and anything it spawned.
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
}

/// Starts one worker process and returns its handle.
///
/// `parse` turns native output into Relay events; `terminal` turns the outcome
/// into the single event that ends the worker.
pub async fn run_cli(
    spec: StreamSpec,
    mode: StreamMode,
    parse: ParseFn,
    terminal: TerminalFn,
    supervisor: Arc<ProcessSupervisor>,
    require_session: bool,
) -> Result<WorkerHandle> {
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    // Own process group: cancellation reaches the CLI's own children too.
    command.process_group(0);

    let mut child: Child = command.spawn().map_err(|error| {
        RelayError::new("ADAPTER_FAILURE", format!("{}: {error}", spec.executable))
    })?;
    let pid = child.id().map(|id| id as i32);
    if let Some(pid) = pid {
        supervisor.register(&spec.supervisor_key, pid);
    }
    // Read what the kernel says about the process now, while it is certainly the
    // one just spawned: this is the identity a later daemon verifies before it
    // ever signals the pid (see `relay_core::process`).
    let process = pid.and_then(|pid| relay_core::process::capture(pid as u32));

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RelayError::new("ADAPTER_FAILURE", "worker stdout is unavailable"))?;
    let stderr = child.stderr.take();

    if let Some(payload) = spec.stdin.clone() {
        if let Some(mut stdin) = child.stdin.take() {
            let write = async move {
                let _ = stdin.write_all(payload.as_bytes()).await;
                let _ = stdin.shutdown().await;
            };
            tokio::spawn(write);
        }
    } else {
        drop(child.stdin.take());
    }

    let (sender, receiver) = mpsc::channel::<AdapterEvent>(256);
    let (ready_sender, ready_receiver) = oneshot::channel::<String>();
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = stderr {
        let tail = Arc::clone(&stderr_tail);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                append_tail(&tail, &line);
            }
        });
    }

    let supervisor_for_task = Arc::clone(&supervisor);
    let cleanup = spec.cleanup.clone();
    let key = spec.supervisor_key.clone();
    let parse_for_task = Arc::clone(&parse);
    let terminal_for_task = Arc::clone(&terminal);
    let session_holder: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let session_for_task = Arc::clone(&session_holder);

    // The child is reaped in its own task. Reading its output and waiting for it
    // must not be the same await: a pipe that has not been drained yet can keep a
    // finished process unreaped, and a reader that waits for EOF would then wait
    // forever.
    let (exit_sender, mut exit_receiver) =
        oneshot::channel::<std::io::Result<std::process::ExitStatus>>();
    tokio::spawn(async move {
        let status = child.wait().await;
        let _ = exit_sender.send(status);
    });

    tokio::spawn(async move {
        let mut outcome = StreamOutcome::default();
        let mut ready_sender = Some(ready_sender);
        let reader = BufReader::new(stdout);
        let mut exited = false;
        let mut exit_status: Option<std::process::ExitStatus> = None;

        match mode {
            StreamMode::Lines => {
                let mut lines = reader.lines();
                loop {
                    let next = if exited {
                        // The process is gone: take whatever is already buffered,
                        // then stop instead of waiting for an EOF that may never be
                        // observed.
                        match tokio::time::timeout(Duration::from_millis(250), lines.next_line())
                            .await
                        {
                            Ok(result) => result,
                            Err(_) => break,
                        }
                    } else {
                        tokio::select! {
                            result = lines.next_line() => result,
                            status = &mut exit_receiver => {
                                exit_status = status.ok().and_then(|result| result.ok());
                                exited = true;
                                continue;
                            }
                        }
                    };
                    match next {
                        Ok(Some(line)) => {
                            let parsed = parse_for_task(&line);
                            absorb(
                                &mut outcome,
                                &parsed,
                                &session_for_task,
                                &mut ready_sender,
                                &supervisor_for_task,
                                &key,
                            );
                            for event in parsed.events {
                                if sender.send(event).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(_) if exited => break,
                        Err(error) => {
                            outcome.spawn_error = Some(error.to_string());
                            break;
                        }
                    }
                }
            }
            StreamMode::WholeOutput => {
                let mut buffer = String::new();
                let mut reader = reader;
                if let Err(error) = reader.read_to_string(&mut buffer).await {
                    outcome.spawn_error = Some(error.to_string());
                }
                let parsed = parse_for_task(&buffer);
                absorb(
                    &mut outcome,
                    &parsed,
                    &session_for_task,
                    &mut ready_sender,
                    &supervisor_for_task,
                    &key,
                );
                for event in parsed.events {
                    if sender.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }

        // Whatever happened above, the exit status is known by now: either the
        // select saw it, or the process ended while draining.
        if exit_status.is_none() {
            if exited {
                // Already consumed by the select; nothing more to read.
            } else if let Ok(status) = exit_receiver.await {
                exit_status = status.ok();
            }
        }
        match exit_status {
            Some(status) => {
                outcome.exit_code = status.code();
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    outcome.signal = status.signal();
                }
            }
            None => {
                if outcome.spawn_error.is_none() {
                    outcome.spawn_error = Some("worker exit status is unavailable".to_string());
                }
            }
        }
        outcome.stderr_tail = stderr_tail.lock().unwrap().clone();
        if outcome.session_id.is_none() {
            outcome.session_id = session_for_task.lock().unwrap().clone();
        }

        // The child has been reaped: drop every alias before the result is
        // announced, so no stale pid can outlive this process.
        if let Some(pid) = pid {
            supervisor_for_task.forget_process(pid);
        }
        if let Some(path) = &cleanup {
            let _ = std::fs::remove_file(path);
        }
        let _ = sender.send(terminal_for_task(&outcome)).await;
        drop(sender);
    });

    let native_session_id = if require_session {
        match ready_receiver.await {
            Ok(session_id) => Some(session_id),
            Err(_) => {
                return Err(RelayError::new(
                    "ADAPTER_FAILURE",
                    "worker exited before it reported a session",
                ))
            }
        }
    } else {
        None
    };

    Ok(WorkerHandle {
        native_session_id,
        process_id: pid.map(|pid| pid as u32),
        process,
        events: receiver,
    })
}

#[allow(clippy::too_many_arguments)]
fn absorb(
    outcome: &mut StreamOutcome,
    parsed: &ParsedOutput,
    session_holder: &Arc<Mutex<Option<String>>>,
    ready_sender: &mut Option<oneshot::Sender<String>>,
    supervisor: &Arc<ProcessSupervisor>,
    key: &str,
) {
    if let Some(session_id) = &parsed.session_id {
        if outcome.session_id.is_none() {
            outcome.session_id = Some(session_id.clone());
            *session_holder.lock().unwrap() = Some(session_id.clone());
            // Cancellation can arrive under either id.
            supervisor.rebind(key, session_id);
            if let Some(sender) = ready_sender.take() {
                let _ = sender.send(session_id.clone());
            }
        }
    }
    if let Some(text) = &parsed.final_text {
        outcome.final_text = Some(text.clone());
    }
    if let Some(message) = &parsed.error_message {
        outcome.error_message = Some(message.clone());
    }
    if let Some(kind) = &parsed.turn_end_kind {
        outcome.turn_end_kind = Some(kind.clone());
    }
    if let Some(status) = &parsed.result_status {
        outcome.result_status = Some(status.clone());
    }
}

fn append_tail(tail: &Arc<Mutex<String>>, line: &str) {
    const MAX: usize = 8_192;
    let mut guard = tail.lock().unwrap();
    guard.push_str(line);
    guard.push('\n');
    if guard.len() > MAX {
        let mut cut = guard.len() - MAX;
        while cut < guard.len() && !guard.is_char_boundary(cut) {
            cut += 1;
        }
        *guard = guard[cut..].to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_core::RelayEventType;

    fn parse_line(line: &str) -> ParsedOutput {
        let value: serde_json::Value =
            serde_json::from_str(line).unwrap_or(serde_json::Value::Null);
        let mut parsed = ParsedOutput::default();
        match value.get("type").and_then(|value| value.as_str()) {
            Some("session") => {
                parsed.session_id = value.get("id").and_then(|v| v.as_str()).map(str::to_string)
            }
            Some("text") => {
                let text = value
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                parsed.final_text = Some(text.clone());
                parsed.events.push(AdapterEvent::new(
                    RelayEventType::WorkerMessage,
                    serde_json::json!({ "text": text }),
                ));
            }
            _ => {}
        }
        parsed
    }

    fn terminal(outcome: &StreamOutcome) -> AdapterEvent {
        if outcome.exited_cleanly() {
            AdapterEvent::new(
                RelayEventType::WorkerCompleted,
                serde_json::json!({ "summary": outcome.final_text.clone().unwrap_or_default() }),
            )
        } else {
            AdapterEvent::new(
                RelayEventType::WorkerFailed,
                serde_json::json!({ "message": outcome.stderr_tail.clone() }),
            )
        }
    }

    fn spec(script: &str) -> StreamSpec {
        StreamSpec {
            executable: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            cwd: "/tmp".to_string(),
            env: Vec::new(),
            stdin: None,
            supervisor_key: "worker:test".to_string(),
            cleanup: None,
        }
    }

    #[tokio::test]
    async fn a_streaming_worker_reports_its_session_and_events() {
        let supervisor = Arc::new(ProcessSupervisor::new());
        let handle = run_cli(
            spec("printf '%s\\n' '{\"type\":\"session\",\"id\":\"s-1\"}' '{\"type\":\"text\",\"text\":\"hello\"}'"),
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal),
            supervisor,
            true,
        )
        .await
        .unwrap();
        assert_eq!(handle.native_session_id.as_deref(), Some("s-1"));

        let mut receiver = handle.events;
        let mut seen = Vec::new();
        while let Some(event) = receiver.recv().await {
            seen.push(event);
        }
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(seen[1].event_type, RelayEventType::WorkerCompleted);
        assert_eq!(seen[1].data["summary"], "hello");
    }

    #[tokio::test]
    async fn a_failing_worker_reports_failure_with_stderr() {
        let supervisor = Arc::new(ProcessSupervisor::new());
        let handle = run_cli(
            spec("echo 'boom' >&2; exit 3"),
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal),
            supervisor,
            false,
        )
        .await
        .unwrap();
        let mut receiver = handle.events;
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.event_type, RelayEventType::WorkerFailed);
        assert!(event.data["message"].as_str().unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn whole_output_mode_parses_once_at_exit() {
        let supervisor = Arc::new(ProcessSupervisor::new());
        let handle = run_cli(
            spec("printf '{\"type\":\"text\",\"text\":\"final answer\"}'"),
            StreamMode::WholeOutput,
            Arc::new(parse_line),
            Arc::new(terminal),
            supervisor,
            false,
        )
        .await
        .unwrap();
        let mut receiver = handle.events;
        let first = receiver.recv().await.unwrap();
        assert_eq!(first.event_type, RelayEventType::WorkerMessage);
        let second = receiver.recv().await.unwrap();
        assert_eq!(second.data["summary"], "final answer");
    }

    #[tokio::test]
    async fn cancellation_terminates_a_running_worker() {
        let supervisor = Arc::new(ProcessSupervisor::new());
        let handle = run_cli(
            spec("sleep 30"),
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal),
            Arc::clone(&supervisor),
            false,
        )
        .await
        .unwrap();
        let pid = handle.process_id.unwrap() as i32;
        assert!(supervisor.is_tracked("worker:test"));
        assert!(supervisor.terminate("worker:test"));

        let mut receiver = handle.events;
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .expect("worker should stop")
            .unwrap();
        assert_eq!(event.event_type, RelayEventType::WorkerFailed);
        // The process is gone.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    }

    /// The old bug: the worker key was dropped on exit but the native session
    /// alias was not, so a later terminate_all could signal a reused pid.
    #[tokio::test]
    async fn every_alias_of_a_process_is_dropped_when_it_exits() {
        let supervisor = Arc::new(ProcessSupervisor::new());
        // The child reports its session and then stays alive, so both of its
        // names are observable while it is really running: a child that exits at
        // once can be reaped before the test ever looks.
        let handle = run_cli(
            spec("printf '%s\n' '{\"type\":\"session\",\"id\":\"s-alias\"}'; sleep 30"),
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal),
            Arc::clone(&supervisor),
            true,
        )
        .await
        .unwrap();
        assert_eq!(handle.native_session_id.as_deref(), Some("s-alias"));
        // Both names point at the same registration while it runs.
        assert_eq!(
            supervisor.tracked_keys(),
            vec!["s-alias".to_string(), "worker:test".to_string()]
        );
        let pid = handle.process_id.unwrap() as i32;
        assert!(supervisor.terminate("worker:test"));

        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        // The exit cleared both keys ...
        assert!(supervisor.tracked_keys().is_empty());
        assert!(!supervisor.is_tracked("worker:test"));
        assert!(!supervisor.is_tracked("s-alias"));
        // ... and neither terminate nor terminate_all can reach the old pid.
        assert!(!supervisor.terminate("worker:test"));
        assert!(!supervisor.terminate("s-alias"));
        supervisor.terminate_all();
        assert!(supervisor.tracked_keys().is_empty());
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    }

    /// terminate only ever touches a process Relay still tracks.
    #[tokio::test]
    async fn terminate_ignores_a_process_that_already_exited() {
        let supervisor = Arc::new(ProcessSupervisor::new());
        let handle = run_cli(
            spec("exit 0"),
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal),
            Arc::clone(&supervisor),
            false,
        )
        .await
        .unwrap();
        let pid = handle.process_id.unwrap() as i32;
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        assert!(!supervisor.is_tracked("worker:test"));
        // A zombie would still answer signal 0; the child is reaped by now.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert!(!supervisor.terminate("worker:test"));
    }
}
