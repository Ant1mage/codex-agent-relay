//! Relay's own configuration, on disk, with one owner for the paths.
//!
//! Split by lifetime, not by process:
//!
//! * SQLite (`relay.sqlite`) holds runtime state: the event log, sessions, runs.
//! * `config.toml` holds what the user configured: Agent Profiles, policy and
//!   hand-registered runtimes.
//!
//! The daemon is the only writer of the config file; the MCP process re-reads it
//! on every call, so configuration changes never need a restart.

pub mod paths;
pub mod presets;
pub mod store;

pub use paths::{
    codex_home, config_path, database_path, marketplace_root, relay_home, relay_version,
    resources_dir, server_info_path,
};
pub use presets::profile_presets;
pub use store::{ConfigStore, RelayConfig};
