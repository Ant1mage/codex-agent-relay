//! Relay Core: the stable part of Relay.
//!
//! Core understands Relay's own domain only — Runtime, AgentProfile, Run, Step,
//! WorkerSession, Capability, Policy, AccessMode, Isolation, RelayEvent and
//! RunProjection. It knows nothing about Tauri, Codex plugin layout, MCP tool
//! names, SQLite, DeepSeek flags, HTTP routes or Electron: swapping any of those
//! must not change this crate.

pub mod adapter;
pub mod domain;
pub mod event;
pub mod host_sessions;
pub mod policy;
#[cfg(not(target_family = "wasm"))]
pub mod process;
pub mod projection;
pub mod registry;
pub mod run;
pub mod store;
pub mod worker_text;

#[cfg(test)]
mod test_support;

pub use adapter::{AdapterEvent, AgentAdapter, DetectionResult, WorkerHandle};
pub use domain::{
    now, AccessMode, AdapterCapabilities, AgentProfile, CapabilitySet, EnforcementSet, HostSession,
    HostSessionStatus, HostSessionUpsert, Isolation, ManualRuntime, ModelOption, OptionsSource,
    ProcessIdentity, ReasoningLevel, RelayError, RelayPolicy, RelayPolicyOverride, Result,
    ResumeInput, Run, RunRequest, RunStatus, Runtime, RuntimeHealth, RuntimeOptions, StartInput,
    Step, StepStatus, Timestamp, WorkerSession, WorkerStatus,
};
pub use event::{bound_native_event, RelayEvent, RelayEventType};
pub use host_sessions::{apply_upsert, HostSessionRegistry, HostSessionStore};
pub use policy::{assert_policy_allows, normalize_path, PolicyResolver, PolicyScope};
pub use projection::{project_run, RunProjection};
pub use registry::{AdapterRegistry, ProfileRegistry, RuntimeRegistry, SyncReport};
pub use run::{ActiveRun, RunController};
pub use store::{EventStore, MemoryEventStore};
pub use worker_text::{
    assistant_text, AssistantMessage, AssistantTextDelta, AssistantTextSource, WorkerAssistantText,
};
