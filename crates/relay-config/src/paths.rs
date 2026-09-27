//! Where Relay keeps things.
//!
//! Every path has one definition and one environment override, so a development
//! session can move the whole state directory without touching code.

use std::path::PathBuf;

/// Relay's state directory. `RELAY_HOME` overrides it.
pub fn relay_home() -> PathBuf {
    if let Some(home) = std::env::var_os("RELAY_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    home_dir().join(".relay")
}

/// The event log and sessions.
pub fn database_path() -> PathBuf {
    env_path("RELAY_DB_PATH").unwrap_or_else(|| relay_home().join("relay.sqlite"))
}

/// Agent Profiles, policy and manual runtimes.
pub fn config_path() -> PathBuf {
    env_path("RELAY_CONFIG_PATH").unwrap_or_else(|| relay_home().join("config.toml"))
}

/// How the tray and scripts reach a running daemon.
pub fn server_info_path() -> PathBuf {
    env_path("RELAY_SERVER_INFO_PATH").unwrap_or_else(|| relay_home().join("server.json"))
}

/// Codex's own directory.
pub fn codex_home() -> PathBuf {
    env_path("CODEX_HOME").unwrap_or_else(|| home_dir().join(".codex"))
}

/// Where Relay materialises the plugin it asks Codex to install.
pub fn marketplace_root() -> PathBuf {
    env_path("RELAY_CODEX_PLUGIN_ROOT").unwrap_or_else(|| relay_home().join("codex-plugin"))
}

/// Packaged resources (the daemon, the MCP server, the Codex integration assets,
/// the built UI). Passed through by the desktop shell.
pub fn resources_dir() -> Option<PathBuf> {
    env_path("RELAY_RESOURCES_DIR")
}

/// The running Relay version.
///
/// Release builds inline it; a checkout reports the workspace version it was
/// compiled from.
pub fn relay_version() -> String {
    if let Ok(value) = std::env::var("RELAY_VERSION") {
        if !value.is_empty() {
            return value;
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
