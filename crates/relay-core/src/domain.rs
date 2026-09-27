//! Relay's domain model.
//!
//! These types are Relay's own semantics. They deliberately contain no Tauri,
//! HTTP, SQLite, MCP or provider-specific knowledge: every other crate in this
//! workspace depends on this module, never the other way round.

use serde::{Deserialize, Serialize};

/// RFC 3339 timestamp with millisecond precision, e.g. `2026-01-19T12:00:00.000Z`.
pub type Timestamp = String;

/// Current wall-clock timestamp in the same shape JavaScript's `toISOString()`
/// produced, so existing stored events and clients keep parsing.
pub fn now() -> Timestamp {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// What a profile is allowed to do inside a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySet {
    pub read_workspace: bool,
    pub write_workspace: bool,
    pub execute_commands: bool,
    pub network_access: bool,
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self {
            read_workspace: true,
            write_workspace: false,
            execute_commands: false,
            network_access: false,
        }
    }
}

/// What a CLI can do, as declared by its adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterCapabilities {
    pub non_interactive: bool,
    pub structured_events: bool,
    pub cwd: bool,
    pub resume: bool,
    pub send: bool,
    pub cancel: bool,
    pub child_sessions: bool,
    /// True only when the CLI itself advertises a model flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_selection: Option<bool>,
}

impl Default for AdapterCapabilities {
    fn default() -> Self {
        Self {
            non_interactive: true,
            structured_events: false,
            cwd: true,
            resume: false,
            send: false,
            cancel: false,
            child_sessions: false,
            model_selection: None,
        }
    }
}

/// One selectable model, as reported by the CLI itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One selectable reasoning strength.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningLevel {
    pub strength: u8,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OptionsSource {
    Cli,
    Api,
    Default,
}

/// What a runtime's CLI reports about its own model and reasoning choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOptions {
    pub runtime_id: String,
    pub adapter_id: String,
    pub models: Vec<ModelOption>,
    pub levels: Vec<ReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_flag: Option<String>,
    pub source: OptionsSource,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl RuntimeOptions {
    pub fn empty(runtime_id: impl Into<String>, adapter_id: impl Into<String>, diagnostic: impl Into<String>) -> Self {
        Self {
            runtime_id: runtime_id.into(),
            adapter_id: adapter_id.into(),
            models: Vec::new(),
            levels: Vec::new(),
            model_flag: None,
            reasoning_flag: None,
            source: OptionsSource::Default,
            diagnostics: vec![diagnostic.into()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHealth {
    Available,
    AuthenticationRequired,
    Unavailable,
}

/// A CLI on this machine, found by detection or registered by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub id: String,
    pub adapter_id: String,
    pub executable_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub health: RuntimeHealth,
    pub capabilities: AdapterCapabilities,
}

/// A user-facing capability on top of a runtime. One runtime can back several.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub runtime_id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub capabilities: CapabilitySet,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    ReadOnly,
    Propose,
    Write,
}

impl AccessMode {
    pub fn is_write(self) -> bool {
        matches!(self, AccessMode::Write)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    Shared,
    Worktree,
}

impl Default for Isolation {
    fn default() -> Self {
        Isolation::Shared
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    AwaitingHost,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Orphaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub host_session_id: String,
    pub profile_id: String,
    pub task: String,
    pub cwd: String,
    pub access_mode: AccessMode,
    pub isolation: Isolation,
    pub status: RunStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Queued,
    Starting,
    Running,
    AwaitingHost,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Orphaned,
}

impl From<RunStatus> for StepStatus {
    fn from(status: RunStatus) -> Self {
        match status {
            RunStatus::Queued => StepStatus::Queued,
            RunStatus::Starting => StepStatus::Starting,
            RunStatus::Running => StepStatus::Running,
            RunStatus::AwaitingHost => StepStatus::AwaitingHost,
            RunStatus::Completed => StepStatus::Completed,
            RunStatus::Failed => StepStatus::Failed,
            RunStatus::Cancelled => StepStatus::Cancelled,
            RunStatus::Interrupted => StepStatus::Interrupted,
            RunStatus::Orphaned => StepStatus::Orphaned,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub run_id: String,
    pub profile_id: String,
    pub task: String,
    pub access_mode: AccessMode,
    pub isolation: Isolation,
    pub status: StepStatus,
    pub iteration: u32,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Orphaned,
}

impl WorkerStatus {
    pub fn is_active(self) -> bool {
        matches!(self, WorkerStatus::Starting | WorkerStatus::Running)
    }
}

/// One process/conversation in a native runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSession {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub iteration: u32,
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_worker_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    pub status: WorkerStatus,
    pub started_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostSessionStatus {
    Active,
    Offline,
    Ended,
}

/// One Codex session (thread). Never a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSession {
    pub id: String,
    pub host: String,
    pub native_session_id: String,
    pub display_name: String,
    pub name_source: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: HostSessionStatus,
    pub started_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

/// What a host reports about a session; Relay derives id and timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSessionUpsert {
    pub native_session_id: String,
    pub display_name: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "default_session_status")]
    pub status: HostSessionStatus,
}

fn default_session_status() -> HostSessionStatus {
    HostSessionStatus::Active
}

/// One delegation requested by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub host_session_id: String,
    pub profile_id: String,
    pub task: String,
    pub cwd: String,
    pub access_mode: AccessMode,
    #[serde(default)]
    pub isolation: Isolation,
}

/// Everything an adapter needs to start a worker process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInput {
    pub run_id: String,
    pub worker_session_id: String,
    pub task: String,
    pub cwd: String,
    pub access_mode: AccessMode,
    /// Exact executable chosen by the runtime registry, including manual entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Everything an adapter needs to continue an existing native session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeInput {
    pub run_id: String,
    pub worker_session_id: String,
    pub native_session_id: String,
    pub task: String,
    pub cwd: String,
    pub access_mode: AccessMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Relay's routing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPolicy {
    pub max_concurrent_runs: u32,
    pub max_concurrent_writers: u32,
    pub require_worktree_for_parallel_writers: bool,
    pub allow_write: bool,
    pub allow_commands: bool,
    pub allow_network: bool,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            max_concurrent_runs: 4,
            max_concurrent_writers: 1,
            require_worktree_for_parallel_writers: true,
            allow_write: true,
            allow_commands: true,
            allow_network: true,
        }
    }
}

/// Sparse policy patch, applied on top of the global policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPolicyOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_runs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_writers: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_worktree_for_parallel_writers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_write: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_commands: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_network: Option<bool>,
}

impl RelayPolicyOverride {
    pub fn apply_to(&self, policy: RelayPolicy) -> RelayPolicy {
        RelayPolicy {
            max_concurrent_runs: self.max_concurrent_runs.unwrap_or(policy.max_concurrent_runs),
            max_concurrent_writers: self.max_concurrent_writers.unwrap_or(policy.max_concurrent_writers),
            require_worktree_for_parallel_writers: self
                .require_worktree_for_parallel_writers
                .unwrap_or(policy.require_worktree_for_parallel_writers),
            allow_write: self.allow_write.unwrap_or(policy.allow_write),
            allow_commands: self.allow_commands.unwrap_or(policy.allow_commands),
            allow_network: self.allow_network.unwrap_or(policy.allow_network),
        }
    }
}

/// A runtime the user registered by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualRuntime {
    pub id: String,
    pub adapter_id: String,
    pub executable_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Relay's own error taxonomy. Every variant maps to a stable code string.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RelayError {
    #[error("{message}")]
    Coded { code: &'static str, message: String },
}

impl RelayError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self::Coded { code, message: message.into() }
    }

    pub fn code(&self) -> &'static str {
        match self {
            RelayError::Coded { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            RelayError::Coded { message, .. } => message,
        }
    }

    pub fn run_not_found(run_id: &str) -> Self {
        Self::new("RUN_NOT_FOUND", format!("Unknown run {run_id}"))
    }

    pub fn worker_not_found(worker_id: &str) -> Self {
        Self::new("WORKER_NOT_FOUND", format!("Unknown worker {worker_id}"))
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new("INVALID_STATE", message)
    }

    pub fn runtime_not_found(runtime_id: &str) -> Self {
        Self::new("RUNTIME_NOT_FOUND", format!("Unknown runtime {runtime_id}"))
    }

    pub fn profile_not_found(profile_id: &str) -> Self {
        Self::new("PROFILE_NOT_FOUND", format!("Unknown profile {profile_id}"))
    }
}

pub type Result<T> = std::result::Result<T, RelayError>;

impl From<serde_json::Error> for RelayError {
    fn from(error: serde_json::Error) -> Self {
        RelayError::new("INVALID_PAYLOAD", error.to_string())
    }
}

/// Forgets the difference between "absent" and "false" for optional flags.
pub fn flag(value: Option<bool>) -> bool {
    value.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_event_field_names_as_camel_case() {
        let profile = AgentProfile {
            id: "deepseek-code".into(),
            name: "DeepSeek Code".into(),
            runtime_id: "runtime:deepseek-harness".into(),
            description: "Coding worker".into(),
            instructions: None,
            model: None,
            reasoning: None,
            capabilities: CapabilitySet::default(),
            enabled: true,
        };
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["runtimeId"], "runtime:deepseek-harness");
        assert_eq!(json["capabilities"]["readWorkspace"], true);
        assert!(json.get("instructions").is_none());
    }

    #[test]
    fn policy_override_keeps_unspecified_fields() {
        let base = RelayPolicy::default();
        let patch = RelayPolicyOverride { allow_write: Some(false), ..Default::default() };
        let merged = patch.apply_to(base);
        assert!(!merged.allow_write);
        assert_eq!(merged.max_concurrent_runs, base.max_concurrent_runs);
    }

    #[test]
    fn timestamps_stay_iso8601_with_millis() {
        let stamp = now();
        assert!(stamp.ends_with('Z'));
        assert!(stamp.contains('.'));
    }
}
