//! The daemon's HTTP surface, driven through the real router.
//!
//! These are the guarantees the tray, the panel, the inspector and the MCP server
//! rely on: the loopback guards, the token, the routes themselves, the execution
//! calls the front-ends make, and the SPA fallback.

use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use relay_api::{
    router, CodexAction, CodexIntegration, CodexStatus, EnvironmentService, InstallResult,
    PolicyBody, RelayConfigView, RelayServerOptions, RelayServerState, RelayStore,
    RunProjectionView, RunService, RunStartBody, RunStarted, RuntimeBody, RuntimeMutation,
    RuntimeProbe, SessionContext,
};
use relay_core::{
    AccessMode, AgentProfile, HostSession, Isolation, RelayPolicy, Run, RunStatus, RuntimeOptions,
    Step, StepStatus, WorkerSession, WorkerStatus,
};
use relay_storage::{Database, SqliteEventStore, SqliteHostSessionStore};
use tower::ServiceExt;

const TOKEN: &str = "test-token";

/// The daemon's configuration, as the HTTP layer sees it.
///
/// A save really changes the profiles this stub reports, so a route that forgets
/// to hand the new environment to the store fails here instead of only in the
/// menu bar.
#[derive(Default)]
struct TestService {
    profiles: Mutex<Vec<AgentProfile>>,
}

impl TestService {
    fn profiles(&self) -> Vec<AgentProfile> {
        self.profiles.lock().unwrap().clone()
    }

    fn view(&self, profiles: Vec<AgentProfile>) -> RelayConfigView {
        RelayConfigView {
            profiles,
            policy: RelayPolicy::default(),
            workspace_overrides: Default::default(),
            manual_runtimes: Vec::new(),
            warnings: vec!["config.toml 无法解析".to_string()],
            revision: "revision-1".to_string(),
        }
    }
}

#[async_trait]
impl EnvironmentService for TestService {
    fn environment(&self) -> relay_api::environment::Environment {
        relay_api::environment::Environment {
            profiles: self.profiles(),
            ..Default::default()
        }
    }

    fn config(&self) -> RelayConfigView {
        self.view(self.profiles())
    }

    async fn refresh(&self) -> (relay_api::environment::Environment, RelayConfigView) {
        (self.environment(), self.config())
    }

    async fn runtime_options(&self, runtime_id: &str) -> RuntimeOptions {
        RuntimeOptions::empty(runtime_id, "deepseek-harness", "no options")
    }

    async fn probe(&self, _adapter_id: &str, _executable_path: &str) -> RuntimeProbe {
        RuntimeProbe {
            ok: true,
            version: Some("1.2.3".to_string()),
            error: None,
        }
    }

    async fn save_runtime(&self, _id: &str, _body: RuntimeBody) -> RuntimeMutation {
        RuntimeMutation {
            config: self.config(),
            probe: RuntimeProbe {
                ok: true,
                version: None,
                error: None,
            },
        }
    }

    fn delete_runtime(&self, _id: &str) -> RelayConfigView {
        self.config()
    }

    fn save_profile(&self, profile: AgentProfile) -> Result<RelayConfigView, String> {
        let profiles = {
            let mut profiles = self.profiles.lock().unwrap();
            profiles.retain(|existing| existing.id != profile.id);
            profiles.push(profile);
            profiles.clone()
        };
        Ok(self.view(profiles))
    }

    fn delete_profile(&self, id: &str) -> Result<RelayConfigView, String> {
        let profiles = {
            let mut profiles = self.profiles.lock().unwrap();
            profiles.retain(|existing| existing.id != id);
            profiles.clone()
        };
        Ok(self.view(profiles))
    }

    fn save_policy(&self, _body: PolicyBody) -> Result<RelayConfigView, String> {
        Ok(self.config())
    }

    fn adapter_ids(&self) -> Vec<String> {
        vec!["deepseek-harness".to_string()]
    }
}

struct TestCodex {
    probes: Arc<AtomicUsize>,
}

#[async_trait]
impl CodexIntegration for TestCodex {
    async fn status(&self) -> CodexStatus {
        self.probes.fetch_add(1, Ordering::SeqCst);
        CodexStatus {
            checks: Vec::new(),
            configured: false,
        }
    }

    async fn run(&self, _action: CodexAction) -> InstallResult {
        InstallResult {
            status: self.status().await,
            messages: vec!["installed".to_string()],
        }
    }
}

/// Records what the HTTP layer asked execution to do.
#[derive(Default)]
struct RecordingRuns {
    calls: Mutex<Vec<String>>,
}

impl RecordingRuns {
    fn record(&self, call: impl Into<String>) {
        self.calls.lock().unwrap().push(call.into());
    }
}

fn projection() -> RunProjectionView {
    let stamp = relay_core::now();
    RunProjectionView {
        run: Run {
            id: "run:1".into(),
            host_session_id: "codex:test".into(),
            profile_id: "agent-1".into(),
            task: "do it".into(),
            cwd: "/tmp".into(),
            access_mode: AccessMode::ReadOnly,
            isolation: Isolation::Shared,
            status: RunStatus::Running,
            created_at: stamp.clone(),
            updated_at: stamp.clone(),
        },
        steps: vec![Step {
            id: "step:1".into(),
            run_id: "run:1".into(),
            profile_id: "agent-1".into(),
            task: "do it".into(),
            access_mode: AccessMode::ReadOnly,
            isolation: Isolation::Shared,
            status: StepStatus::Running,
            iteration: 1,
            created_at: stamp.clone(),
            updated_at: stamp.clone(),
        }],
        workers: vec![WorkerSession {
            id: "worker:1".into(),
            run_id: "run:1".into(),
            step_id: "step:1".into(),
            iteration: 1,
            runtime_id: "runtime:1".into(),
            native_session_id: None,
            parent_worker_session_id: None,
            process_id: None,
            process: None,
            status: WorkerStatus::Running,
            started_at: stamp.clone(),
            ended_at: None,
        }],
        result: None,
        last_event: None,
    }
}

fn host_session() -> HostSession {
    HostSession {
        id: "codex:test".into(),
        host: "codex".into(),
        native_session_id: "test".into(),
        display_name: "Test session".into(),
        name_source: "codex".into(),
        cwd: "/tmp".into(),
        model: None,
        status: relay_core::HostSessionStatus::Active,
        started_at: relay_core::now(),
        updated_at: relay_core::now(),
        ended_at: None,
    }
}

#[async_trait]
impl RunService for RecordingRuns {
    async fn list_agents(&self, session: &SessionContext) -> Result<Vec<AgentProfile>, String> {
        self.record(format!("list_agents:{}", session.thread_id));
        Ok(Vec::new())
    }

    async fn sync_session(&self, session: &SessionContext) -> Result<HostSession, String> {
        self.record(format!("sync_session:{}", session.thread_id));
        Ok(host_session())
    }

    async fn end_session(&self, session: &SessionContext) -> Result<Option<HostSession>, String> {
        self.record(format!("end_session:{}", session.thread_id));
        Ok(Some(host_session()))
    }

    async fn end_session_by_native_id(
        &self,
        native_session_id: &str,
    ) -> Result<Option<HostSession>, String> {
        self.record(format!("end_session_by_native_id:{native_session_id}"));
        Ok(Some(host_session()))
    }

    async fn start(&self, body: RunStartBody) -> Result<RunStarted, String> {
        self.record(format!("start:{}", body.agent_id));
        Ok(RunStarted {
            run_id: "run:1".into(),
            worker_session_id: "worker:1".into(),
            host_session_display_name: "Test session".into(),
        })
    }

    async fn resume(&self, worker_session_id: &str, feedback: &str) -> Result<RunStarted, String> {
        self.record(format!("resume:{worker_session_id}:{feedback}"));
        Ok(RunStarted {
            run_id: "run:1".into(),
            worker_session_id: "worker:2".into(),
            host_session_display_name: "Test session".into(),
        })
    }

    async fn status(&self, worker_session_id: &str) -> Result<RunProjectionView, String> {
        self.record(format!("status:{worker_session_id}"));
        Ok(projection())
    }

    async fn wait(&self, worker_session_id: &str) -> Result<RunProjectionView, String> {
        self.record(format!("wait:{worker_session_id}"));
        Ok(projection())
    }

    async fn send(&self, worker_session_id: &str, message: &str) -> Result<(), String> {
        self.record(format!("send:{worker_session_id}:{message}"));
        Ok(())
    }

    async fn cancel(&self, worker_session_id: &str) -> Result<(), String> {
        self.record(format!("cancel:{worker_session_id}"));
        Ok(())
    }

    async fn accept(&self, worker_session_id: &str) -> Result<RunProjectionView, String> {
        self.record(format!("accept:{worker_session_id}"));
        Ok(projection())
    }

    async fn cancel_session(&self, host_session_id: &str) -> Result<u32, String> {
        self.record(format!("cancel_session:{host_session_id}"));
        Ok(0)
    }
}

struct Harness {
    state: Arc<RelayServerState>,
    runs: Arc<RecordingRuns>,
    codex_probes: Arc<AtomicUsize>,
}

fn harness(directory: &std::path::Path) -> Harness {
    let database = Arc::new(Database::open(directory.join("relay.sqlite")).unwrap());
    let store = Arc::new(RelayStore::new(
        Arc::new(SqliteEventStore::new(Arc::clone(&database))),
        Arc::new(SqliteHostSessionStore::new(Arc::clone(&database))),
    ));
    let runs = Arc::new(RecordingRuns::default());
    let codex_probes = Arc::new(AtomicUsize::new(0));
    let state = RelayServerState::new(RelayServerOptions {
        store,
        service: Arc::new(TestService::default()),
        runs: runs.clone(),
        codex: Arc::new(TestCodex {
            probes: Arc::clone(&codex_probes),
        }),
        web_root: directory.join("ui"),
        panel_root: directory.join("ui"),
        token: TOKEN.to_string(),
        port: Arc::new(AtomicU16::new(7352)),
        version: "0.1.0".to_string(),
        started_at: relay_core::now(),
        nonce: "nonce-1".to_string(),
        database_path: directory.join("relay.sqlite").display().to_string(),
    });
    Harness {
        state,
        runs,
        codex_probes,
    }
}

async fn get(
    state: &Arc<RelayServerState>,
    path: &str,
    host: &str,
    token: Option<&str>,
) -> (StatusCode, String) {
    let mut request = Request::builder()
        .uri(path)
        .method("GET")
        .header("host", host);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = router(Arc::clone(state))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).to_string())
}

async fn send(
    state: &Arc<RelayServerState>,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, String) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:7352")
        .header("authorization", format!("Bearer {TOKEN}"));
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let body = body
        .map(|body| Body::from(body.to_string()))
        .unwrap_or_else(Body::empty);
    let response = router(Arc::clone(state))
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn post(
    state: &Arc<RelayServerState>,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    send(state, "POST", path, Some(body)).await
}

async fn put(
    state: &Arc<RelayServerState>,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    send(state, "PUT", path, Some(body)).await
}

async fn delete(state: &Arc<RelayServerState>, path: &str) -> (StatusCode, String) {
    send(state, "DELETE", path, None).await
}

#[tokio::test]
async fn the_api_answers_with_a_token_and_refuses_without_one() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = get(
        &harness.state,
        "/api/health?token=test-token",
        "127.0.0.1:7352",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let health: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(health["ok"], true);
    assert_eq!(health["nonce"], "nonce-1");
    assert_eq!(health["port"], 7352);
    assert_eq!(health["version"], "0.1.0");
    assert_eq!(health["sessions"], 0);
    assert_eq!(health["runs"], 0);

    let (status, body) = get(&harness.state, "/api/health", "127.0.0.1:7352", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("Missing or invalid Relay token"));

    let (status, _) = get(
        &harness.state,
        "/api/health",
        "127.0.0.1:7352",
        Some("test-token"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// The Status tab polls health; a health check must never run the Codex CLI.
#[tokio::test]
async fn health_never_probes_the_codex_integration() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    for _ in 0..5 {
        let (status, _) = get(
            &harness.state,
            "/api/health?token=test-token",
            "127.0.0.1:7352",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(harness.codex_probes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_codex_status_is_probed_once_per_ttl_and_invalidated_by_an_action() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    // Four reads inside the TTL are one probe.
    for _ in 0..4 {
        let _ = harness.state.codex_status().await;
    }
    assert_eq!(harness.codex_probes.load(Ordering::SeqCst), 1);

    // An install/repair/update/remove invalidates it.
    let (status, _) = post(&harness.state, "/api/codex/repair", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let _ = harness.state.codex_status().await;
    assert_eq!(harness.codex_probes.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn the_guards_reject_foreign_hosts_and_origins() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = get(
        &harness.state,
        "/api/health?token=test-token",
        "evil.example.com",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("loopback"));

    let request = Request::builder()
        .uri("/api/health?token=test-token")
        .header("host", "127.0.0.1:7352")
        .header("origin", "https://evil.example.com")
        .body(Body::empty())
        .unwrap();
    let response = router(Arc::clone(&harness.state))
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_projection_routes_serve_what_the_surfaces_render() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = get(
        &harness.state,
        "/api/snapshot",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let snapshot: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(snapshot["sessions"].as_array().unwrap().is_empty());
    assert!(snapshot["generatedAt"].is_string());

    let (status, body) = get(&harness.state, "/api/menu", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let menu: serde_json::Value = serde_json::from_str(&body).unwrap();
    // No runtimes detected, no Codex wiring: the menu says so instead of "ready".
    assert_eq!(menu["status"], "noRuntime");
    assert_eq!(menu["runningWorkers"], 0);

    let (status, body) = get(&harness.state, "/api/config", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let config: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(config["profiles"].as_array().unwrap().len(), 0);
    assert_eq!(config["warnings"][0], "config.toml 无法解析");
    assert_eq!(config["revision"], "revision-1");

    let (status, body) = get(
        &harness.state,
        "/api/adapters",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"adapters":["deepseek-harness"]}"#);

    let (status, body) = get(
        &harness.state,
        "/api/runtimes/runtime:1/options",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let options: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(options["runtimeId"], "runtime:1");
    assert_eq!(options["source"], "default");

    let (status, body) = get(
        &harness.state,
        "/api/runs/run:1/events?after=0",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"runId":"run:1","events":[]}"#);
}

/// The control panel saves to `/api/config`; the menu bar reads `/api/menu`.
/// A profile only reaches the tray because every mutation hands the store the
/// environment the service just produced — the store projects what it was given,
/// and it is given nothing at all until a mutation publishes one.
#[tokio::test]
async fn a_saved_profile_reaches_the_menu_and_the_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (_, body) = get(&harness.state, "/api/menu", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(menu_agents(&body).len(), 0);

    let (status, _) = put(
        &harness.state,
        "/api/config/profiles/agent-menu",
        serde_json::json!({
            "id": "agent-menu",
            "name": "Menu Check",
            "description": "",
            "runtimeId": "runtime:missing",
            "capabilities": {
                "readWorkspace": true,
                "writeWorkspace": false,
                "executeCommands": false,
                "networkAccess": false
            },
            "enabled": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = get(&harness.state, "/api/menu", "127.0.0.1:7352", Some(TOKEN)).await;
    let agents = menu_agents(&body);
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "agent-menu");
    assert_eq!(agents[0]["name"], "Menu Check");
    // The runtime it names does not exist: the menu says why instead of hiding it.
    assert_eq!(agents[0]["blocked"], "missing");

    // The panel and the inspector read the snapshot, so it has to agree.
    let (_, body) = get(
        &harness.state,
        "/api/snapshot",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    let snapshot: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(snapshot["profiles"][0]["name"], "Menu Check");

    // A deleted agent must leave the menu too, or the tray keeps offering it.
    let (status, _) = delete(&harness.state, "/api/config/profiles/agent-menu").await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&harness.state, "/api/menu", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(menu_agents(&body).len(), 0);
}

/// The agents one menu payload offers, in the order the tray draws them.
fn menu_agents(body: &str) -> Vec<serde_json::Value> {
    let menu: serde_json::Value = serde_json::from_str(body).unwrap();
    menu["agents"].as_array().cloned().unwrap_or_default()
}

/// Execution belongs to the daemon: every MCP tool has a route here, and the
/// routes call the run service directly instead of leaving work for another
/// process to pick up.
#[tokio::test]
async fn execution_routes_reach_the_run_service() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = post(
        &harness.state,
        "/api/agents",
        serde_json::json!({ "threadId": "codex:test" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "[]");

    let (status, body) = post(
        &harness.state,
        "/api/runs",
        serde_json::json!({
            "session": { "threadId": "codex:test" },
            "agentId": "agent-1",
            "task": "do it",
            "accessMode": "write"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let started: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(started["runId"], "run:1");
    assert_eq!(started["workerSessionId"], "worker:1");

    let (status, body) = get(
        &harness.state,
        "/api/workers/worker:1",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let view: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(view["run"]["id"], "run:1");
    assert_eq!(view["workers"][0]["id"], "worker:1");

    let (status, _) = post(
        &harness.state,
        "/api/workers/worker:1/wait",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post(
        &harness.state,
        "/api/workers/worker:1/send",
        serde_json::json!({ "message": "go on" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"sent":true}"#);

    let (status, _) = post(
        &harness.state,
        "/api/workers/worker:1/accept",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post(
        &harness.state,
        "/api/workers/worker:1/resume",
        serde_json::json!({ "feedback": "again" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("worker:2"));

    let (status, body) = post(
        &harness.state,
        "/api/sessions/sync",
        serde_json::json!({ "threadId": "codex:test" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("codex:test"));

    let (status, _) = post(
        &harness.state,
        "/api/sessions/end",
        serde_json::json!({ "nativeSessionId": "test" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let calls = harness.runs.calls.lock().unwrap().clone();
    assert!(calls.contains(&"start:agent-1".to_string()), "{calls:?}");
    assert!(calls.contains(&"wait:worker:1".to_string()), "{calls:?}");
    assert!(
        calls.contains(&"send:worker:1:go on".to_string()),
        "{calls:?}"
    );
    assert!(
        calls.contains(&"resume:worker:1:again".to_string()),
        "{calls:?}"
    );
    assert!(
        calls.contains(&"end_session_by_native_id:test".to_string()),
        "{calls:?}"
    );
}

/// Cancellation is a direct call on the worker's owner. There is no queue, no
/// claim and no lease: a cancel can never be consumed by the wrong process.
#[tokio::test]
async fn cancellation_is_a_direct_call_on_the_owner() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = post(
        &harness.state,
        "/api/workers/worker:1/cancel",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let result: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["accepted"], true);

    let (status, body) = post(
        &harness.state,
        "/api/sessions/codex:test/cancel",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let result: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["accepted"], true);
    assert_eq!(result["count"], 0);

    let calls = harness.runs.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            "cancel:worker:1".to_string(),
            "cancel_session:codex:test".to_string()
        ]
    );
}

#[tokio::test]
async fn unknown_api_routes_answer_as_an_api_and_the_ui_falls_back_to_the_spa() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = get(&harness.state, "/api/nope", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body was {body}");
    assert_eq!(
        body, r#"{"error":"Unknown Relay API route: /api/nope"}"#,
        "the API must answer with the full path it was asked for"
    );

    // The UI is not built in this test directory: the daemon explains that
    // instead of answering with an empty page.
    let (status, body) = get(&harness.state, "/", "127.0.0.1:7352", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Relay inspector is not built yet"));

    let (status, body) = get(&harness.state, "/panel", "127.0.0.1:7352", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Relay inspector is not built yet"));
}

#[tokio::test]
async fn diagnostics_are_plain_text_for_a_bug_report() {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(directory.path());

    let (status, body) = get(
        &harness.state,
        "/api/diagnostics",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with("Relay diagnostics\n"));
    assert!(body.contains("Relay 0.1.0"));
    assert!(body.contains("Daemon: http://127.0.0.1:7352"));
    assert!(body.contains("Sessions: 0"));
}
