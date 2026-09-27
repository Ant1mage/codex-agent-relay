//! End-to-end delegation over the real MCP stdio surface.
//!
//! This is the contract that must not break:
//!
//! ```text
//! Codex → MCP run_agent → Relay daemon starts the runtime CLI → structured events
//!   → worker/completed → wait_agent returns result.summary
//!   → accept_agent → Run completed
//! ```
//!
//! The daemon runs in-process here and the runtime CLI is a fixture, so the test
//! exercises Relay's own path (HTTP surface, controller, adapter, event log,
//! projection, MCP tool surface) end to end without needing `dsh` or Codex
//! installed. The MCP servers are real child processes, because the point of the
//! architecture is that they are replaceable.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use relay_api::server_info::ServerInfo;
use relay_api::{RelayServerOptions, RelayServerState, RelayStore};
use relay_codex::{CodexThreadMetadata, CodexThreadMetadataResolver};
use relay_config::ConfigStore;
use relay_core::{EventStore, HostSessionStore, RunController};
use relay_storage::{Database, SqliteEventStore, SqliteHostSessionStore};
use relayd::{DaemonService, RelayEngine, RuntimeConfigReloader};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// Codex's own app-server is not part of this test: the thread metadata comes
/// from the fixture that also stands in for the workspace.
struct FixtureThreads {
    cwd: PathBuf,
}

#[async_trait::async_trait]
impl CodexThreadMetadataResolver for FixtureThreads {
    async fn resolve(&self, thread_id: &str) -> relay_core::Result<CodexThreadMetadata> {
        Ok(CodexThreadMetadata {
            id: thread_id.to_string(),
            display_name: "Fixture session".to_string(),
            cwd: self.cwd.display().to_string(),
            model: Some("gpt-5".to_string()),
        })
    }
}

/// A Relay daemon: the real engine, the real HTTP surface, an ephemeral port.
struct Daemon {
    _server: tokio::task::JoinHandle<()>,
    info: ServerInfo,
}

impl Daemon {
    async fn start(home: &Path, workspace: &Path) -> Self {
        let database = Arc::new(Database::open(home.join("relay.sqlite")).unwrap());
        let events = Arc::new(SqliteEventStore::new(Arc::clone(&database)));
        let sessions = Arc::new(SqliteHostSessionStore::new(Arc::clone(&database)));
        let store = Arc::new(RelayStore::new(Arc::clone(&events), Arc::clone(&sessions)));

        let controller = Arc::new(RunController::new(
            Arc::clone(&events) as Arc<dyn EventStore>
        ));
        for adapter in relay_adapters::adapters() {
            controller.adapters.register(adapter).unwrap();
        }
        let config = ConfigStore::new(home.join("config.toml"));
        let reloader = Arc::new(RuntimeConfigReloader::new(
            Arc::clone(&controller),
            config.clone(),
        ));
        reloader.refresh().await.unwrap();
        relay_storage::reconcile_stale_runs(&events).unwrap();

        let engine = Arc::new(RelayEngine::new(
            Arc::clone(&controller),
            Arc::clone(&sessions) as Arc<dyn HostSessionStore>,
            Arc::new(FixtureThreads {
                cwd: workspace.to_path_buf(),
            }),
            Arc::clone(&reloader),
        ));
        let service = Arc::new(DaemonService::new(config).await);
        store.set_environment(service.environment());

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let info = ServerInfo {
            pid: std::process::id(),
            port,
            url: format!("http://127.0.0.1:{port}"),
            token: "test-token".to_string(),
            nonce: "test-nonce".to_string(),
            started_at: relay_core::now(),
            version: "0.2.0".to_string(),
            database: home.join("relay.sqlite").display().to_string(),
        };
        let state = RelayServerState::new(RelayServerOptions {
            store,
            service,
            runs: engine,
            codex: Arc::new(NoCodex),
            web_root: home.join("ui"),
            panel_root: home.join("ui"),
            token: info.token.clone(),
            port: Arc::new(std::sync::atomic::AtomicU16::new(port)),
            version: info.version.clone(),
            started_at: info.started_at.clone(),
            nonce: info.nonce.clone(),
            database_path: info.database.clone(),
        });
        let router = relay_api::router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        // The MCP server discovers the daemon the same way the tray does.
        relay_api::server_info::write_server_info(&home.join("server.json"), &info).unwrap();
        Self {
            _server: server,
            info,
        }
    }

    fn sqlite(&self) -> Arc<Database> {
        Arc::new(Database::open(&self.info.database).unwrap())
    }
}

struct NoCodex;

#[async_trait::async_trait]
impl relay_api::CodexIntegration for NoCodex {
    async fn status(&self) -> relay_api::CodexStatus {
        relay_api::CodexStatus::unknown()
    }
    async fn run(&self, _action: relay_api::CodexAction) -> relay_api::InstallResult {
        relay_api::InstallResult {
            status: relay_api::CodexStatus::unknown(),
            messages: Vec::new(),
        }
    }
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpClient {
    async fn start(home: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_relay-mcp"))
            .env("RELAY_HOME", home)
            .env("CODEX_THREAD_ID", "test-thread")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("relay-mcp must start");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        client.initialize().await;
        client
    }

    async fn initialize(&mut self) {
        let response = self
            .send(
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "relay-e2e", "version": "0.1.0" }
                })),
            )
            .await;
        assert_eq!(response["result"]["serverInfo"]["name"], "relay");
        self.notify("notifications/initialized").await;
    }

    async fn send(&mut self, method: &str, params: Option<serde_json::Value>) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(params) = params {
            request["params"] = params;
        }
        self.stdin
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        self.stdin.flush().await.unwrap();
        self.read_response(id).await
    }

    async fn notify(&mut self, method: &str) {
        let message = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        self.stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn read_response(&mut self, id: u64) -> serde_json::Value {
        loop {
            let mut line = String::new();
            let read =
                tokio::time::timeout(Duration::from_secs(60), self.stdout.read_line(&mut line))
                    .await;
            let count = read
                .expect("relay-mcp must answer in time")
                .expect("relay-mcp must stay alive");
            assert!(
                count > 0,
                "relay-mcp closed the stream while waiting for response {id}"
            );
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            if message.get("id").and_then(|value| value.as_u64()) == Some(id) {
                return message;
            }
        }
    }

    /// The successful payload of a tool call.
    async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let result = self.call_tool_raw(name, arguments).await;
        assert_ne!(
            result.get("isError").and_then(|value| value.as_bool()),
            Some(true),
            "{name} returned an error: {result}"
        );
        result["structuredContent"]["result"].clone()
    }

    /// The raw tool result, so a test can assert on a failure.
    async fn call_tool_raw(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        let response = self
            .send(
                "tools/call",
                Some(serde_json::json!({ "name": name, "arguments": arguments })),
            )
            .await;
        assert!(response.get("error").is_none(), "{name} failed: {response}");
        response["result"].clone()
    }

    async fn kill(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    async fn shutdown(self) {
        self.kill().await;
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn workspace_config(home: &Path, fixtures: &Path) {
    let dsh = fixtures.join("dsh");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"
[[runtimes]]
id = "runtime:fixture"
adapter_id = "deepseek-harness"
executable_path = "{dsh}"
label = "Fixture dsh"

[[agents]]
id = "agent-fixture"
name = "Fixture worker"
runtime_id = "runtime:fixture"
description = "Exercises the delegation path."
enabled = true

[agents.capabilities]
read_workspace = true
write_workspace = true
execute_commands = true
network_access = false
"#,
            dsh = dsh.display()
        ),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn codex_can_delegate_wait_and_accept_over_mcp() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    workspace_config(home.path(), &fixtures());
    let daemon = Daemon::start(home.path(), workspace.path()).await;

    let mut client = McpClient::start(home.path()).await;

    // The tool surface Codex sees.
    let tools = client.send("tools/list", None).await;
    let names: Vec<String> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "list_agents",
            "run_agent",
            "get_agent_status",
            "wait_agent",
            "send_agent",
            "cancel_agent",
            "accept_agent",
            "resume_agent",
            "sync_session",
            "end_session",
        ]
    );

    // The configured agent is visible, and the session is registered from the
    // Codex thread identity.
    let agents = client.call_tool("list_agents", serde_json::json!({})).await;
    assert_eq!(agents[0]["id"], "agent-fixture");
    assert_eq!(agents[0]["runtimeId"], "runtime:fixture");

    // Delegate.
    let started = client
        .call_tool(
            "run_agent",
            serde_json::json!({
                "agent_id": "agent-fixture",
                "task": "Prove the vertical slice",
                "access_mode": "write"
            }),
        )
        .await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    let worker_id = started["workerSessionId"].as_str().unwrap().to_string();
    assert_eq!(started["hostSessionDisplayName"], "Fixture session");

    // Wait for the worker, then read the summary Codex reviews.
    let waited = client
        .call_tool(
            "wait_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(waited["run"]["status"], "awaiting_host");
    assert_eq!(waited["result"]["summary"], "work complete");
    assert_eq!(waited["workers"][0]["status"], "completed");
    assert_eq!(waited["lastEvent"]["type"], "run/awaiting_host");

    // Review, then accept.
    let accepted = client
        .call_tool(
            "accept_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(accepted["run"]["status"], "completed");
    assert_eq!(accepted["steps"][0]["status"], "completed");

    client.shutdown().await;

    // The event log holds the whole delegation, in order.
    let store = SqliteEventStore::new(daemon.sqlite());
    let events = store.list(&run_id).unwrap();
    let types: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        vec![
            "run/created",
            "step/created",
            "worker/started",
            "worker/message",
            "worker/reasoning",
            "tool/read",
            "tool/result",
            "worker/message",
            "worker/message",
            "worker/message",
            "worker/completed",
            "run/awaiting_host",
            "run/accepted",
        ]
    );
    assert!(events.iter().all(|event| event.seq >= 1));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == relay_core::RelayEventType::RunAccepted)
            .count(),
        1
    );

    // The host session was registered with Codex's own name.
    let sessions = SqliteHostSessionStore::new(daemon.sqlite());
    let stored = sessions.list().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].display_name, "Fixture session");
    assert_eq!(stored[0].cwd, workspace.path().display().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_worker_reports_its_message_and_never_awaits_the_host() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    workspace_config(home.path(), &fixtures());
    let _daemon = Daemon::start(home.path(), workspace.path()).await;

    let mut client = McpClient::start(home.path()).await;
    let started = client
        .call_tool(
            "run_agent",
            serde_json::json!({ "agent_id": "agent-fixture", "task": "FAIL now" }),
        )
        .await;
    let worker_id = started["workerSessionId"].as_str().unwrap().to_string();

    let waited = client
        .call_tool(
            "wait_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(waited["run"]["status"], "failed");
    assert_eq!(waited["result"]["message"], "fixture failed");

    // A failed run cannot be accepted.
    let result = client
        .call_tool_raw(
            "accept_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(result["isError"], true);
    assert!(result["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("cannot be accepted"));

    client.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_worker_ends_the_run_as_cancelled() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    workspace_config(home.path(), &fixtures());
    let _daemon = Daemon::start(home.path(), workspace.path()).await;

    let mut client = McpClient::start(home.path()).await;
    let started = client
        .call_tool(
            "run_agent",
            serde_json::json!({ "agent_id": "agent-fixture", "task": "HANG until cancelled" }),
        )
        .await;
    let worker_id = started["workerSessionId"].as_str().unwrap().to_string();

    // Give the supervisor a moment to publish worker/started.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let cancelled = client
        .call_tool(
            "cancel_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(cancelled["cancelled"], true);

    let status = client
        .call_tool(
            "get_agent_status",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(status["run"]["status"], "cancelled");
    assert_eq!(status["lastEvent"]["type"], "worker/cancelled");

    client.shutdown().await;
}

/// The architecture's headline property: the process that owns a worker is the
/// daemon, so the MCP server that started the delegation can die without
/// touching it, and a new MCP server picks the same run up.
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_outlives_the_mcp_process_that_started_it() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    workspace_config(home.path(), &fixtures());
    let _daemon = Daemon::start(home.path(), workspace.path()).await;

    let mut first = McpClient::start(home.path()).await;
    let started = first
        .call_tool(
            "run_agent",
            serde_json::json!({ "agent_id": "agent-fixture", "task": "HANG until cancelled" }),
        )
        .await;
    let worker_id = started["workerSessionId"].as_str().unwrap().to_string();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Codex's MCP connection dies. Nothing about the worker may change.
    first.kill().await;

    let mut second = McpClient::start(home.path()).await;
    let status = second
        .call_tool(
            "get_agent_status",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(status["run"]["status"], "running");
    assert_eq!(status["workers"][0]["status"], "running");

    // The new front-end can finish the job the old one started.
    let cancelled = second
        .call_tool(
            "cancel_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(cancelled["cancelled"], true);
    let status = second
        .call_tool(
            "get_agent_status",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(status["run"]["status"], "cancelled");

    second.shutdown().await;
}

/// Two MCP servers, one daemon: both see the same run and neither can start a
/// duplicate or steal the other's cancellation.
#[tokio::test(flavor = "multi_thread")]
async fn two_mcp_servers_share_one_daemon_without_duplicating_work() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    workspace_config(home.path(), &fixtures());
    let _daemon = Daemon::start(home.path(), workspace.path()).await;

    let mut first = McpClient::start(home.path()).await;
    let mut second = McpClient::start(home.path()).await;

    let started = first
        .call_tool(
            "run_agent",
            serde_json::json!({ "agent_id": "agent-fixture", "task": "Prove the vertical slice" }),
        )
        .await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    let worker_id = started["workerSessionId"].as_str().unwrap().to_string();

    // The second server sees the first server's worker, and there is exactly one.
    let waited = second
        .call_tool(
            "wait_agent",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(waited["run"]["id"], run_id);
    assert_eq!(waited["run"]["status"], "awaiting_host");
    assert_eq!(waited["workers"].as_array().unwrap().len(), 1);

    // So does the first, after the second already waited on it.
    let status = first
        .call_tool(
            "get_agent_status",
            serde_json::json!({ "worker_session_id": worker_id }),
        )
        .await;
    assert_eq!(status["run"]["status"], "awaiting_host");

    first.shutdown().await;
    second.shutdown().await;
}

/// Without a daemon there is no execution runtime to fall back to: Relay says so
/// instead of quietly owning a second one.
#[tokio::test(flavor = "multi_thread")]
async fn an_mcp_server_without_a_daemon_reports_it_instead_of_starting_one() {
    let home = tempfile::tempdir().unwrap();
    let mut client = McpClient::start(home.path()).await;

    let result = client
        .call_tool_raw("list_agents", serde_json::json!({}))
        .await;
    assert_eq!(result["isError"], true);
    let message = result["content"][0]["text"].as_str().unwrap();
    assert!(
        message.contains("Relay daemon is not running"),
        "unexpected message: {message}"
    );

    client.shutdown().await;
}
