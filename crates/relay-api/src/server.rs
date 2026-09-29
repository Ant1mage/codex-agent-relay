//! The daemon's only listener: loopback HTTP that serves the built UI, the
//! projection it renders, and Relay's own configuration.
//!
//! The daemon also owns execution: the `RunService` behind `/api/runs` is the
//! same controller the daemon supervises, so every front-end (the control panel,
//! the tray, and the MCP server Codex talks to) reaches workers through one
//! process. Nothing else spawns an agent CLI.
//!
//! Two local-only guards protect it. The `Host` check stops DNS rebinding (a page
//! on a public name resolving to loopback), and the `Origin` check stops other
//! sites from reading the log through the user's browser. The token then covers
//! everything that is not the browser.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use futures::stream::Stream;
use relay_core::{AgentProfile, HostSession, RuntimeOptions};
use serde::Deserialize;
use tokio::sync::{broadcast, Mutex as AsyncMutex};

use crate::contract::{
    AdapterCatalog, ApiError, CancelResult, CodexAction, CodexStatus, EndSessionBody, Health,
    InspectorSnapshot, InstallResult, PolicyBody, ProbeBody, RefreshResult, RelayConfigView,
    ResumeBody, RunProjectionView, RunStartBody, RunStarted, RuntimeBody, RuntimeMutation,
    RuntimeProbe, SendBody, SessionContext, StreamMessage,
};
use crate::diagnostics::{build_diagnostics_report, DiagnosticsInput};
use crate::store_view::RelayStore;

/// How long a Codex integration status stays valid before it is probed again.
///
/// `codex.status()` runs the Codex CLI, so it must never sit on the SSE tick's
/// path. Install/repair/update/remove and an explicit refresh invalidate it.
pub const CODEX_STATUS_TTL: Duration = Duration::from_secs(60);

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

/// Execution, as the daemon owns it.
///
/// Every method here is served by the daemon's own `RunController`. Front-ends
/// call them; none of them creates a second execution runtime.
#[async_trait]
pub trait RunService: Send + Sync {
    /// Agent Profiles usable by this host session, after a configuration refresh.
    async fn list_agents(&self, session: &SessionContext) -> Result<Vec<AgentProfile>, String>;
    /// Resolves and stores the host session behind a Codex thread.
    async fn sync_session(&self, session: &SessionContext) -> Result<HostSession, String>;
    /// Marks a host session ended and drops its temporary policy.
    async fn end_session(&self, session: &SessionContext) -> Result<Option<HostSession>, String>;
    /// Ends a session by its native id, for the `SessionEnd` hook.
    async fn end_session_by_native_id(
        &self,
        native_session_id: &str,
    ) -> Result<Option<HostSession>, String>;
    async fn start(&self, body: RunStartBody) -> Result<RunStarted, String>;
    async fn resume(&self, worker_session_id: &str, feedback: &str) -> Result<RunStarted, String>;
    async fn status(&self, worker_session_id: &str) -> Result<RunProjectionView, String>;
    async fn wait(&self, worker_session_id: &str) -> Result<RunProjectionView, String>;
    async fn send(&self, worker_session_id: &str, message: &str) -> Result<(), String>;
    async fn cancel(&self, worker_session_id: &str) -> Result<(), String>;
    async fn accept(&self, worker_session_id: &str) -> Result<RunProjectionView, String>;
    /// Cancels every worker a host session still has running.
    async fn cancel_session(&self, host_session_id: &str) -> Result<u32, String>;
    /// Cancels active workers, deletes a host session and all its associated runs and events.
    async fn delete_session(&self, host_session_id: &str) -> Result<(), String>;
}

/// The Codex integration lifecycle, implemented outside the HTTP layer.
#[async_trait]
pub trait CodexIntegration: Send + Sync {
    async fn status(&self) -> CodexStatus;
    async fn run(&self, action: CodexAction) -> InstallResult;
}

struct CachedCodex {
    at: Instant,
    status: CodexStatus,
}

pub struct RelayServerState {
    pub store: Arc<RelayStore>,
    pub service: Arc<dyn EnvironmentService>,
    pub runs: Arc<dyn RunService>,
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
    /// How long a probed Codex status stays valid. Zero disables the cache.
    pub codex_ttl: Duration,
    codex_cache: RwLock<Option<CachedCodex>>,
    codex_probe_lock: AsyncMutex<()>,
    updates: broadcast::Sender<Arc<InspectorSnapshot>>,
    /// A daemon shutdown has to end live inspector streams before Axum can
    /// finish its graceful shutdown. Otherwise an open EventSource keeps the
    /// HTTP server (and therefore relayd) alive indefinitely.
    stream_shutdown: broadcast::Sender<()>,
}

/// Everything the daemon has to hand the HTTP layer.
pub struct RelayServerOptions {
    pub store: Arc<RelayStore>,
    pub service: Arc<dyn EnvironmentService>,
    pub runs: Arc<dyn RunService>,
    pub codex: Arc<dyn CodexIntegration>,
    pub web_root: PathBuf,
    pub panel_root: PathBuf,
    pub token: String,
    pub port: Arc<AtomicU16>,
    pub version: String,
    pub started_at: String,
    pub nonce: String,
    pub database_path: String,
}

impl RelayServerState {
    pub fn new(options: RelayServerOptions) -> Arc<Self> {
        let (updates, _) = broadcast::channel(64);
        let (stream_shutdown, _) = broadcast::channel(1);
        Arc::new(Self {
            store: options.store,
            service: options.service,
            runs: options.runs,
            codex: options.codex,
            web_root: options.web_root,
            panel_root: options.panel_root,
            token: options.token,
            port: options.port,
            version: options.version,
            started_at: options.started_at,
            nonce: options.nonce,
            database_path: options.database_path,
            tick: Duration::from_millis(400),
            codex_ttl: CODEX_STATUS_TTL,
            codex_cache: RwLock::new(None),
            codex_probe_lock: AsyncMutex::new(()),
            updates,
            stream_shutdown,
        })
    }

    /// Closes every currently connected inspector stream. Active request
    /// handlers retain the server state, so graceful shutdown alone cannot
    /// release an SSE request that is waiting for the next update.
    pub fn close_inspector_streams(&self) {
        let _ = self.stream_shutdown.send(());
    }

    /// The Codex status, probed at most once per TTL.
    pub async fn codex_status(&self) -> CodexStatus {
        if let Some(cached) = self.cached_codex() {
            return cached;
        }
        let _probe = self.codex_probe_lock.lock().await;
        if let Some(cached) = self.cached_codex() {
            return cached;
        }
        let status = self.codex.status().await;
        self.store_codex(status.clone());
        status
    }

    /// Starts a stale Codex check without putting external CLI latency on the
    /// menu request path. Repeated menu polls share the single-flight lock.
    fn refresh_codex_status_in_background(self: &Arc<Self>) {
        if self.cached_codex().is_some() {
            return;
        }
        let state = Arc::clone(self);
        tokio::spawn(async move {
            let _ = state.codex_status().await;
        });
    }

    fn cached_codex(&self) -> Option<CodexStatus> {
        let guard = self.codex_cache.read().unwrap();
        guard.as_ref().and_then(|cached| {
            (cached.at.elapsed() < self.codex_ttl).then(|| cached.status.clone())
        })
    }

    fn store_codex(&self, status: CodexStatus) {
        *self.codex_cache.write().unwrap() = Some(CachedCodex {
            at: Instant::now(),
            status,
        });
    }

    /// An install, repair, update, removal or an explicit refresh invalidates the
    /// cached status; the next reader probes once and caches the answer again.
    pub fn invalidate_codex(&self) {
        *self.codex_cache.write().unwrap() = None;
    }

    /// Cheap identity of everything a client can see. Without the runtime scan and
    /// the Codex state a client would keep rendering state that changed elsewhere.
    ///
    /// This runs on the 400 ms tick, so it only reads cached state: a Codex probe
    /// here would spawn an external CLI five times a second.
    fn stamp(&self) -> String {
        format!(
            "{}|{}|{:?}|{:?}",
            self.store.revision(),
            self.service.config().revision,
            self.service.environment(),
            self.cached_codex()
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
            // Refresh the cached status at most once per TTL, before stamping.
            let _ = self.codex_status().await;
            let current = self.stamp();
            if current == last {
                continue;
            }
            last = current;
            let codex = self.codex_status().await;
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
        Some(origin) => match origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
        {
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
    state.store.snapshot(state.codex_status().await)
}

/// Hands the service's environment to the store that projects it.
///
/// The store keeps the environment it was given, so every mutation has to
/// publish the new one. Without that, `/api/menu` and `/api/snapshot` keep
/// serving the environment the daemon started with: a profile saved in the
/// control panel reached `/api/config` — which is what the panel renders —
/// but never the menu bar, and neither the SSE revision nor the projection cache
/// key (both stamped with the profile and runtime counts) ever moved.
fn publish_environment(state: &RelayServerState) {
    state.store.set_environment(state.service.environment());
}

/// `/api/health` stays cheap on purpose: identity, the counters the Status tab
/// shows, and nothing that spawns a process. A health check must never be the
/// reason a Codex CLI starts.
async fn health(State(state): State<Arc<RelayServerState>>) -> Response {
    let (sessions, runs) = state.store.health_counts();
    Json(Health {
        ok: true,
        pid: std::process::id(),
        nonce: state.nonce.clone(),
        port: state.port.load(Ordering::SeqCst),
        started_at: state.started_at.clone(),
        version: state.version.clone(),
        database: state.database_path.clone(),
        sessions,
        runs,
    })
    .into_response()
}

async fn snapshot(State(state): State<Arc<RelayServerState>>) -> Response {
    Json(snapshot_of(&state).await).into_response()
}

async fn menu(State(state): State<Arc<RelayServerState>>) -> Response {
    state.refresh_codex_status_in_background();
    let codex = state.cached_codex().unwrap_or_else(CodexStatus::unknown);
    Json(state.store.menu(codex)).into_response()
}

async fn config(State(state): State<Arc<RelayServerState>>) -> Response {
    Json(state.service.config()).into_response()
}

async fn adapters(State(state): State<Arc<RelayServerState>>) -> Response {
    Json(AdapterCatalog {
        adapters: state.service.adapter_ids(),
    })
    .into_response()
}

fn failed(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: message.into(),
        }),
    )
        .into_response()
}

fn from_service<T: serde::Serialize>(result: Result<T, String>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(message) => failed(message),
    }
}

async fn save_profile(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
    Json(mut profile): Json<AgentProfile>,
) -> Response {
    profile.id = id;
    match state.service.save_profile(profile) {
        Ok(config) => {
            publish_environment(&state);
            Json(config).into_response()
        }
        Err(message) => failed(message),
    }
}

async fn delete_profile(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.service.delete_profile(&id) {
        Ok(config) => {
            publish_environment(&state);
            Json(config).into_response()
        }
        Err(message) => failed(message),
    }
}

async fn save_policy(
    State(state): State<Arc<RelayServerState>>,
    Json(body): Json<PolicyBody>,
) -> Response {
    match state.service.save_policy(body) {
        Ok(config) => Json(config).into_response(),
        Err(message) => failed(message),
    }
}

async fn runtime_options(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    Json(state.service.runtime_options(&id).await).into_response()
}

async fn probe_runtime(
    State(state): State<Arc<RelayServerState>>,
    Json(body): Json<ProbeBody>,
) -> Response {
    if body.executable_path.is_empty() {
        return failed("Missing executablePath");
    }
    Json(
        state
            .service
            .probe(&body.adapter_id, &body.executable_path)
            .await,
    )
    .into_response()
}

async fn save_runtime(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RuntimeBody>,
) -> Response {
    if body.adapter_id.is_empty() || body.executable_path.is_empty() {
        return failed("Missing runtime body");
    }
    let mutation = state.service.save_runtime(&id, body).await;
    publish_environment(&state);
    Json(mutation).into_response()
}

async fn delete_runtime(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let config = state.service.delete_runtime(&id);
    publish_environment(&state);
    Json(config).into_response()
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
        [
            ("content-type", "text/plain; charset=utf-8"),
            ("cache-control", "no-store"),
        ],
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

/* ---------------------------- execution ---------------------------- */

async fn list_agents(
    State(state): State<Arc<RelayServerState>>,
    Json(session): Json<SessionContext>,
) -> Response {
    from_service(state.runs.list_agents(&session).await)
}

async fn start_run(
    State(state): State<Arc<RelayServerState>>,
    Json(body): Json<RunStartBody>,
) -> Response {
    from_service(state.runs.start(body).await)
}

async fn worker_status(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    from_service(state.runs.status(&id).await)
}

async fn wait_worker(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    from_service(state.runs.wait(&id).await)
}

async fn send_worker(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SendBody>,
) -> Response {
    match state.runs.send(&id, &body.message).await {
        Ok(()) => Json(serde_json::json!({ "sent": true })).into_response(),
        Err(message) => failed(message),
    }
}

/// The worker's owner is the daemon, so cancellation is a direct call: there is
/// no cross-process queue to route through and nothing to claim.
async fn cancel_worker(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.runs.cancel(&id).await {
        Ok(()) => Json(CancelResult {
            accepted: true,
            count: None,
            message: Some("Cancellation requested".to_string()),
        })
        .into_response(),
        Err(error) => Json(CancelResult {
            accepted: false,
            count: None,
            message: Some(error),
        })
        .into_response(),
    }
}

async fn cancel_session(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.runs.cancel_session(&id).await {
        Ok(count) => Json(CancelResult {
            accepted: true,
            count: Some(count),
            message: None,
        })
        .into_response(),
        Err(error) => Json(CancelResult {
            accepted: false,
            count: None,
            message: Some(error),
        })
        .into_response(),
    }
}

async fn delete_session(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.runs.delete_session(&id).await {
        Ok(()) => {
            state.store.invalidate();
            Json(serde_json::json!({ "deleted": true })).into_response()
        }
        Err(error) => failed(error),
    }
}

async fn accept_worker(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    from_service(state.runs.accept(&id).await)
}

async fn resume_worker(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<ResumeBody>,
) -> Response {
    from_service(state.runs.resume(&id, &body.feedback).await)
}

async fn sync_session(
    State(state): State<Arc<RelayServerState>>,
    Json(session): Json<SessionContext>,
) -> Response {
    from_service(state.runs.sync_session(&session).await)
}

async fn end_session(
    State(state): State<Arc<RelayServerState>>,
    Json(body): Json<EndSessionBody>,
) -> Response {
    if let Some(native_session_id) = body.native_session_id.filter(|id| !id.is_empty()) {
        return from_service(
            state
                .runs
                .end_session_by_native_id(&native_session_id)
                .await,
        );
    }
    from_service(state.runs.end_session(&body.session).await)
}

async fn codex_action(
    State(state): State<Arc<RelayServerState>>,
    AxumPath(action): AxumPath<String>,
) -> Response {
    match CodexAction::parse(&action) {
        Some(action) => {
            let result = state.codex.run(action).await;
            // The integration just changed; the next reader probes it once.
            state.invalidate_codex();
            Json(result).into_response()
        }
        None => failed(format!("Unknown Codex action: {action}")),
    }
}

async fn refresh(State(state): State<Arc<RelayServerState>>) -> Response {
    let (environment, config) = state.service.refresh().await;
    publish_environment(&state);
    state.invalidate_codex();
    Json(RefreshResult {
        runtimes: environment.runtimes.len() as u32,
        profiles: config.profiles.len() as u32,
        detected_at: relay_core::now(),
    })
    .into_response()
}

const MIME: [(&str, &str); 12] = [
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
    ("wasm", "application/wasm"),
];

fn mime_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    MIME.iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime)| *mime)
        .unwrap_or("application/octet-stream")
}

/// Serves the built UI. `/panel/*` is the control panel: the same bundle, so its
/// API calls pass the Origin guard without special cases.
async fn serve_static(state: &RelayServerState, pathname: &str) -> Response {
    let is_panel = pathname == "/panel" || pathname.starts_with("/panel/");
    let root = if is_panel {
        &state.panel_root
    } else {
        &state.web_root
    };
    let relative = if is_panel {
        pathname
            .trim_start_matches("/panel")
            .trim_start_matches('/')
    } else {
        pathname.trim_start_matches('/')
    };
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let candidate = root.join(relative);
    let inside = candidate.starts_with(root);
    let target = if inside {
        candidate
    } else {
        root.join("index.html")
    };

    match tokio::fs::read(&target).await {
        Ok(body) => (
            StatusCode::OK,
            [
                ("content-type", mime_for(&target)),
                ("cache-control", "no-store"),
            ],
            body,
        )
            .into_response(),
        Err(_) => {
            // Single-page fallback for a client-side route.
            if pathname != "/" && Path::new(pathname).extension().is_none() {
                if let Ok(body) = tokio::fs::read(root.join("index.html")).await {
                    return (
                        StatusCode::OK,
                        [
                            ("content-type", "text/html; charset=utf-8"),
                            ("cache-control", "no-store"),
                        ],
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
    let mut shutdown = state.stream_shutdown.subscribe();
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
            let update = tokio::select! {
                _ = shutdown.recv() => break,
                update = updates.recv() => update,
            };
            match update {
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

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/* ------------------------------------------------------------------ */
/* Router                                                              */
/* ------------------------------------------------------------------ */

pub fn router(state: Arc<RelayServerState>) -> Router {
    // Routes carry their own `/api` prefix instead of being nested: a nested
    // router rewrites the path, and the 404 for an unknown API route has to
    // report what the caller actually asked for.
    Router::new()
        .route("/api/health", get(health))
        .route("/api/snapshot", get(snapshot))
        .route("/api/menu", get(menu))
        .route("/api/config", get(config))
        .route(
            "/api/config/profiles/{id}",
            put(save_profile).delete(delete_profile),
        )
        .route("/api/config/policy", put(save_policy))
        .route(
            "/api/config/runtimes/{id}",
            put(save_runtime).delete(delete_runtime),
        )
        .route("/api/adapters", get(adapters))
        .route("/api/runtimes/probe", post(probe_runtime))
        .route("/api/runtimes/{id}/options", get(runtime_options))
        .route("/api/diagnostics", get(diagnostics))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/agents", post(list_agents))
        .route("/api/runs", post(start_run))
        .route("/api/workers/{id}", get(worker_status))
        .route("/api/workers/{id}/wait", post(wait_worker))
        .route("/api/workers/{id}/send", post(send_worker))
        .route("/api/workers/{id}/cancel", post(cancel_worker))
        .route("/api/workers/{id}/accept", post(accept_worker))
        .route("/api/workers/{id}/resume", post(resume_worker))
        .route("/api/sessions/sync", post(sync_session))
        .route("/api/sessions/end", post(end_session))
        .route("/api/sessions/{id}", delete(delete_session))
        .route("/api/sessions/{id}/cancel", post(cancel_session))
        .route("/api/codex/{action}", post(codex_action))
        .route("/api/refresh", post(refresh))
        .route("/api/stream", get(stream))
        // An unknown API route answers as an API, never as the SPA.
        .route("/api/{*rest}", axum::routing::any(unknown_api_route))
        .fallback(get(fallback))
        .with_state(Arc::clone(&state))
        .layer(axum::middleware::from_fn_with_state(state, guard))
}

async fn fallback(State(state): State<Arc<RelayServerState>>, uri: Uri) -> Response {
    serve_static(&state, uri.path()).await
}

async fn unknown_api_route(uri: Uri) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: format!("Unknown Relay API route: {}", uri.path()),
        }),
    )
        .into_response()
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
    if request.uri().path().starts_with("/api/")
        && !authorized(&state, request.headers(), request.uri())
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: "Missing or invalid Relay token".into(),
            }),
        )
            .into_response();
    }
    next.run(request).await
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
    fn wasm_assets_use_the_browser_required_mime_type() {
        assert_eq!(mime_for(Path::new("relay-ui_bg.wasm")), "application/wasm");
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

    #[tokio::test]
    async fn closing_inspector_streams_wakes_connected_sse_handlers() {
        let state = test_state();
        let mut shutdown = state.stream_shutdown.subscribe();

        state.close_inspector_streams();

        tokio::time::timeout(Duration::from_millis(100), shutdown.recv())
            .await
            .expect("an open inspector stream must be released during shutdown")
            .expect("the shutdown notification must be delivered");
    }

    pub(crate) fn test_state() -> Arc<RelayServerState> {
        use relay_storage::{Database, SqliteEventStore, SqliteHostSessionStore};
        static NEXT_TEST_DATABASE: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let database_id = NEXT_TEST_DATABASE.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "relay-api-test-{}-{database_id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let database = Arc::new(Database::open(directory.join("relay.sqlite")).unwrap());
        let store = Arc::new(RelayStore::new(
            Arc::new(SqliteEventStore::new(Arc::clone(&database))),
            Arc::new(SqliteHostSessionStore::new(Arc::clone(&database))),
        ));
        RelayServerState::new(RelayServerOptions {
            store,
            service: Arc::new(UnavailableService),
            runs: Arc::new(UnavailableRuns),
            codex: Arc::new(NoCodex),
            web_root: directory.join("web"),
            panel_root: directory.join("panel"),
            token: "secret".to_string(),
            port: Arc::new(AtomicU16::new(7352)),
            version: "0.1.0".to_string(),
            started_at: relay_core::now(),
            nonce: "nonce".to_string(),
            database_path: directory.join("relay.sqlite").display().to_string(),
        })
    }

    pub(crate) struct UnavailableService;

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
            RuntimeProbe {
                ok: false,
                version: None,
                error: Some("unsupported".into()),
            }
        }
        async fn save_runtime(&self, _id: &str, _body: RuntimeBody) -> RuntimeMutation {
            RuntimeMutation {
                config: self.config(),
                probe: RuntimeProbe {
                    ok: false,
                    version: None,
                    error: Some("unsupported".into()),
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
            Vec::new()
        }
    }

    /// A daemon without an execution engine: every run call fails loudly.
    pub(crate) struct UnavailableRuns;

    fn unavailable<T>() -> Result<T, String> {
        Err("execution is not available in this test state".to_string())
    }

    #[async_trait]
    impl RunService for UnavailableRuns {
        async fn list_agents(
            &self,
            _session: &SessionContext,
        ) -> Result<Vec<AgentProfile>, String> {
            unavailable()
        }
        async fn sync_session(&self, _session: &SessionContext) -> Result<HostSession, String> {
            unavailable()
        }
        async fn end_session(
            &self,
            _session: &SessionContext,
        ) -> Result<Option<HostSession>, String> {
            unavailable()
        }
        async fn end_session_by_native_id(
            &self,
            _native_session_id: &str,
        ) -> Result<Option<HostSession>, String> {
            unavailable()
        }
        async fn start(&self, _body: RunStartBody) -> Result<RunStarted, String> {
            unavailable()
        }
        async fn resume(
            &self,
            _worker_session_id: &str,
            _feedback: &str,
        ) -> Result<RunStarted, String> {
            unavailable()
        }
        async fn status(&self, _worker_session_id: &str) -> Result<RunProjectionView, String> {
            unavailable()
        }
        async fn wait(&self, _worker_session_id: &str) -> Result<RunProjectionView, String> {
            unavailable()
        }
        async fn send(&self, _worker_session_id: &str, _message: &str) -> Result<(), String> {
            unavailable()
        }
        async fn cancel(&self, _worker_session_id: &str) -> Result<(), String> {
            unavailable()
        }
        async fn accept(&self, _worker_session_id: &str) -> Result<RunProjectionView, String> {
            unavailable()
        }
        async fn cancel_session(&self, _host_session_id: &str) -> Result<u32, String> {
            unavailable()
        }
        async fn delete_session(&self, _host_session_id: &str) -> Result<(), String> {
            unavailable()
        }
    }

    pub(crate) struct NoCodex;

    #[async_trait]
    impl CodexIntegration for NoCodex {
        async fn status(&self) -> CodexStatus {
            CodexStatus::unknown()
        }
        async fn run(&self, _action: CodexAction) -> InstallResult {
            InstallResult {
                status: CodexStatus::unknown(),
                messages: Vec::new(),
            }
        }
    }

    struct CountingCodex {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl CodexIntegration for CountingCodex {
        async fn status(&self) -> CodexStatus {
            self.calls.fetch_add(1, Ordering::SeqCst);
            CodexStatus::unknown()
        }
        async fn run(&self, _action: CodexAction) -> InstallResult {
            InstallResult {
                status: CodexStatus::unknown(),
                messages: Vec::new(),
            }
        }
    }

    struct SlowCodex {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl CodexIntegration for SlowCodex {
        async fn status(&self) -> CodexStatus {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(250)).await;
            CodexStatus::unknown()
        }
        async fn run(&self, _action: CodexAction) -> InstallResult {
            InstallResult {
                status: CodexStatus::unknown(),
                messages: Vec::new(),
            }
        }
    }

    #[tokio::test]
    async fn menu_does_not_wait_for_slow_codex_cli_and_coalesces_background_probes() {
        let mut state = test_state();
        let codex = Arc::new(SlowCodex {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        Arc::get_mut(&mut state).unwrap().codex = codex.clone();

        let started = Instant::now();
        let response = menu(State(Arc::clone(&state))).await;
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(response.status(), StatusCode::OK);

        // Several tray polls during a single CLI probe must not start a probe
        // storm, nor should any poll block on that CLI.
        for _ in 0..10 {
            let response = menu(State(Arc::clone(&state))).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while state.cached_codex().is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the background Codex probe should eventually populate the cache");
        assert_eq!(codex.calls.load(Ordering::SeqCst), 1);
    }

    /// A server state whose Codex integration counts every real probe, with the
    /// client connection the ticker needs to have somebody to publish to.
    #[allow(clippy::type_complexity)]
    fn counting_state(
        ttl: Duration,
        tick: Duration,
    ) -> (
        tempfile::TempDir,
        Arc<RelayServerState>,
        Arc<CountingCodex>,
        tokio::sync::broadcast::Receiver<Arc<InspectorSnapshot>>,
    ) {
        use relay_storage::{Database, SqliteEventStore, SqliteHostSessionStore};
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(Database::open(directory.path().join("relay.sqlite")).unwrap());
        let store = Arc::new(RelayStore::new(
            Arc::new(SqliteEventStore::new(Arc::clone(&database))),
            Arc::new(SqliteHostSessionStore::new(Arc::clone(&database))),
        ));
        let codex = Arc::new(CountingCodex {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut state = RelayServerState::new(RelayServerOptions {
            store,
            service: Arc::new(UnavailableService),
            runs: Arc::new(UnavailableRuns),
            codex: codex.clone(),
            web_root: directory.path().join("web"),
            panel_root: directory.path().join("panel"),
            token: "secret".to_string(),
            port: Arc::new(AtomicU16::new(7353)),
            version: "0.1.0".to_string(),
            started_at: relay_core::now(),
            nonce: "nonce".to_string(),
            database_path: directory.path().join("relay.sqlite").display().to_string(),
        });
        let client = {
            let state = Arc::get_mut(&mut state).expect("the state has one owner here");
            state.codex_ttl = ttl;
            state.tick = tick;
            // A connected client is what makes the ticker work at all.
            state.updates.subscribe()
        };
        (directory, state, codex, client)
    }

    #[tokio::test]
    async fn an_idle_tick_does_not_probe_the_codex_integration() {
        let (_directory, state, codex, _client) =
            counting_state(CODEX_STATUS_TTL, Duration::from_millis(400));
        // 50 ticks at 400 ms is the "leave the inspector open" case.
        for _ in 0..50 {
            let _ = state.codex_status().await;
        }
        assert_eq!(
            codex.calls.load(Ordering::SeqCst),
            1,
            "a cached Codex status must be reused instead of re-probed on every tick"
        );
    }

    /// The SSE ticker is the one automatic reader of the Codex status, so it is
    /// the one place a probe storm could hide: however fast it ticks, a real probe
    /// happens at most once per TTL.
    #[tokio::test]
    async fn the_ticker_probes_the_codex_integration_at_most_once_per_ttl() {
        const TTL: Duration = Duration::from_millis(150);
        const WINDOW: Duration = Duration::from_millis(700);
        let (_directory, state, codex, _client) = counting_state(TTL, Duration::from_millis(10));

        let ticker = tokio::spawn(Arc::clone(&state).run_ticker());
        tokio::time::sleep(WINDOW).await;
        ticker.abort();

        let calls = codex.calls.load(Ordering::SeqCst);
        let allowed = (WINDOW.as_millis() / TTL.as_millis()) as usize + 1;
        assert!(calls >= 1, "the ticker must refresh the cached status");
        assert!(
            calls <= allowed,
            "the ticker probed {calls} times in {} ms with a {} ms TTL; at most {allowed} probes are allowed",
            WINDOW.as_millis(),
            TTL.as_millis()
        );
    }
}
