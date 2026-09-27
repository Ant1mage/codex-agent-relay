//! Relay's HTTP/SSE surface and the wire contract every client shares.
//!
//! The contract is serde-only, so the WASM UI compiles it without a server; the
//! daemon adds the router, the projections it renders and the diagnostics report.

pub mod contract;

#[cfg(feature = "contract")]
pub mod environment;

#[cfg(feature = "server")]
pub mod diagnostics;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod server_info;
#[cfg(feature = "server")]
pub mod store_view;

pub use contract::*;

#[cfg(feature = "server")]
pub use server::{bind, router, CodexIntegration, EnvironmentService, RelayServerState};
#[cfg(feature = "server")]
pub use server_info::{DEFAULT_PORT, HOST, PORT_ATTEMPTS};
#[cfg(feature = "server")]
pub use store_view::{menu_status, task_preview, RelayStore};
