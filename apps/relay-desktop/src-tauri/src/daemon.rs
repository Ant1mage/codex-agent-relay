//! Everything the shell knows about the daemon.
//!
//! Ported from `apps/menu-bar/src/daemon.ts`. The tray is a client: it reads
//! `~/.relay/server.json`, asks the daemon for the projection, and can start the
//! daemon and open the inspector or the panel — it never touches the database.
//!
//! Liveness is never taken from a PID. They get reused, so both the restart path
//! and the quit path require the daemon to answer with the nonce recorded in
//! `server.json` before anything is trusted or signalled.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use relay_api::server_info::{is_process_alive, read_server_info, ServerInfo};
use relay_api::{Health, MenuView};

use crate::api::RelayClient;
use crate::menu_model::DaemonStatus;

/// Set to `1` to keep the shell from starting relayd by itself.
pub const NO_AUTOSTART_ENV: &str = "RELAY_NO_AUTOSTART";
/// Absolute path to a relayd binary, instead of the bundled/detected one.
pub const DAEMON_COMMAND_ENV: &str = "RELAY_DAEMON_COMMAND";
/// Browser the inspector opens in, instead of the first installed Chromium.
pub const BROWSER_ENV: &str = "RELAY_BROWSER";
/// Where the packaged resources live; passed on to the daemon we spawn.
pub const RESOURCES_DIR_ENV: &str = "RELAY_RESOURCES_DIR";

/// How long a verified daemon is given to exit before a restart proceeds.
/// relayd allows up to 1.5 s for SSE clients to drain, so a fixed delay would
/// otherwise start the replacement too soon: it would see the old nonce, exit,
/// and leave no daemon behind.
pub const RESTART_WAIT_MS: u64 = 3_000;
/// How often the restart waits for the PID to disappear.
pub const RESTART_POLL_MS: u64 = 50;

/// Chromium-family browsers, in the order the menu tries them.
const BROWSERS: [&str; 4] = [
    "Microsoft Edge",
    "Google Chrome",
    "Chromium",
    "Brave Browser",
];

/// What one probe learned. `verified` is the only thing that authorises a signal.
#[derive(Debug, Clone, PartialEq)]
pub struct DaemonProbe {
    pub status: DaemonStatus,
    pub info: Option<ServerInfo>,
    pub menu: Option<MenuView>,
    pub error: Option<String>,
    /// True only when the daemon answered with the nonce from server.json.
    /// Restart, "already running" and quitting all key off this: a PID on its own
    /// proves nothing.
    pub verified: bool,
    /// The daemon answered, but with a different Relay version than this app.
    /// After an update the previous version keeps running until it is restarted,
    /// and the menu has to say so rather than driving a mismatched API.
    pub version_mismatch: bool,
    pub running_version: Option<String>,
    pub app_version: Option<String>,
}

impl Default for DaemonProbe {
    fn default() -> Self {
        Self {
            status: DaemonStatus::Stopped,
            info: None,
            menu: None,
            error: None,
            verified: false,
            version_mismatch: false,
            running_version: None,
            app_version: None,
        }
    }
}

impl DaemonProbe {
    fn stopped(info: Option<ServerInfo>, error: Option<String>) -> Self {
        Self {
            status: DaemonStatus::Stopped,
            info,
            error,
            ..Self::default()
        }
    }
}

/// The identity rule, in one place so it cannot drift between the probe, the
/// restart path and quitting.
pub fn identity_verified(info: &ServerInfo, health: &Health) -> bool {
    health.pid == info.pid && !info.nonce.is_empty() && health.nonce == info.nonce
}

/// Where `server.json` lives (`RELAY_HOME` aware).
pub fn server_info_path() -> PathBuf {
    relay_config::server_info_path()
}

/// Startup output is kept with the daemon state so a failed menu-bar launch
/// has something actionable to inspect.
pub fn daemon_log_path() -> PathBuf {
    relay_config::relay_home().join("relayd.log")
}

/// The recorded server.json, or `None` when there is no usable daemon record.
pub fn read_info() -> Option<ServerInfo> {
    read_server_info(&server_info_path())
}

/// Asks the daemon what it is. The daemon is only "running" once it has proved
/// its identity.
pub async fn probe(app_version: &str) -> DaemonProbe {
    let Some(info) = read_info() else {
        return DaemonProbe::stopped(None, None);
    };
    let client = RelayClient::new(info.url.clone(), info.token.clone());
    let health = match client.health().await {
        Ok(health) => health,
        Err(error) => return DaemonProbe::stopped(Some(info), Some(error.message)),
    };
    if !identity_verified(&info, &health) {
        return DaemonProbe::stopped(
            Some(info),
            Some("server.json 与运行的 daemon 不匹配（已过期）".to_string()),
        );
    }
    // Identity is proven from here on: the daemon answered with the nonce from its
    // own record. A menu that cannot be fetched is a rendering problem, not a
    // reason to forget which process this is — quitting still has to stop it.
    let (menu, error) = match client.menu().await {
        Ok(menu) => (Some(menu), None),
        Err(error) => (None, Some(error.message)),
    };
    let version_mismatch = !app_version.is_empty() && health.version != app_version;
    DaemonProbe {
        status: if error.is_none() {
            DaemonStatus::Running
        } else {
            DaemonStatus::Stopped
        },
        info: Some(info),
        menu,
        error,
        verified: true,
        version_mismatch,
        running_version: Some(health.version),
        app_version: Some(app_version.to_string()),
    }
}

/// A cheap identity check: the record in `server.json`, confirmed by
/// `/api/health`.
///
/// `probe` applies the same rule and then fetches the menu; quitting only needs
/// to know whether there is a daemon it is allowed to signal.
pub async fn verify() -> Option<ServerInfo> {
    let info = read_info()?;
    let client = RelayClient::new(info.url.clone(), info.token.clone());
    let health = client.health().await.ok()?;
    identity_verified(&info, &health).then_some(info)
}

/// True when this process runs from an installed `.app` bundle.
pub fn is_packaged() -> bool {
    is_packaged_exe(&std::env::current_exe().unwrap_or_default())
}

pub fn is_packaged_exe(exe: &Path) -> bool {
    let path = exe.to_string_lossy();
    path.contains(".app/Contents/MacOS")
}

/// Where the runtime files live: `Contents/Resources` inside a packaged app, the
/// build tree in development. The daemon is told about it explicitly instead of
/// guessing a path relative to a bundle file.
pub fn resources_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var(RESOURCES_DIR_ENV) {
        if !explicit.is_empty() {
            return PathBuf::from(explicit);
        }
    }
    if is_packaged() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(macos) = exe.parent() {
                return macos.join("../Resources");
            }
        }
    }
    // Development: the workspace build output.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../out")
}

/// A resolved daemon launch: the program, its arguments and the extra
/// environment a packaged daemon needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Resolves the daemon to launch: the explicit override, else the `relayd`
/// binary next to the current executable (in a bundle that is
/// `Contents/MacOS/relayd`, which is where `externalBin` puts it), else
/// `resources/relayd`.
pub fn daemon_command_with(
    exe: &Path,
    packaged: bool,
    override_command: Option<&str>,
    resources: &Path,
) -> Option<DaemonCommand> {
    if let Some(command) = override_command.filter(|value| !value.is_empty()) {
        return Some(DaemonCommand {
            program: PathBuf::from(command),
            args: Vec::new(),
            env: Vec::new(),
        });
    }
    let env = if packaged {
        vec![(
            RESOURCES_DIR_ENV.to_string(),
            resources.to_string_lossy().to_string(),
        )]
    } else {
        Vec::new()
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(directory) = exe.parent() {
        candidates.push(directory.join("relayd"));
    }
    candidates.push(resources.join("relayd"));
    candidates.push(resources.join("bin").join("relayd"));
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .map(|program| DaemonCommand {
            program,
            args: Vec::new(),
            env,
        })
}

fn daemon_command() -> Option<DaemonCommand> {
    let exe = std::env::current_exe().unwrap_or_default();
    let packaged = is_packaged_exe(&exe);
    let override_command = std::env::var(DAEMON_COMMAND_ENV).ok();
    daemon_command_with(
        &exe,
        packaged,
        override_command.as_deref(),
        &resources_dir(),
    )
}

/// Starts the daemon detached, so a tray that is killed (or a terminal that
/// closes) cannot take a running worker down with it: the daemon ends on an
/// explicit stop, not with whoever happens to be its parent. Failures are
/// reported as a string instead of a panic — the menu bar is the one surface the
/// user has left.
pub fn start_daemon() -> Result<(), String> {
    let Some(command) = daemon_command() else {
        return Err("找不到 relayd 入口（既没有打包产物，也不在源码仓库里）".to_string());
    };
    let log_path = daemon_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("无法写入日志 {}：{error}", log_path.display()))?;
    let stdout = log.try_clone().map_err(|error| error.to_string())?;
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(log));
    for (name, value) in &command.env {
        process.env(name, value);
    }
    // A new session, so the daemon keeps
    // running (and keeps the event log being written) after the tray exits.
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        process.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match process.spawn() {
        Ok(mut child) => {
            // Reap the child when it eventually exits; until then this thread just
            // waits, which keeps the process table clean without holding a handle.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Only ever called for a daemon whose nonce was verified.
pub fn stop_daemon(info: &ServerInfo) {
    #[cfg(unix)]
    unsafe {
        libc::kill(info.pid as i32, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    let _ = info;
}

/// Waits for a verified daemon to actually exit, so a restart cannot race the
/// drain the old process is still doing.
pub async fn stop_daemon_and_wait(info: &ServerInfo) {
    stop_daemon(info);
    let deadline = std::time::Instant::now() + Duration::from_millis(RESTART_WAIT_MS);
    while is_process_alive(info.pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(RESTART_POLL_MS)).await;
    }
}

/// The first Chromium-family browser installed in one of `roots`.
pub fn installed_browser_in(roots: &[PathBuf]) -> Option<String> {
    for name in BROWSERS {
        for root in roots {
            if root.join(format!("{name}.app")).exists() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// The browser the inspector opens in, or `None` for the system default.
pub fn installed_browser() -> Option<String> {
    if let Ok(override_name) = std::env::var(BROWSER_ENV) {
        if !override_name.is_empty() {
            return Some(override_name);
        }
    }
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    installed_browser_in(&roots)
}

/// How the menu labels the inspector's destination.
pub fn browser_name() -> String {
    installed_browser().unwrap_or_else(|| "default browser".to_string())
}

/// Opens a new tab, preferring Chromium-family browsers over the default.
///
/// Returns false when `open` could not be spawned at all, so the caller can fall
/// back to the opener plugin. `open` cannot fail in a way that should take the
/// tray down.
pub fn open_in_browser(url: &str) -> bool {
    let mut command = Command::new("open");
    if let Some(browser) = installed_browser() {
        command.arg("-a").arg(browser);
    }
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(error) => {
            eprintln!("[relay] open failed: {error}");
            false
        }
    }
}

/// One asset from the repository (development) or the bundle (packaged).
pub fn asset_path(relative: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let resources = resources_dir();
    candidates.push(resources.join("assets").join(relative));
    candidates.push(resources.join(relative));
    // Development: the checkout the binary was built from.
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../assets")
            .join(relative),
    );
    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ServerInfo {
        ServerInfo {
            pid: 42,
            port: 7352,
            url: "http://127.0.0.1:7352".into(),
            token: "token".into(),
            nonce: "nonce".into(),
            started_at: "2026-01-01T00:00:00.000Z".into(),
            version: "0.1.0".into(),
            database: "/tmp/relay.sqlite".into(),
        }
    }

    fn health(pid: u32, nonce: &str) -> Health {
        Health {
            ok: true,
            pid,
            nonce: nonce.into(),
            port: 7352,
            started_at: "2026-01-01T00:00:00.000Z".into(),
            version: "0.1.0".into(),
            database: "/tmp/relay.sqlite".into(),
            sessions: 0,
            runs: 0,
        }
    }

    #[test]
    fn only_the_recorded_nonce_proves_identity() {
        let recorded = info();
        assert!(identity_verified(&recorded, &health(42, "nonce")));
        // A reused PID with a different process behind it.
        assert!(!identity_verified(&recorded, &health(43, "nonce")));
        assert!(!identity_verified(&recorded, &health(42, "stale")));
        // A record without a nonce proves nothing at all.
        let mut without_nonce = info();
        without_nonce.nonce = String::new();
        assert!(!identity_verified(&without_nonce, &health(42, "")));
        assert!(!identity_verified(&without_nonce, &health(42, "nonce")));
    }

    #[test]
    fn the_daemon_is_resolved_next_to_the_executable_then_in_resources() {
        let directory = tempfile::tempdir().unwrap();
        let macos = directory.path().join("Relay.app/Contents/MacOS");
        let resources = directory.path().join("Relay.app/Contents/Resources");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&resources).unwrap();
        let exe = macos.join("relay-desktop");

        // Nothing anywhere: no command, and the tray reports it as a string.
        assert!(daemon_command_with(&exe, true, None, &resources).is_none());

        // A sidecar in Contents/MacOS wins over one in Resources.
        std::fs::write(resources.join("relayd"), b"#!/bin/sh\n").unwrap();
        let bundled = daemon_command_with(&exe, true, None, &resources).unwrap();
        assert_eq!(bundled.program, resources.join("relayd"));
        assert_eq!(
            bundled.env,
            vec![(
                RESOURCES_DIR_ENV.to_string(),
                resources.to_string_lossy().to_string()
            )]
        );

        std::fs::write(macos.join("relayd"), b"#!/bin/sh\n").unwrap();
        let sidecar = daemon_command_with(&exe, true, None, &resources).unwrap();
        assert_eq!(sidecar.program, macos.join("relayd"));

        // A development build passes no resources directory: relayd finds the UI
        // from its own build tree.
        let development = daemon_command_with(&exe, false, None, &resources).unwrap();
        assert!(development.env.is_empty());

        // The override short-circuits both.
        let override_command =
            daemon_command_with(&exe, true, Some("/opt/relayd"), &resources).unwrap();
        assert_eq!(override_command.program, PathBuf::from("/opt/relayd"));
        assert!(override_command.args.is_empty());
        assert!(override_command.env.is_empty());
        // An empty override is not an override.
        assert_eq!(
            daemon_command_with(&exe, true, Some(""), &resources)
                .unwrap()
                .program,
            macos.join("relayd")
        );
    }

    #[test]
    fn the_browser_is_chosen_from_the_installed_applications() {
        let directory = tempfile::tempdir().unwrap();
        let system = directory.path().join("Applications");
        let home = directory.path().join("home/Applications");
        std::fs::create_dir_all(&system).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let roots = vec![system.clone(), home.clone()];

        assert_eq!(installed_browser_in(&roots), None);
        std::fs::create_dir_all(home.join("Brave Browser.app")).unwrap();
        assert_eq!(
            installed_browser_in(&roots).as_deref(),
            Some("Brave Browser")
        );
        // Edge outranks Chrome outranks Chromium outranks Brave.
        std::fs::create_dir_all(system.join("Google Chrome.app")).unwrap();
        assert_eq!(
            installed_browser_in(&roots).as_deref(),
            Some("Google Chrome")
        );
        std::fs::create_dir_all(system.join("Microsoft Edge.app")).unwrap();
        assert_eq!(
            installed_browser_in(&roots).as_deref(),
            Some("Microsoft Edge")
        );
    }

    #[test]
    fn packaged_apps_are_recognised_by_their_bundle_path() {
        assert!(is_packaged_exe(Path::new(
            "/Applications/Relay.app/Contents/MacOS/relay-desktop"
        )));
        assert!(!is_packaged_exe(Path::new(
            "/Users/x/relay/target/debug/relay-desktop"
        )));
    }
}
