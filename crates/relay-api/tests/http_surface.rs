//! The daemon's HTTP surface, driven through the real router.
//!
//! These are the guarantees the tray, the panel and the inspector rely on: the
//! loopback guards, the token, the routes themselves and the SPA fallback.

use std::sync::atomic::AtomicU16;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use relay_api::{
    router, CodexAction, CodexIntegration, CodexStatus, EnvironmentService, InstallResult,
    PolicyBody, RelayConfigView, RelayServerState, RelayStore, RuntimeBody, RuntimeMutation,
    RuntimeProbe,
};
use relay_core::{AgentProfile, RelayPolicy, RuntimeOptions};
use relay_storage::{Database, SqliteControlQueue, SqliteEventStore, SqliteHostSessionStore};
use tower::ServiceExt;

const TOKEN: &str = "test-token";

struct TestService;

#[async_trait]
impl EnvironmentService for TestService {
    fn environment(&self) -> relay_api::environment::Environment {
        relay_api::environment::Environment::default()
    }

    fn config(&self) -> RelayConfigView {
        RelayConfigView {
            profiles: Vec::new(),
            policy: RelayPolicy::default(),
            workspace_overrides: Default::default(),
            manual_runtimes: Vec::new(),
            warnings: vec!["config.toml 无法解析".to_string()],
            revision: "revision-1".to_string(),
        }
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

    fn save_profile(&self, _profile: AgentProfile) -> Result<RelayConfigView, String> {
        Ok(self.config())
    }

    fn delete_profile(&self, _id: &str) -> Result<RelayConfigView, String> {
        Ok(self.config())
    }

    fn save_policy(&self, _body: PolicyBody) -> Result<RelayConfigView, String> {
        Ok(self.config())
    }

    fn adapter_ids(&self) -> Vec<String> {
        vec!["deepseek-harness".to_string()]
    }
}

struct TestCodex;

#[async_trait]
impl CodexIntegration for TestCodex {
    async fn status(&self) -> CodexStatus {
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

fn state(directory: &std::path::Path) -> Arc<RelayServerState> {
    let database = Arc::new(Database::open(directory.join("relay.sqlite")).unwrap());
    let store = Arc::new(RelayStore::new(
        Arc::new(SqliteEventStore::new(Arc::clone(&database))),
        Arc::new(SqliteHostSessionStore::new(Arc::clone(&database))),
        Arc::new(SqliteControlQueue::new(database)),
    ));
    RelayServerState::new(
        store,
        Arc::new(TestService),
        Arc::new(TestCodex),
        directory.join("ui"),
        directory.join("ui"),
        TOKEN.to_string(),
        Arc::new(AtomicU16::new(7352)),
        "0.2.0".to_string(),
        relay_core::now(),
        "nonce-1".to_string(),
        directory.join("relay.sqlite").display().to_string(),
    )
}

async fn call(
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

#[tokio::test]
async fn the_api_answers_with_a_token_and_refuses_without_one() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());

    let (status, body) = call(
        &state,
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
    assert_eq!(health["version"], "0.2.0");
    assert_eq!(health["sessions"], 0);
    assert_eq!(health["runs"], 0);

    let (status, body) = call(&state, "/api/health", "127.0.0.1:7352", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("Missing or invalid Relay token"));

    let (status, _) = call(&state, "/api/health", "127.0.0.1:7352", Some("test-token")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_guards_reject_foreign_hosts_and_origins() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());

    let (status, body) = call(
        &state,
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
    let response = router(Arc::clone(&state)).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_projection_routes_serve_what_the_surfaces_render() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());

    let (status, body) = call(&state, "/api/snapshot", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let snapshot: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(snapshot["sessions"].as_array().unwrap().is_empty());
    assert!(snapshot["generatedAt"].is_string());

    let (status, body) = call(&state, "/api/menu", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let menu: serde_json::Value = serde_json::from_str(&body).unwrap();
    // No runtimes detected, no Codex wiring: the menu says so instead of "ready".
    assert_eq!(menu["status"], "noRuntime");
    assert_eq!(menu["runningWorkers"], 0);

    let (status, body) = call(&state, "/api/config", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    let config: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(config["profiles"].as_array().unwrap().len(), 0);
    assert_eq!(config["warnings"][0], "config.toml 无法解析");
    assert_eq!(config["revision"], "revision-1");

    let (status, body) = call(&state, "/api/adapters", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"adapters":["deepseek-harness"]}"#);

    let (status, body) = call(
        &state,
        "/api/runtimes/runtime:1/options",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let options: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(options["runtimeId"], "runtime:1");
    assert_eq!(options["source"], "default");

    let (status, body) = call(
        &state,
        "/api/runs/run:1/events?after=0",
        "127.0.0.1:7352",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"runId":"run:1","events":[]}"#);
}

#[tokio::test]
async fn cancellations_are_accepted_through_the_control_queue() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());

    let request = Request::builder()
        .method("POST")
        .uri("/api/workers/worker:1/cancel")
        .header("host", "127.0.0.1:7352")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = router(Arc::clone(&state)).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["accepted"], true);

    // The MCP process would claim it; here the queue itself is the assertion.
    let database = Database::open(directory.path().join("relay.sqlite")).unwrap();
    let claimed = SqliteControlQueue::new(Arc::new(database))
        .claim_next()
        .unwrap()
        .unwrap();
    assert_eq!(claimed.worker_session_id, "worker:1");
    assert!(claimed.is_cancel_worker());

    let request = Request::builder()
        .method("POST")
        .uri("/api/sessions/codex:test/cancel")
        .header("host", "127.0.0.1:7352")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = router(Arc::clone(&state)).oneshot(request).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["accepted"], true);
    assert_eq!(result["count"], 0);
}

#[tokio::test]
async fn unknown_api_routes_answer_as_an_api_and_the_ui_falls_back_to_the_spa() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());

    let (status, body) = call(&state, "/api/nope", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body was {body}");
    assert_eq!(
        body, r#"{"error":"Unknown Relay API route: /api/nope"}"#,
        "the API must answer with the full path it was asked for"
    );

    // The UI is not built in this test directory: the daemon explains that
    // instead of answering with an empty page.
    let (status, body) = call(&state, "/", "127.0.0.1:7352", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Relay inspector is not built yet"));

    let (status, body) = call(&state, "/panel", "127.0.0.1:7352", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Relay inspector is not built yet"));
}

#[tokio::test]
async fn diagnostics_are_plain_text_for_a_bug_report() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());

    let (status, body) = call(&state, "/api/diagnostics", "127.0.0.1:7352", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with("Relay diagnostics\n"));
    assert!(body.contains("Relay 0.2.0"));
    assert!(body.contains("Daemon: http://127.0.0.1:7352"));
    assert!(body.contains("Sessions: 0"));
}
