//! The desktop shell: state, polling, dispatch and app lifecycle.
//!
//! Ported from `apps/menu-bar/src/main.ts`. There is no application window and no
//! inspector HTML here — the menu carries status and quick actions, the panel
//! carries the forms, and clicking a session opens the log viewer in
//! Edge/Chrome. Everything the tray shows or changes goes through relayd.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

use crate::api::{ApiError, RelayClient};
use crate::daemon::{self, DaemonProbe, NO_AUTOSTART_ENV};
use crate::i18n::Locale;
use crate::menu_model::{
    build_menu_bar_items, menu_bar_status_label, DaemonStatus, MenuBarAction, MenuBarPlatform,
    MenuBarView, VersionMismatch,
};
use crate::panel::{self, PanelState, PanelTarget};
use crate::tray;
use crate::updater::{self, UpdateState, UpdateStore};

/// Matches the inspector's poll; both read the same cached projection server-side.
const POLL_MS: u64 = 2_000;
/// How long "starting" may last before the menu admits the daemon did not come up.
const START_TIMEOUT_MS: u64 = 60_000;

/// Everything the shell remembers between refreshes.
struct ShellState {
    probe: DaemonProbe,
    starting_since: Option<Instant>,
    /// Last failure worth showing in the menu (spawn, panel, MCP action).
    last_error: Option<String>,
    /// When that failure was recorded, so a healthy probe cannot wipe it before
    /// the menu has had a chance to show it.
    last_error_at: Option<Instant>,
    /// True when this process launched the running daemon. Only that daemon is
    /// stopped on quit: a daemon the user started themselves keeps serving.
    started_by_us: bool,
    /// Serialized previous model, so an unchanged menu is not rebuilt.
    last_model: String,
    busy: bool,
    locale: Locale,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            probe: DaemonProbe::default(),
            starting_since: None,
            last_error: None,
            last_error_at: None,
            started_by_us: false,
            last_model: String::new(),
            busy: false,
            locale: Locale::En,
        }
    }
}

/// The one piece of managed state the menu is a projection of.
#[derive(Default)]
pub struct Shell {
    inner: Mutex<ShellState>,
}

fn with_state<R>(app: &AppHandle, f: impl FnOnce(&mut ShellState) -> R) -> R {
    let shell = app.state::<Shell>();
    let mut guard = shell
        .inner
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    f(&mut guard)
}

/// The status the menu shows: "starting" only lasts `START_TIMEOUT_MS`.
fn current_status(probe: DaemonStatus, starting_since: Option<Instant>) -> DaemonStatus {
    if probe == DaemonStatus::Running {
        return DaemonStatus::Running;
    }
    match starting_since {
        Some(since) if since.elapsed() < Duration::from_millis(START_TIMEOUT_MS) => {
            DaemonStatus::Starting
        }
        _ => DaemonStatus::Stopped,
    }
}

/// What the shell knows that is not the daemon probe: the platform's menu shape
/// and the two OS-level switches the menu renders.
struct ShellSwitches {
    platform: MenuBarPlatform,
    launch_at_login: bool,
    update: UpdateState,
    browser: String,
}

/// The menu model: a projection of the probe plus the OS-level switches, so it can
/// be compared as a string and only rebuilt when it really changed.
fn build_view(
    locale: Locale,
    status: DaemonStatus,
    probe: &DaemonProbe,
    last_error: Option<&str>,
    switches: ShellSwitches,
) -> MenuBarView {
    let ShellSwitches {
        platform,
        launch_at_login,
        update,
        browser,
    } = switches;
    MenuBarView {
        locale: locale.as_str().to_string(),
        platform,
        daemon: status,
        menu: match status {
            DaemonStatus::Running => probe.menu.clone(),
            _ => None,
        },
        // A failure the shell caused outranks why the daemon is unreachable.
        error: last_error
            .map(str::to_string)
            .or_else(|| probe.error.clone()),
        // An app update replaces the bundle but not the running daemon: say so.
        daemon_version_mismatch: match (&probe.running_version, &probe.app_version) {
            (Some(running), Some(app)) if probe.version_mismatch => Some(VersionMismatch {
                running: running.clone(),
                app: app.clone(),
            }),
            _ => None,
        },
        update: Some(update),
        launch_at_login,
        browser,
        max_sessions: None,
        max_workers: None,
    }
}

/// Rebuilds the native menu when anything it shows changed.
pub async fn refresh(app: &AppHandle, force: bool) {
    if with_state(app, |state| {
        if state.busy {
            true
        } else {
            state.busy = true;
            false
        }
    }) {
        return;
    }

    let app_version = app.package_info().version.to_string();
    let probe = daemon::probe(&app_version).await;
    let launch_at_login = app.autolaunch().is_enabled().unwrap_or(false);
    let update = app.state::<UpdateStore>().state();

    let model = with_state(app, |state| {
        state.probe = probe;
        if state.probe.status == DaemonStatus::Running {
            state.starting_since = None;
            // An MCP action reports through the same field and calls for a refresh
            // immediately, so the error has to survive at least one poll interval
            // or nobody would ever see it.
            let stale = state
                .last_error_at
                .map(|at| at.elapsed() >= Duration::from_millis(POLL_MS))
                .unwrap_or(true);
            if stale {
                state.last_error = None;
                state.last_error_at = None;
            }
        }
        build_view(
            state.locale,
            current_status(state.probe.status, state.starting_since),
            &state.probe,
            state.last_error.as_deref(),
            ShellSwitches {
                platform: MenuBarPlatform::current(),
                launch_at_login,
                update,
                browser: daemon::browser_name(),
            },
        )
    });

    let serialized = model.serialized();
    let changed = with_state(app, |state| {
        state.busy = false;
        if !force && serialized == state.last_model {
            return false;
        }
        state.last_model = serialized;
        true
    });
    if !changed {
        return;
    }

    let tooltip = menu_bar_status_label(&model);
    let items = build_menu_bar_items(&model);
    if let Err(error) = tray::apply(app, &items, &tooltip) {
        eprintln!("[relay] failed to update the menu: {error}");
    }
}

/// Schedules a forced refresh from synchronous code (menu actions, update
/// progress, the single-instance callback).
pub fn refresh_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { refresh(&app, true).await });
}

fn notify(app: &AppHandle) -> impl Fn() + Send + Sync + 'static {
    let app = app.clone();
    move || refresh_now(&app)
}

fn set_last_error(app: &AppHandle, error: Option<String>) {
    with_state(app, |state| {
        state.last_error = error;
        state.last_error_at = Some(Instant::now());
    });
}

fn mark_starting(app: &AppHandle) -> bool {
    let started_at = Instant::now();
    let locale = with_state(app, |state| {
        if !can_begin_start(state.starting_since) {
            return None;
        }
        state.starting_since = Some(started_at);
        Some(state.locale)
    });
    let Some(locale) = locale else {
        return false;
    };

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(START_TIMEOUT_MS)).await;
        let timed_out = with_state(&handle, |state| {
            if state.starting_since == Some(started_at)
                && state.probe.status != DaemonStatus::Running
            {
                state.starting_since = None;
                true
            } else {
                false
            }
        });
        if timed_out {
            let message = match locale {
                Locale::ZhCn => format!(
                    "日志服务 60 秒内未就绪；查看日志：{}",
                    daemon::daemon_log_path().display()
                ),
                _ => format!(
                    "Log service did not become ready in 60 seconds; see {}",
                    daemon::daemon_log_path().display()
                ),
            };
            set_last_error(&handle, Some(message));
            refresh(&handle, true).await;
        }
    });
    true
}

fn can_begin_start(starting_since: Option<Instant>) -> bool {
    !starting_since.is_some_and(|since| since.elapsed() < Duration::from_millis(START_TIMEOUT_MS))
}

/// Starts the daemon and remembers that this process owns it, so quitting stops
/// it and a daemon the user started themselves keeps serving.
fn start_daemon(app: &AppHandle) -> Option<String> {
    match daemon::start_daemon() {
        Ok(()) => {
            with_state(app, |state| state.started_by_us = true);
            None
        }
        Err(error) => Some(error),
    }
}

fn client(app: &AppHandle) -> Option<RelayClient> {
    with_state(app, |state| state.probe.info.clone())
        .map(|info| RelayClient::new(info.url, info.token))
}

fn info(app: &AppHandle) -> Option<relay_api::server_info::ServerInfo> {
    with_state(app, |state| state.probe.info.clone())
}

fn open_url_in_browser(app: &AppHandle, url: &str) {
    if daemon::open_in_browser(url) {
        return;
    }
    // `open` could not even be spawned; the opener plugin is the fallback.
    if let Err(error) = app.opener().open_url(url, None::<&str>) {
        eprintln!("[relay] open failed: {error}");
    }
}

fn open_inspector(app: &AppHandle, host_session_id: Option<&str>, run_id: Option<&str>) {
    let Some(info) = info(app) else {
        return;
    };
    open_url_in_browser(app, &info.inspector_url(host_session_id, run_id));
}

fn open_control_panel(
    app: &AppHandle,
    tab: crate::menu_model::PanelTab,
    intent: Option<crate::menu_model::PanelIntent>,
    profile_id: Option<String>,
) {
    let Some(info) = info(app) else {
        set_last_error(app, Some("daemon 未运行，无法打开配置面板".to_string()));
        refresh_now(app);
        return;
    };
    let target = PanelTarget {
        base: info.url,
        token: info.token,
        lang: with_state(app, |state| state.locale.as_str().to_string()),
        tab: Some(tab),
        intent,
        profile_id,
    };
    if let Err(error) = panel::open(app, target) {
        set_last_error(app, Some(error));
        refresh_now(app);
    }
}

/// A client action's boxed future: the shell runs it off the menu thread and
/// reports whatever the daemon said into the menu.
type ClientAction = Pin<Box<dyn Future<Output = Result<Option<String>, ApiError>> + Send>>;

/// Runs an MCP-side action (Codex lifecycle, rescan, cancel) and surfaces the
/// outcome.
fn run_client_action<F>(app: &AppHandle, label: &str, action: F)
where
    F: FnOnce(RelayClient) -> ClientAction + Send + 'static,
{
    let Some(client) = client(app) else {
        set_last_error(app, Some(format!("{label}: daemon 未运行")));
        refresh_now(app);
        return;
    };
    let app = app.clone();
    let label = label.to_string();
    tauri::async_runtime::spawn(async move {
        let message = match action(client).await {
            Ok(message) => message,
            Err(error) => Some(format!("{label}: {error}")),
        };
        set_last_error(&app, message);
        refresh(&app, true).await;
    });
}

/// Performs a menu action. The tray never decides which agent should run: every
/// arm here is either an HTTP call or an OS-level switch.
pub fn dispatch(app: &AppHandle, action: MenuBarAction) {
    match action {
        MenuBarAction::OpenInspector {
            host_session_id,
            run_id,
        } => {
            open_inspector(app, host_session_id.as_deref(), run_id.as_deref());
        }
        MenuBarAction::OpenPanel {
            tab,
            intent,
            profile_id,
        } => {
            open_control_panel(app, tab, intent, profile_id);
        }
        MenuBarAction::Rescan => {
            run_client_action(app, "rescan", |client| {
                Box::pin(async move { client.refresh().await.map(|_| None) })
            });
        }
        MenuBarAction::RepairCodex => {
            run_client_action(app, "codex repair", |client| {
                Box::pin(async move {
                    client
                        .codex_repair()
                        .await
                        .map(|result| result.messages.into_iter().next())
                })
            });
        }
        MenuBarAction::StartDaemon => {
            if !mark_starting(app) {
                return;
            }
            let error = start_daemon(app);
            if error.is_some() {
                with_state(app, |state| state.starting_since = None);
            }
            set_last_error(app, error);
            refresh_now(app);
        }
        MenuBarAction::RestartDaemon => {
            if !mark_starting(app) {
                return;
            }
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                // Only a daemon whose identity was verified is ever signalled.
                // Wait for its PID to leave before starting, because relayd may
                // drain SSE clients for up to 1.5 s.
                let verified = with_state(&handle, |state| state.probe.verified);
                if verified {
                    if let Some(info) = info(&handle) {
                        daemon::stop_daemon_and_wait(&info).await;
                    }
                }
                let error = start_daemon(&handle);
                if error.is_some() {
                    with_state(&handle, |state| state.starting_since = None);
                }
                set_last_error(&handle, error);
                refresh(&handle, true).await;
            });
            refresh_now(app);
        }
        MenuBarAction::CancelWorker { worker_session_id } => {
            run_client_action(app, "cancel", move |client| {
                Box::pin(
                    async move { client.cancel_worker(&worker_session_id).await.map(|_| None) },
                )
            });
        }
        MenuBarAction::CancelSession { host_session_id } => {
            run_client_action(app, "cancel session", move |client| {
                Box::pin(async move { client.cancel_session(&host_session_id).await.map(|_| None) })
            });
        }
        MenuBarAction::OpenWorkspace { cwd } => {
            if let Err(error) = app.opener().open_path(cwd, None::<&str>) {
                eprintln!("[relay] open failed: {error}");
            }
        }
        MenuBarAction::CopySessionId { host_session_id } => {
            if let Err(error) = app.clipboard().write_text(host_session_id) {
                eprintln!("[relay] clipboard failed: {error}");
            }
        }
        MenuBarAction::CopyDiagnostics => {
            let Some(client) = client(app) else {
                set_last_error(app, Some("diagnostics: daemon 未运行".to_string()));
                refresh_now(app);
                return;
            };
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                match client.diagnostics().await {
                    Ok(report) => {
                        if let Err(error) = handle.clipboard().write_text(report) {
                            eprintln!("[relay] clipboard failed: {error}");
                        }
                        set_last_error(&handle, None);
                    }
                    Err(error) => set_last_error(&handle, Some(format!("diagnostics: {error}"))),
                }
                refresh(&handle, true).await;
            });
        }
        // Parity with the TypeScript union: the tray never emits this, and the
        // the switch has no case for it either.
        MenuBarAction::InstallCodex => {}
        MenuBarAction::Refresh => refresh_now(app),
        MenuBarAction::CheckUpdates => {
            let handle = app.clone();
            let change = notify(app);
            tauri::async_runtime::spawn(async move {
                updater::check(&handle, &change).await;
                refresh(&handle, true).await;
            });
        }
        MenuBarAction::DownloadUpdate => {
            let handle = app.clone();
            let change = notify(app);
            tauri::async_runtime::spawn(async move {
                updater::download(&handle, &change).await;
                refresh(&handle, true).await;
            });
        }
        MenuBarAction::InstallUpdate => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                // The updater replaces the bundle on quit; the daemon we started
                // must go first, or the new app would keep talking to the old
                // daemon. This action is an explicit restart, so even a
                // manually-started verified daemon is stopped here; ordinary App
                // quit still preserves user-owned daemons.
                let verified = with_state(&handle, |state| state.probe.verified);
                if verified {
                    if let Some(info) = info(&handle) {
                        daemon::stop_daemon_and_wait(&info).await;
                    }
                }
                // The record is dropped so the exit path cannot signal a PID that
                // has already been reused.
                with_state(&handle, |state| state.probe = DaemonProbe::default());
                match updater::install(&handle) {
                    // macOS: the plugin installs in place, so Relay relaunches it.
                    Ok(()) => handle.restart(),
                    Err(error) => {
                        set_last_error(&handle, Some(error));
                        refresh(&handle, true).await;
                    }
                }
            });
        }
        MenuBarAction::ToggleLaunchAtLogin => {
            let enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let result = if enabled {
                app.autolaunch().disable()
            } else {
                app.autolaunch().enable()
            };
            if let Err(error) = result {
                set_last_error(app, Some(error.to_string()));
            }
            refresh_now(app);
        }
        MenuBarAction::Quit => app.exit(0),
    }
}

/// A double click opens the inspector for the first session.
pub fn dispatch_double_click(app: &AppHandle) {
    let session = with_state(app, |state| {
        state
            .probe
            .menu
            .as_ref()
            .and_then(|menu| menu.sessions.first().map(|session| session.id.clone()))
    });
    dispatch(
        app,
        MenuBarAction::OpenInspector {
            host_session_id: session,
            run_id: None,
        },
    );
}

/// A second launch shows the panel instead of starting a second tray.
pub fn show_panel(app: &AppHandle) {
    dispatch(
        app,
        MenuBarAction::OpenPanel {
            tab: crate::menu_model::PanelTab::Agents,
            intent: None,
            profile_id: None,
        },
    );
}

/// The tray owns the process lifetime, so quitting is the only place the daemon
/// this app started is stopped.
pub fn on_exit(app: &AppHandle) {
    let (verified, started_by_us, info) = with_state(app, |state| {
        (
            state.probe.verified,
            state.started_by_us,
            state.probe.info.clone(),
        )
    });
    if verified && started_by_us {
        if let Some(info) = info {
            daemon::stop_daemon(&info);
        }
    }
}

/// Startup: first paint, one update check, bring the daemon up, then poll.
async fn start(app: AppHandle) {
    with_state(&app, |state| state.locale = crate::i18n::system_locale());
    refresh(&app, true).await;

    // relayd is this app's own backend, not a service the user has to remember to
    // start: bring it up once at launch unless something is already answering.
    // This runs before the update check, which talks to the network.
    let running = with_state(&app, |state| state.probe.status == DaemonStatus::Running);
    let autostart_disabled = std::env::var(NO_AUTOSTART_ENV)
        .map(|value| value == "1")
        .unwrap_or(false);
    if !running && !autostart_disabled {
        let _ = mark_starting(&app);
        let error = start_daemon(&app);
        if error.is_some() {
            with_state(&app, |state| state.starting_since = None);
        }
        set_last_error(&app, error);
        refresh(&app, true).await;
    }

    // Check once at launch, like any other Mac app; the menu shows the result.
    // Without a feed the status is `unsupported`, which renders nothing.
    {
        let handle = app.clone();
        let change = notify(&app);
        tauri::async_runtime::spawn(async move {
            updater::check(&handle, &change).await;
            refresh(&handle, true).await;
        });
    }

    loop {
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
        refresh(&app, false).await;
    }
}

/// Builds and runs the app. The only surface is the menu bar item.
pub fn run() {
    tauri::Builder::default()
        // First, as the plugin requires: a second launch must reach the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_panel(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Shell::default())
        .manage(PanelState::default())
        .manage(UpdateStore::default())
        .on_menu_event(|app, event| {
            if let Some(action) = tray::parse_action(event.id().as_ref()) {
                dispatch(app, action);
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            // No window means no Dock tile and no app menu: the menu bar is the app.
            #[cfg(target_os = "macos")]
            {
                let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            if let Err(error) = tray::build(&handle) {
                eprintln!("[relay] {error}; Relay cannot show a menu bar item");
                handle.exit(0);
                return Ok(());
            }
            tauri::async_runtime::spawn(start(handle));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Relay could not start")
        .run(|app, event| match event {
            // The tray owns the process lifetime; closing the panel must not quit.
            RunEvent::ExitRequested { code, api, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            // The panel behaves like a popover: clicking anywhere else dismisses it.
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::Focused(false),
                ..
            } if label == panel::PANEL_LABEL => panel::hide(app),
            RunEvent::Exit => on_exit(app),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_api::{CodexStatus, MenuSession, MenuStatus, MenuView, MenuWorker};

    fn menu(status: MenuStatus, running_workers: u32) -> MenuView {
        MenuView {
            status,
            running_workers,
            awaiting_host: 0,
            sessions: vec![MenuSession {
                id: "codex:s1".into(),
                display_name: "Session 1".into(),
                cwd: "/tmp/one".into(),
                active_workers: Vec::<MenuWorker>::new(),
            }],
            agents: Vec::new(),
            runtimes: Vec::new(),
            codex: CodexStatus::unknown(),
        }
    }

    /// The OS-level half of the model, with the defaults the tests care about.
    fn switches() -> ShellSwitches {
        ShellSwitches {
            platform: MenuBarPlatform::Darwin,
            launch_at_login: false,
            update: UpdateState::default(),
            browser: "Google Chrome".into(),
        }
    }

    #[test]
    fn starting_lasts_one_minute_and_no_longer() {
        assert_eq!(
            current_status(DaemonStatus::Running, None),
            DaemonStatus::Running
        );
        // A verified daemon outranks a start that is still in flight.
        assert_eq!(
            current_status(DaemonStatus::Running, Some(Instant::now())),
            DaemonStatus::Running
        );
        assert_eq!(
            current_status(DaemonStatus::Stopped, Some(Instant::now())),
            DaemonStatus::Starting
        );
        let long_ago = Instant::now()
            .checked_sub(Duration::from_millis(START_TIMEOUT_MS + 1))
            .expect("monotonic clock is older than 20 s in this session");
        assert_eq!(
            current_status(DaemonStatus::Stopped, Some(long_ago)),
            DaemonStatus::Stopped
        );
        assert_eq!(
            current_status(DaemonStatus::Stopped, None),
            DaemonStatus::Stopped
        );
    }

    #[test]
    fn a_pending_start_rejects_duplicate_menu_actions() {
        assert!(!can_begin_start(Some(Instant::now())));
        assert!(can_begin_start(None));

        let timed_out = Instant::now()
            .checked_sub(Duration::from_millis(START_TIMEOUT_MS + 1))
            .unwrap();
        assert!(can_begin_start(Some(timed_out)));
    }

    #[test]
    fn the_view_prefers_the_shell_error_over_the_daemon_error() {
        let probe = DaemonProbe {
            error: Some("connection refused".into()),
            ..DaemonProbe::default()
        };
        let view = build_view(Locale::En, DaemonStatus::Stopped, &probe, None, switches());
        assert_eq!(view.error.as_deref(), Some("connection refused"));
        assert_eq!(view.menu, None);

        let view = build_view(
            Locale::En,
            DaemonStatus::Stopped,
            &probe,
            Some("relayd failed to start"),
            switches(),
        );
        assert_eq!(view.error.as_deref(), Some("relayd failed to start"));
    }

    #[test]
    fn a_healthy_probe_carries_the_snapshot_and_the_version_skew() {
        let mut probe = DaemonProbe {
            status: DaemonStatus::Running,
            verified: true,
            ..DaemonProbe::default()
        };
        probe.menu = Some(menu(MenuStatus::Ready, 1));
        probe.version_mismatch = true;
        probe.running_version = Some("0.1.0".into());
        probe.app_version = Some("0.2.0".into());
        let view = build_view(
            Locale::ZhCn,
            DaemonStatus::Running,
            &probe,
            None,
            ShellSwitches {
                launch_at_login: true,
                browser: "default browser".into(),
                ..switches()
            },
        );
        assert_eq!(view.locale, "zh-CN");
        assert!(view.menu.is_some());
        assert!(view.launch_at_login);
        assert_eq!(
            view.daemon_version_mismatch,
            Some(VersionMismatch {
                running: "0.1.0".into(),
                app: "0.2.0".into()
            })
        );
        assert_eq!(menu_bar_status_label(&view), "Relay · 1 个运行中");

        // Without a mismatch flag the versions are not reported as skew.
        probe.version_mismatch = false;
        let view = build_view(
            Locale::En,
            DaemonStatus::Running,
            &probe,
            None,
            ShellSwitches {
                browser: "default browser".into(),
                ..switches()
            },
        );
        assert_eq!(view.daemon_version_mismatch, None);
    }
}
