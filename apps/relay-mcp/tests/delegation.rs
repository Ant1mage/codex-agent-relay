//! End-to-end delegation over the real MCP stdio surface.
//!
//! This is the contract that must not break:
//!
//! ```text
//! MCP run_agent → Relay spawns the runtime CLI → structured events
//!   → worker/completed → wait_agent returns result.summary
//!   → accept_agent → Run completed
//! ```
//!
//! The runtime CLI and the Codex app-server are fixtures, so the test exercises
//! Relay's own path (adapter, event log, projection, tool surface) end to end
//! without needing `dsh` or Codex installed.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpClient {
    async fn start(home: &Path, fixtures: &Path, workspace: &Path) -> Self {
        let path = std::env::var("PATH").unwrap_or_default();
        let mut child = Command::new(env!("CARGO_BIN_EXE_relay-mcp"))
            .env("RELAY_HOME", home)
            .env("RELAY_DB_PATH", home.join("relay.sqlite"))
            .env("RELAY_CONFIG_PATH", home.join("config.toml"))
            .env("CODEX_THREAD_ID", "test-thread")
            .env("RELAY_FIXTURE_CWD", workspace)
            .env("PATH", format!("{}:{}", fixtures.display(), path))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("relay-mcp must start");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
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
                tokio::time::timeout(Duration::from_secs(30), self.stdout.read_line(&mut line))
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

    async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let response = self
            .send(
                "tools/call",
                Some(serde_json::json!({ "name": name, "arguments": arguments })),
            )
            .await;
        assert!(response.get("error").is_none(), "{name} failed: {response}");
        let result = response["result"].clone();
        assert_ne!(
            result.get("isError").and_then(|value| value.as_bool()),
            Some(true),
            "{name} returned an error: {result}"
        );
        result["structuredContent"]["result"].clone()
    }

    async fn shutdown(mut self) {
        let _ = self.child.kill().await;
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn workspace_config(home: &Path, fixtures: &Path, cwd: &Path) {
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
    std::fs::write(home.join("cwd.txt"), cwd.display().to_string()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn codex_can_delegate_wait_and_accept_over_mcp() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let fixtures = fixtures();
    workspace_config(home.path(), &fixtures, workspace.path());

    let mut client = McpClient::start(home.path(), &fixtures, workspace.path()).await;
    let initialize = client
        .send(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "relay-e2e", "version": "0.1.0" }
            })),
        )
        .await;
    assert_eq!(initialize["result"]["serverInfo"]["name"], "relay");
    client.notify("notifications/initialized").await;

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
    let database = relay_storage::Database::open(home.path().join("relay.sqlite")).unwrap();
    let store = relay_storage::SqliteEventStore::new(std::sync::Arc::new(database));
    let events = relay_core::EventStore::list(&store, &run_id).unwrap();
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
    let sessions = relay_storage::SqliteHostSessionStore::new(std::sync::Arc::new(
        relay_storage::Database::open(home.path().join("relay.sqlite")).unwrap(),
    ));
    let stored = relay_core::HostSessionStore::list(&sessions).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].display_name, "Fixture session");
    assert_eq!(stored[0].cwd, workspace.path().display().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_worker_reports_its_message_and_never_awaits_the_host() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let fixtures = fixtures();
    workspace_config(home.path(), &fixtures, workspace.path());

    let mut client = McpClient::start(home.path(), &fixtures, workspace.path()).await;
    client
        .send(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "relay-e2e", "version": "0.1.0" }
            })),
        )
        .await;
    client.notify("notifications/initialized").await;

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
    let response = client
        .send(
            "tools/call",
            Some(serde_json::json!({
                "name": "accept_agent",
                "arguments": { "worker_session_id": worker_id }
            })),
        )
        .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("cannot be accepted"));

    client.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_worker_ends_the_run_as_cancelled() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let fixtures = fixtures();
    workspace_config(home.path(), &fixtures, workspace.path());

    let mut client = McpClient::start(home.path(), &fixtures, workspace.path()).await;
    client
        .send(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "relay-e2e", "version": "0.1.0" }
            })),
        )
        .await;
    client.notify("notifications/initialized").await;

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
    assert!(status["lastEvent"]["type"] == "worker/cancelled");

    client.shutdown().await;
}
