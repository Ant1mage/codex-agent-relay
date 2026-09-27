//! The runtime adapter boundary.
//!
//! An adapter is the only place in Relay that knows a specific CLI. It turns a
//! normalized start/resume input into native arguments, and native output into
//! Relay events. Core never parses provider output and never learns a CLI flag.
//!
//! The trait deliberately stays narrow: it serves the CLIs Relay ships with
//! today. Node sidecars, remote workers, plugin ABI and dynamic loading are out
//! of scope and are not designed for here.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::domain::{
    AdapterCapabilities, RelayError, Result, ResumeInput, Runtime, RuntimeOptions, StartInput,
};
use crate::event::RelayEventType;

/// One normalized fact produced by a running worker.
#[derive(Debug, Clone)]
pub struct AdapterEvent {
    pub event_type: RelayEventType,
    pub data: serde_json::Value,
    pub native_event: Option<serde_json::Value>,
}

impl AdapterEvent {
    pub fn new(event_type: RelayEventType, data: serde_json::Value) -> Self {
        Self {
            event_type,
            data,
            native_event: None,
        }
    }

    pub fn with_native(
        event_type: RelayEventType,
        data: serde_json::Value,
        native: serde_json::Value,
    ) -> Self {
        Self {
            event_type,
            data,
            native_event: Some(native),
        }
    }
}

/// A started worker process and the stream of facts it will produce.
pub struct WorkerHandle {
    pub native_session_id: Option<String>,
    pub process_id: Option<u32>,
    pub events: mpsc::Receiver<AdapterEvent>,
}

impl std::fmt::Debug for WorkerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerHandle")
            .field("native_session_id", &self.native_session_id)
            .field("process_id", &self.process_id)
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DetectionResult {
    pub runtimes: Vec<Runtime>,
    pub diagnostics: Vec<String>,
}

/// The contract every runtime CLI is reached through.
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    /// Stable adapter id used in configuration and in the runtime registry.
    fn id(&self) -> &str;

    /// Declared capabilities. Relay degrades instead of pretending: a CLI that
    /// cannot resume never offers `resume_agent`.
    fn capabilities(&self) -> AdapterCapabilities;

    /// Finds the CLI on this machine. Detection never creates an Agent Profile.
    async fn detect(&self) -> DetectionResult;

    /// Model and reasoning values a run of this runtime can actually apply.
    ///
    /// The whole `Runtime` is passed on purpose: a manually registered runtime
    /// carries an exact `executable_path` that discovery would not find again, and
    /// probing a different CLI than the one a run will use is a bug, not a detail.
    /// A list reported here is a promise — see `with_selection_args` and each
    /// adapter's launch path, which must be able to apply every value offered.
    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions {
        RuntimeOptions::empty(
            &runtime.id,
            &runtime.adapter_id,
            format!("{} exposes no model or reasoning options", self.id()),
        )
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle>;

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        let _ = input;
        Err(RelayError::new(
            "OPERATION_UNSUPPORTED",
            format!("Adapter {} does not support resume", self.id()),
        ))
    }

    async fn send(&self, native_session_id: &str, message: &str) -> Result<()> {
        let _ = (native_session_id, message);
        Err(RelayError::new(
            "OPERATION_UNSUPPORTED",
            format!("Adapter {} does not support send", self.id()),
        ))
    }

    /// Terminates the worker process behind a native session id.
    async fn cancel(&self, native_session_id: &str) -> Result<()>;

    /// Kills anything still running. Called at shutdown.
    async fn dispose(&self) {}

    /// Provider key for the official model-list fallback, when the CLI publishes
    /// no model names. `None` means "no fallback for this runtime".
    fn model_fallback_provider(&self) -> Option<&'static str> {
        None
    }
}

/// Convenience for adapters that need to report a failure with the standard code.
pub fn adapter_failure(message: impl Into<String>) -> RelayError {
    RelayError::new("ADAPTER_FAILURE", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_events_keep_the_native_payload_when_given_one() {
        let event = AdapterEvent::with_native(
            RelayEventType::ToolRead,
            serde_json::json!({ "path": "a.rs" }),
            serde_json::json!({ "raw": true }),
        );
        assert_eq!(event.event_type, RelayEventType::ToolRead);
        assert_eq!(event.native_event.unwrap()["raw"], true);
        assert!(
            AdapterEvent::new(RelayEventType::WorkerMessage, serde_json::json!({}))
                .native_event
                .is_none()
        );
    }
}
