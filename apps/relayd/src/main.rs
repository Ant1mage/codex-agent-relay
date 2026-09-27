//! Relay's local daemon.
//!
//! The daemon is Relay: it owns Relay's configuration, the Codex integration
//! lifecycle, the runtime scan, the event store **and execution**. Every worker
//! process Relay starts is started here, so anything that talks to the daemon —
//! the tray, the control panel, Codex's MCP server — sees the same runs, and a
//! front-end that dies cannot take a running worker with it.
//!
//! It is deliberately a thin composition root: initialise, register, serve. No
//! business logic lives in the route handlers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_adapters::adapters;
use relay_api::server_info::{
    clear_server_info, read_server_info, write_server_info, ServerInfo, DEFAULT_PORT, HOST,
};
use relay_api::{RelayServerOptions, RelayServerState, RelayStore, RunService};
use relay_codex::{CodexAppServerThreadResolver, CodexIntegrationService};
use relay_config::{
    config_path, database_path, relay_home, relay_version, resources_dir, server_info_path,
    ConfigStore,
};
use relay_core::{EventStore, RunController};
use relay_storage::{Database, SqliteEventStore, SqliteHostSessionStore};
use std::io::Write;

use relayd::{shutdown, DaemonService, RelayEngine, RuntimeConfigReloader};

const STARTUP_LOCK_MS: u64 = 3_000;
const STALE_LOCK_MS: u64 = 10_000;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // `--version` and `--help` must answer and exit: a release check that starts
    // a daemon and waits forever is worse than no check.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("relayd {}", relay_version());
        return;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "relayd {} — Relay's local daemon\n\nUSAGE: relayd [--version] [--help]\n\nEnvironment:\n  RELAY_HOME            state directory (default ~/.relay)\n  RELAY_PORT            first port to try (default {DEFAULT_PORT})\n  RELAY_TOKEN           fixed API token instead of a random one\n  RELAY_DB_PATH         event log location\n  RELAY_CONFIG_PATH     configuration file location\n  RELAY_WEB_ROOT        built UI directory\n  RELAY_RESOURCES_DIR   packaged resources directory",
            relay_version()
        );
        return;
    }

    if let Err(error) = run().await {
        eprintln!("relayd: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    // Only a daemon that answers with the recorded nonce counts as running: a PID
    // is not identity, and a stale server.json must never block a fresh start.
    if let Some(info) = confirmed_running_daemon().await {
        println!(
            "Relay daemon is already running at {} (pid {})",
            info.url, info.pid
        );
        return Ok(());
    }
    if !acquire_startup_lock().await {
        println!("Another Relay daemon is starting; giving up this attempt.");
        return Ok(());
    }

    let version = relay_version();
    let nonce = random_token(12);
    let token = std::env::var("RELAY_TOKEN").unwrap_or_else(|_| random_token(24));
    let started_at = relay_core::now();

    let database =
        Arc::new(Database::open(database_path()).map_err(|error| error.message().to_string())?);
    let events = Arc::new(SqliteEventStore::new(Arc::clone(&database)));
    let sessions = Arc::new(SqliteHostSessionStore::new(Arc::clone(&database)));
    let store = Arc::new(RelayStore::new(Arc::clone(&events), Arc::clone(&sessions)));

    // Execution. The daemon starts and supervises every worker process Relay
    // owns; no other process has a RunController.
    let controller = Arc::new(RunController::new(
        Arc::clone(&events) as Arc<dyn EventStore>
    ));
    for adapter in adapters() {
        controller
            .adapters
            .register(adapter)
            .map_err(|error| error.message().to_string())?;
    }
    let config = ConfigStore::new(config_path());
    let reloader = Arc::new(RuntimeConfigReloader::new(
        Arc::clone(&controller),
        config.clone(),
    ));
    reloader
        .refresh()
        .await
        .map_err(|error| error.message().to_string())?;

    // A previous daemon may have stopped mid-run. Nothing else can finish those
    // runs, so close them now, before the UI can show them as running.
    match relay_storage::reconcile_stale_runs(&events) {
        Ok(reconciliation) => {
            if !reconciliation.resolved.is_empty() {
                tracing::warn!(
                    "converged {} run(s) left behind by a previous daemon: {}",
                    reconciliation.resolved.len(),
                    reconciliation.resolved.join(", ")
                );
            }
            for diagnostic in &reconciliation.diagnostics {
                tracing::warn!("{diagnostic}");
            }
        }
        Err(error) => tracing::error!("startup reconciliation failed: {}", error.message()),
    }

    let engine = Arc::new(RelayEngine::new(
        Arc::clone(&controller),
        Arc::clone(&sessions) as Arc<dyn relay_core::HostSessionStore>,
        Arc::new(CodexAppServerThreadResolver::new()),
        Arc::clone(&reloader),
    ));
    let runs: Arc<dyn RunService> = engine;

    let service = Arc::new(DaemonService::new(config).await);
    store.set_environment(service.environment());

    let codex: Arc<dyn relay_api::CodexIntegration> = Arc::new(CodexIntegrationService::new());

    let configured_port = parse_port();
    let port = Arc::new(AtomicU16::new(configured_port));
    let state = RelayServerState::new(RelayServerOptions {
        store,
        service: service.clone(),
        runs,
        codex: Arc::clone(&codex),
        web_root: web_root(),
        panel_root: panel_root(),
        token: token.clone(),
        port: Arc::clone(&port),
        version: version.clone(),
        started_at: started_at.clone(),
        nonce: nonce.clone(),
        database_path: database.path().to_string(),
    });

    let (listener, bound_port) = relay_api::bind(configured_port)
        .await
        .map_err(|error| error.to_string())?;
    port.store(bound_port, Ordering::SeqCst);

    let info = ServerInfo {
        pid: std::process::id(),
        port: bound_port,
        url: format!("http://{HOST}:{bound_port}"),
        token: token.clone(),
        nonce: nonce.clone(),
        started_at: started_at.clone(),
        version: version.clone(),
        database: database.path().to_string(),
    };
    write_server_info(&server_info_path(), &info).map_err(|error| error.to_string())?;

    let environment = service.environment();
    println!(
        "Relay daemon {version} listening on {}\n  inspector  {}\n  database   {}\n  runtimes   {} detected, {} profiles",
        info.url,
        info.inspector_url(None, None),
        database.path(),
        environment.runtimes.len(),
        environment.profiles.len(),
    );

    tokio::spawn(Arc::clone(&state).run_ticker());

    let router = relay_api::router(Arc::clone(&state));
    let server = axum::serve(listener, router).with_graceful_shutdown(shutdown_signal());
    let result = server.await;

    // The daemon is the parent of every worker process: they end with it.
    shutdown(&controller).await;
    clear_server_info(&server_info_path(), std::process::id());
    let _ = std::fs::remove_file(relay_home().join("daemon.lock"));
    result.map_err(|error| error.to_string())
}

fn parse_port() -> u16 {
    std::env::var("RELAY_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_PORT)
}

/// Where the built inspector lives. The daemon serves it, so the tray can open
/// the same URL in a browser.
fn web_root() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(explicit) = std::env::var("RELAY_WEB_ROOT") {
        candidates.push(PathBuf::from(explicit));
    }
    if let Some(resources) = resources_dir() {
        candidates.push(resources.join("ui"));
        candidates.push(resources.join("web"));
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            candidates.push(directory.join("ui"));
            candidates.push(directory.join("../../../apps/relay-desktop/ui/dist"));
        }
    }
    let fallback =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/relay-desktop/ui/dist");
    candidates.push(fallback.clone());
    candidates
        .into_iter()
        .find(|candidate| candidate.join("index.html").is_file())
        .unwrap_or(fallback)
}

/// The control panel is the same bundle: `/panel` is a route inside it.
fn panel_root() -> PathBuf {
    if let Ok(explicit) = std::env::var("RELAY_PANEL_ROOT") {
        return PathBuf::from(explicit);
    }
    web_root()
}

async fn confirmed_running_daemon() -> Option<ServerInfo> {
    let info = read_server_info(&server_info_path())?;
    let url = format!("{}/api/health?token={}", info.url, info.token);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let response = client.get(url).send().await.ok()?;
    let health: relay_api::Health = response.json().await.ok()?;
    if health.pid == info.pid && !info.nonce.is_empty() && health.nonce == info.nonce {
        Some(info)
    } else {
        tracing::warn!("server.json does not match the daemon that answered");
        None
    }
}

/// Serialises concurrent starts. Two daemons launched at the same moment would
/// otherwise both pass the check and the later one would steal server.json.
async fn acquire_startup_lock() -> bool {
    let lock = relay_home().join("daemon.lock");
    if let Some(parent) = lock.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let deadline = Instant::now() + Duration::from_millis(STARTUP_LOCK_MS);
    while Instant::now() < deadline {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut file) => {
                let _ = file.write_all(std::process::id().to_string().as_bytes());
                return true;
            }
            Err(_) => {
                if let Ok(metadata) = std::fs::metadata(&lock) {
                    if let Ok(modified) = metadata.modified() {
                        if modified
                            .elapsed()
                            .map(|age| age.as_millis() > STALE_LOCK_MS as u128)
                            .unwrap_or(false)
                        {
                            let _ = std::fs::remove_file(&lock);
                            continue;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }
    false
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut buffer))
        .is_err()
    {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = ((seed >> (index % 16)) & 0xff) as u8 ^ (index as u8);
        }
    }
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    println!("\nRelay daemon stopping");
}
