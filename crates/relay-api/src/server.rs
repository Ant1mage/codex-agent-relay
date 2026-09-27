//! The daemon's only listener: loopback HTTP that serves the built UI, the
//! projection it renders, and Relay's own configuration.
//!
//! Two local-only guards protect it. The `Host` check stops DNS rebinding (a page
//! on a public name resolving to loopback), and the `Origin` check stops other
//! sites from reading the log through the user's browser. The token then covers
//! everything that is not the browser.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures::stream::Stream;
use relay_core::{AgentProfile, RuntimeOptions};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::contract::{
    AdapterCatalog, ApiError, CancelResult, CodexAction, CodexStatus, Health, InstallResult,
    InspectorSnapshot, PolicyBody, ProbeBody, RefreshResult, RelayConfigView, RuntimeBody, RuntimeMutation,
    RuntimeProbe, StreamMessage,
};
use crate::diagnostics::{build_diagnostics_report, DiagnosticsInput};
use crate::store_view::RelayStore;

/// What the daemon can do that is not reading the event log.
#[async_trait]
pub trait EnvironmentService: Send + Sync {
    fn environment(&self) -> crate::environment::Environment;
    fn config(&self) -> RelayConfigView;
    async fn refresh(&self) -> (crate::environment::Environment, RelayConfigView);
    async fn runtime_options(&self, runtime_id: &str) -> RuntimeOptions;
    async fn probe(&self, adapter_id: &str, executable_path: &str) -> RuntimeProbe;
    async fn save_runtime(&self, id: &str, body: RuntimeBody) -> RuntimeMutation;
    fn delete_runtime(&self, id: &str) -> RelayConfigView;
    fn save_profile(&self, profile: AgentProfile) -> Result<RelayConfigView, String>;
    fn delete_profile(&self, id: &str) -> Result<RelayConfigView, String>;
    fn save_policy(&self, body: PolicyBody) -> Result<RelayConfigView, String>;
    fn adapter_ids(&self) -> Vec<String>;
}

/// The Codex integration lifecycle, implemented outside the HTTP layer.
#[async_trait]
pub trait CodexIntegration: Send + Sync {
    async fn status(&self) -> CodexStatus;
    async fn run(&self, action: CodexAction) -> InstallResult;
}

pub struct RelayServerState {
    pub store: Arc<RelayStore>,
    pub service: Arc<dyn EnvironmentService>,
    pub codex: Arc<dyn CodexIntegration>,
    pub web_root: PathBuf,
    pub panel_root: PathBuf,
    pub token: String,
    pub port: Arc<AtomicU16>,
    pub version: String,
    pub started_at: String,
    pub nonce: String,
    pub database_path: String,
    pub tick: Duration,
    updates: broadcast::Sender<Arc<InspectorSnapshot>>,
}

impl RelayServerState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<RelayStore>,
        service: Arc<dyn EnvironmentService>,
        codex: Arc<dyn CodexIntegration>,
        web_root: PathBuf,
        panel_root: PathBuf,
        token: String,
        port: Arc<AtomicU16>,
        version: String,
        started_at: String,
        nonce: String,
        database_path: String,
    ) -> Arc<Self> {
        let (updates, _) = broadcast::channel(64);
        Arc::new(Self {
            store,
            service,
            codex,
            web_root,
            panel_root,
            token,
            port,
            version,
            started_at,
            nonce,
            database_path,
            tick: Duration::from_millis(400),
            updates,
        })
    }

    /// Cheap identity of everything a client can see. Without the runtime scan
    /// and the Codex state a client would keep rendering state that changed
    /// elsewhere.
    async fn stamp(&self) -> String {
        let codex = self.codex.status().await;
        format!(
            "{}|{}|{:?}|{:?}",
            self.store.revision(),
            self.service.config().revision,
            self.service.environment(),
            codex
        )
    }

    /// One stamp covers the event log, the runtime scan, the configuration and
    /// the Codex integration; the ticker only publishes when it moves.
    pub async fn run_ticker(self: Arc<Self>) {
        let mut last = String::new();
        loop {
            tokio::time::sleep(self.tick).await;
            if self.updates.receiver_count() == 0 {
                continue;
            }
            let current = self.stamp().await;
            if current == last {
                continue;
            }
            last = current;
            let codex = self.codex.status().await;
            let snapshot = Arc::new(self.store.snapshot(codex));
            let _ = self.updates.send(snapshot);
        }
    }
}

/* ------------------------------------------------------------------ */
/* Guards                                                              */
/* ------------------------------------------------------------------ */

fn host_names(port: u16) -> Vec<String> {
    vec![
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        "127.0.0.1".to_string(),
        "localhost".to_string(),
    ]
}

fn request_allowed(headers: &HeaderMap, port: u16) -> bool {
    let allowed = host_names(port);
    let Some(host) = headers.get("host").and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if !allowed.iter().any(|candidate| candidate == host) {
        return false;
    }
    match headers.get("origin").and_then(|value| value.to_str().ok()) {
        None => true,
        Some(origin) => match origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")) {
            Some(rest) => allowed.iter().any(|candidate| candidate == rest),
            None => false,
        },
    }
}

fn authorized(state: &RelayServerState, headers: &HeaderMap, uri: &Uri) -> bool {
    if let Some(query) = uri.query() {
        for pair in query.split('&') {
            if let Some(value) = pair.strip_prefix("token=") {
                if value == state.token {
                    return true;
                }
            }
        }
    }
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(|value| value == format!("Bearer {}", state.token))
        .unwrap_or(false)
}

/* ------------------------------------------------------------------ */
/* Handlers                                                            */
/* ------------------------------------------------------------------ */

async fn snapshot_of(state: &RelayServerState) -> InspectorSnapshot {
    state.store.snapshot(state.codex.status().await)
}

async fn health(State(state): State<Arc<RelayServerState>>) -> Response {
    let snapshot = snapshot_of(&state).await;
    let runs: u32 = snapshot.sessions.iter().map(|session| session.runs.len() as u32).sum();
    Json(Health {
        ok: true,
        pid: std::process::id(),
        nonce: state.nonce.clone(),
        port: state.port.load(Ordering::SeqCst),
        started_at: state.started_at.clone(),
        version: state.version.clone(),
        database: state.database_path.clone(),
        sessions: snapshot.sessions.len() as u32,
        runs,
    })
    .into_response()
}

async fn snapshot(State(state): State<Arc<RelayServerState>>) -> Response {
    Json(snapshot_of(&state).await).into_response()
}

async fn menu(State(state): State<Arc<RelayServerState>>) -> Response {
    let codex = state.codex.status().await;
    Json(state.store.menu(codex)).into_response()
}

async fn config(State(state): State<Arc<RelayServerState>>) -> Response {
    Json(state.service.config()).into_response()
}

async fn adapters(State(state): State<Arc<RelayServerState>>) -> Response {
    Json(AdapterCatalog { adapters: state.service.adapter_ids() }).into_response()
}

fn failed(message: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, Json(ApiError { error: message.into() })).into_response()
}

async fn save_profile(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
    Json(mut profile): Json<AgentProfile>,
) -> Response {
    profile.id = id;
    match state.service.save_profile(profile) {
        Ok(config) => Json(config).into_response(),
        Err(message) => failed(message),
    }
}

async fn delete_profile(State(state): State<Arc<RelayServerState>>, AxumPath(id): AxumPath<String>) -> Response {
    match state.service.delete_profile(&id) {
        Ok(config) => Json(config).into_response(),
        Err(message) => failed(message),
    }
}

async fn save_policy(State(state): State<Arc<RelayServerState>>, Json(body): Json<PolicyBody>) -> Response {
    match state.service.save_policy(body) {
        Ok(config) => Json(config).into_response(),
        Err(message) => failed(message),
    }
}

async fn runtime_options(State(state): State<Arc<RelayServerState>>, AxumPath(id): AxumPath<String>) -> Response {
    Json(state.service.runtime_options(&id).await).into_response()
}

async fn probe_runtime(State(state): State<Arc<RelayServerState>>, Json(body): Json<ProbeBody>) -> Response {
    if body.executable_path.is_empty() {
        return failed("Missing executablePath");
    }
    Json(state.service.probe(&body.adapter_id, &body.executable_path).await).into_response()
}

async fn save_runtime(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RuntimeBody>,
) -> Response {
    if body.adapter_id.is_empty() || body.executable_path.is_empty() {
        return failed("Missing runtime body");
    }
    Json(state.service.save_runtime(&id, body).await).into_response()
}

async fn delete_runtime(State(state): State<Arc<RelayServerState>>, AxumPath(id): AxumPath<String>) -> Response {
    Json(state.service.delete_runtime(&id)).into_response()
}

async fn diagnostics(State(state): State<Arc<RelayServerState>>) -> Response {
    let snapshot = snapshot_of(&state).await;
    let report = build_diagnostics_report(&DiagnosticsInput {
        app_version: &state.version,
        runtime_version: "rust",
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        port: state.port.load(Ordering::SeqCst),
        database_path: &state.database_path,
        snapshot: &snapshot,
        generated_at: None,
    });
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8"), ("cache-control", "no-store")],
        report,
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
}

async fn run_events(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<EventsQuery>,
) -> Response {
    Json(state.store.events_for(&run_id, query.after)).into_response()
}

async fn cancel_worker(State(state): State<Arc<RelayServerState>>, AxumPath(id): AxumPath<String>) -> Response {
    match state.store.cancel_worker(&id) {
        Ok(()) => Json(CancelResult {
            accepted: true,
            count: None,
            message: Some("Cancellation requested".to_string()),
        })
        .into_response(),
        Err(error) => Json(CancelResult {
            accepted: false,
            count: None,
            message: Some(error.message().to_string()),
        })
        .into_response(),
    }
}

async fn cancel_session(State(state): State<Arc<RelayServerState>>, AxumPath(id): AxumPath<String>) -> Response {
    match state.store.cancel_session(&id) {
        Ok(count) => Json(CancelResult { accepted: true, count: Some(count), message: None }).into_response(),
        Err(error) => Json(CancelResult {
            accepted: false,
            count: None,
            message: Some(error.message().to_string()),
        })
        .into_response(),
    }
}

async fn codex_action(State(state): State<Arc<RelayServerState>>, AxumPath(action): AxumPath<String>) -> Response {
    match CodexAction::parse(&action) {
        Some(action) => Json(state.codex.run(action).await).into_response(),
        None => failed(format!("Unknown Codex action: {action}")),
    }
}

async fn refresh(State(state): State<Arc<RelayServerState>>) -> Response {
    let (environment, config) = state.service.refresh().await;
    Json(RefreshResult {
        runtimes: environment.runtimes.len() as u32,
        profiles: config.profiles.len() as u32,
        detected_at: relay_core::now(),
    })
    .into_response()
}

const MIME: [(&str, &str); 11] = [
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json; charset=utf-8"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("ico", "image/x-icon"),
    ("woff2", "font/woff2"),
];

fn mime_for(path: &Path) -> &'static str {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    MIME.iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime)| *mime)
        .unwrap_or("application/octet-stream")
}

/// Serves the built UI. `/panel/*` is the control panel: the same bundle, so its
/// API calls pass the Origin guard without special cases.
async fn serve_static(state: &RelayServerState, pathname: &str) -> Response {
    let is_panel = pathname == "/panel" || pathname.starts_with("/panel/");
    let root = if is_panel { &state.panel_root } else { &state.web_root };
    let relative = if is_panel {
        pathname.trim_start_matches("/panel").trim_start_matches('/')
    } else {
        pathname.trim_start_matches('/')
    };
    let relative = if relative.is_empty() { "index.html" } else { relative };
    let candidate = root.join(relative);
    let inside = candidate.starts_with(root);
    let target = if inside { candidate } else { root.join("index.html") };

    match tokio::fs::read(&target).await {
        Ok(body) => (
            StatusCode::OK,
            [("content-type", mime_for(&target)), ("cache-control", "no-store")],
            body,
        )
            .into_response(),
        Err(_) => {
            // Single-page fallback for a client-side route.
            if pathname != "/" && Path::new(pathname).extension().is_none() {
                if let Ok(body) = tokio::fs::read(root.join("index.html")).await {
                    return (
                        StatusCode::OK,
                        [("content-type", "text/html; charset=utf-8"), ("cache-control", "no-store")],
                        body,
                    )
                        .into_response();
                }
            }
            (
                StatusCode::OK,
                [("content-type", "text/plain; charset=utf-8")],
                format!(
                    "Relay inspector is not built yet.\n\nBuild it with:  trunk build --release\nAPI is available at http://127.0.0.1:{}/api/health?token=…\n",
                    state.port.load(Ordering::SeqCst)
                ),
            )
                .into_response()
        }
    }
}

async fn stream(
    State(state): State<Arc<RelayServerState>>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut updates = state.updates.subscribe();
    // Start from "now": the client pulls history per run and de-duplicates by
    // sequence, so the stream only has to deliver what happens next.
    let mut cursors = state.store.cursors();
    let port = state.port.load(Ordering::SeqCst);
    let started_at = state.started_at.clone();
    let initial = snapshot_of(&state).await;
    let store = Arc::clone(&state.store);

    let stream = async_stream::stream! {
        yield Ok(Event::default().comment("relay stream"));
        yield Ok(Event::default().data(serde_json::to_string(&StreamMessage::Hello { port, started_at }).unwrap()));
        yield Ok(Event::default().data(serde_json::to_string(&StreamMessage::Snapshot { snapshot: Box::new(initial) }).unwrap()));

        loop {
            match updates.recv().await {
                Ok(snapshot) => {
                    let current = store.cursors();
                    for (run_id, seq) in current.iter() {
                        let sent = cursors.get(run_id).copied().unwrap_or(0);
                        if *seq <= sent {
                            continue;
                        }
                        let batch = store.events_for(run_id, sent);
                        cursors.insert(run_id.clone(), *seq);
                        if !batch.events.is_empty() {
                            yield Ok(Event::default().data(
                                serde_json::to_string(&StreamMessage::Events { batch }).unwrap(),
                            ));
                        }
                    }
                    yield Ok(Event::default().data(
                        serde_json::to_string(&StreamMessage::Snapshot { snapshot: Box::new((*snapshot).clone()) }).unwrap(),
                    ));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let current = snapshot_of(&state).await;
                    yield Ok(Event::default().data(
                        serde_json::to_string(&StreamMessage::Snapshot { snapshot: Box::new(current) }).unwrap(),
                    ));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("ping"))
}

/* ------------------------------------------------------------------ */
/* Router                                                              */
/* ------------------------------------------------------------------ */

pub fn router(state: Arc<RelayServerState>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/menu", get(menu))
        .route("/config", get(config))
        .route("/config/profiles/{id}", put(save_profile).delete(delete_profile))
        .route("/config/policy", put(save_policy))
        .route("/config/runtimes/{id}", put(save_runtime).delete(delete_runtime))
        .route("/adapters", get(adapters))
        .route("/runtimes/probe", post(probe_runtime))
        .route("/runtimes/{id}/options", get(runtime_options))
        .route("/diagnostics", get(diagnostics))
        .route("/runs/{id}/events", get(run_events))
        .route("/workers/{id}/cancel", post(cancel_worker))
        .route("/sessions/{id}/cancel", post(cancel_session))
        .route("/codex/{action}", post(codex_action))
        .route("/refresh", post(refresh))
        .route("/stream", get(stream));

    Router::new()
        .nest("/api", api)
        .fallback(get(fallback))
        .with_state(Arc::clone(&state))
        .layer(axum::middleware::from_fn_with_state(state, guard))
}

async fn fallback(State(state): State<Arc<RelayServerState>>, uri: Uri) -> Response {
    serve_static(&state, uri.path()).await
}

/// One place for both local-only guards and token auth, so no route can forget
/// them.
async fn guard(
    State(state): State<Arc<RelayServerState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let port = state.port.load(Ordering::SeqCst);
    if !request_allowed(request.headers(), port) {
        return (
            StatusCode::FORBIDDEN,
            "Relay only answers loopback requests from its own origin",
        )
            .into_response();
    }
    if request.uri().path().starts_with("/api/") && !authorized(&state, request.headers(), request.uri()) {
        return (StatusCode::UNAUTHORIZED, Json(ApiError { error: "Missing or invalid Relay token".into() }))
            .into_response();
    }
    let response = next.run(request).await;
    if response.status() == StatusCode::NOT_FOUND {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError { error: "Unknown Relay API route".into() }),
        )
            .into_response();
    }
    response
}

/// Binds the first free port in the block, so a second daemon never fails hard.
pub async fn bind(listener_port: u16) -> std::io::Result<(tokio::net::TcpListener, u16)> {
    let mut candidate = listener_port;
    let mut remaining = crate::server_info::PORT_ATTEMPTS;
    loop {
        match tokio::net::TcpListener::bind((crate::server_info::HOST, candidate)).await {
            Ok(listener) => return Ok((listener, candidate)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && remaining > 0 => {
                remaining -= 1;
                candidate += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_origin_guards_reject_foreign_requests() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:7352".parse().unwrap());
        assert!(request_allowed(&headers, 7352));

        headers.insert("host", "evil.example.com".parse().unwrap());
        assert!(!request_allowed(&headers, 7352));

        headers.insert("host", "127.0.0.1:7352".parse().unwrap());
        headers.insert("origin", "https://evil.example.com".parse().unwrap());
        assert!(!request_allowed(&headers, 7352));

        headers.insert("origin", "http://127.0.0.1:7352".parse().unwrap());
        assert!(request_allowed(&headers, 7352));
    }

    #[test]
    fn the_token_is_accepted_as_a_query_parameter_or_a_bearer_header() {
        let state = test_state();
        let uri: Uri = "/api/health?token=secret".parse().unwrap();
        assert!(authorized(&state, &HeaderMap::new(), &uri));

        let uri: Uri = "/api/health".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        assert!(authorized(&state, &headers, &uri));

        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(!authorized(&state, &headers, &uri));
    }

    fn test_state() -> Arc<RelayServerState> {
        use relay_storage::{Database, SqliteControlQueue, SqliteEventStore, SqliteHostSessionStore};
        let directory = std::env::temp_dir().join(format!("relay-api-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = Arc::new(Database::open(directory.join("relay.sqlite")).unwrap());
        let store = Arc::new(RelayStore::new(
            Arc::new(SqliteEventStore::new(Arc::clone(&database))),
            Arc::new(SqliteHostSessionStore::new(Arc::clone(&database))),
            Arc::new(SqliteControlQueue::new(database)),
        ));
        RelayServerState::new(
            store,
            Arc::new(UnavailableService),
            Arc::new(NoCodex),
            directory.join("web"),
            directory.join("panel"),
            "secret".to_string(),
            Arc::new(AtomicU16::new(7352)),
            "0.2.0".to_string(),
            relay_core::now(),
            "nonce".to_string(),
            directory.join("relay.sqlite").display().to_string(),
        )
    }

    struct UnavailableService;

    #[async_trait]
    impl EnvironmentService for UnavailableService {
        fn environment(&self) -> crate::environment::Environment {
            crate::environment::Environment::default()
        }
        fn config(&self) -> RelayConfigView {
            RelayConfigView {
                profiles: Vec::new(),
                policy: Default::default(),
                workspace_overrides: Default::default(),
                manual_runtimes: Vec::new(),
                warnings: Vec::new(),
                revision: "test".into(),
            }
        }
        async fn refresh(&self) -> (crate::environment::Environment, RelayConfigView) {
            (self.environment(), self.config())
        }
        async fn runtime_options(&self, runtime_id: &str) -> RuntimeOptions {
            RuntimeOptions::empty(runtime_id, "none", "no adapter")
        }
        async fn probe(&self, _adapter_id: &str, _executable_path: &str) -> RuntimeProbe {
            RuntimeProbe { ok: false, version: None, error: Some("unsupported".into()) }
        }
        async fn save_runtime(&self, _id: &str, _body: RuntimeBody) -> RuntimeMutation {
            RuntimeMutation {
                config: self.config(),
                probe: RuntimeProbe { ok: false, version: None, error: Some("unsupported".into()) },
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
            Vec::new()
        }
    }

    struct NoCodex;

    #[async_trait]
    impl CodexIntegration for NoCodex {
        async fn status(&self) -> CodexStatus {
            CodexStatus::unknown()
        }
        async fn run(&self, _action: CodexAction) -> InstallResult {
            InstallResult { status: CodexStatus::unknown(), messages: Vec::new() }
        }
    }
}
