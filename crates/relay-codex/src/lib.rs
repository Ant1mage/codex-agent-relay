//! The Codex side of Relay.
//!
//! Two separate channels, deliberately kept apart:
//!
//! * **Hooks supply identity.** `SessionStart` hands Relay a thread id and Relay
//!   reads the real name, cwd and model from the local Codex app-server.
//! * **MCP supplies control.** The tool surface stays generic, so adding a
//!   runtime never changes what Codex sees.
//!
//! This crate also owns the integration lifecycle: the local plugin marketplace,
//! the MCP entry (Relay's own binary), repair, update and removal.

pub mod cli;
pub mod integration;
pub mod threads;

pub use cli::{codex_candidates, find_codex_cli, CodexExecutable};
pub use integration::{
    codex_status, desired_mcp_command, install_codex, materialise_plugin, mcp_executable,
    read_mcp_entry, relay_plugin_version, relay_sources, remove_codex, CodexIntegrationService,
    McpCommand, RelaySources,
};
pub use threads::{CodexAppServerThreadResolver, CodexThreadMetadata, CodexThreadMetadataResolver};
