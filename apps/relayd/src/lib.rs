//! Relay's daemon: the one process that owns Relay's execution.
//!
//! The daemon exposes three things to the rest of the product:
//!
//! * `reloader` keeps the runtime registry aligned with the configuration file;
//! * `engine` is the `relay_api::RunService` the HTTP layer serves — it starts,
//!   supervises and cancels external Agent CLI processes;
//! * `environment` answers everything that is not a run: detected runtimes,
//!   Agent Profiles, policy and the executable probe.
//!
//! The tray, the control panel and the MCP server Codex talks to are all clients
//! of this daemon over loopback HTTP. None of them spawns a worker.

pub mod engine;
pub mod environment;
pub mod reloader;

pub use engine::{shutdown, RelayEngine};
pub use environment::DaemonService;
pub use reloader::RuntimeConfigReloader;
