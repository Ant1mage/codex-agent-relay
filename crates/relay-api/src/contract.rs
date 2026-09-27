//! Relay's wire contract.
//!
//! One schema per payload, shared by the daemon, the tray, the inspector and the
//! MCP process, so every surface validates the same document instead of keeping
//! its own copy of the shape. This module is serde-only: the WASM UI compiles it
//! without pulling in a server.

use relay_core::{
    AgentProfile, HostSession, ManualRuntime, RelayEvent, RelayPolicy, RelayPolicyOverride, Run, Runtime,
    RuntimeOptions, Step, WorkerSession,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run: Run,
    pub steps: Vec<Step>,
    pub workers: Vec<WorkerSession>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: HostSession,
    pub runs: Vec<RunView>,
    /// Workers in starting/running state, so a list can show load at a glance.
    pub active_workers: u32,
    pub awaiting_host: u32,
}

/// One thing Codex needs before Relay can be delegated to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexCheckId {
    CodexCli,
    RelayMcp,
    RelaySkill,
    RelayPlugin,
    RelayHooks,
}

impl CodexCheckId {
    pub fn as_str(self) -> &'static str {
        match self {
            CodexCheckId::CodexCli => "codex-cli",
            CodexCheckId::RelayMcp => "relay-mcp",
            CodexCheckId::RelaySkill => "relay-skill",
            CodexCheckId::RelayPlugin => "relay-plugin",
            CodexCheckId::RelayHooks => "relay-hooks",
        }
    }

    /// Translation key the tray and the panel use for this check.
    pub fn label_key(self) -> &'static str {
        match self {
            CodexCheckId::CodexCli => "onboarding.check.codex-cli",
            CodexCheckId::RelayMcp => "onboarding.check.relay-mcp",
            CodexCheckId::RelaySkill => "onboarding.check.relay-skill",
            CodexCheckId::RelayPlugin => "onboarding.check.relay-plugin",
            CodexCheckId::RelayHooks => "onboarding.check.relay-hooks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexCheckStatus {
    Ok,
    Missing,
    Stale,
    Outdated,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCheck {
    pub id: CodexCheckId,
    pub ok: bool,
    pub status: CodexCheckStatus,
    pub detail: String,
    /// Suggested next step, shown verbatim in the control panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStatus {
    pub checks: Vec<CodexCheck>,
    pub configured: bool,
}

impl CodexStatus {
    pub fn unknown() -> Self {
        Self { checks: Vec::new(), configured: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectorSnapshot {
    pub sessions: Vec<SessionView>,
    pub runtimes: Vec<Runtime>,
    pub profiles: Vec<AgentProfile>,
    pub diagnostics: Vec<String>,
    pub codex: CodexStatus,
    pub generated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuWorker {
    pub worker_session_id: String,
    pub run_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSession {
    pub id: String,
    pub display_name: String,
    pub cwd: String,
    pub active_workers: Vec<MenuWorker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuBlocked {
    Auth,
    Missing,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuAgent {
    pub id: String,
    pub name: String,
    /// Absent when the agent can run; otherwise why it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<MenuBlocked>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuStatus {
    Ready,
    NeedsSetup,
    NoRuntime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuView {
    pub status: MenuStatus,
    pub running_workers: u32,
    pub awaiting_host: u32,
    pub sessions: Vec<MenuSession>,
    pub agents: Vec<MenuAgent>,
    /// Runtime scan results, so the menu can show the environment itself.
    pub runtimes: Vec<Runtime>,
    pub codex: CodexStatus,
}

/// Relay's own configuration as it exists on disk right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayConfigView {
    pub profiles: Vec<AgentProfile>,
    pub policy: RelayPolicy,
    pub workspace_overrides: std::collections::BTreeMap<String, RelayPolicyOverride>,
    /// Hand-registered runtimes, merged with what the scanner finds.
    pub manual_runtimes: Vec<ManualRuntime>,
    /// Unparseable or invalid configuration files. The daemon keeps running on
    /// defaults; the panel shows this instead of pretending nothing happened.
    pub warnings: Vec<String>,
    /// Changes whenever the configuration file changes.
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterCatalog {
    pub adapters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProbe {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A runtime write answers with the new configuration and what was verified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMutation {
    pub config: RelayConfigView,
    pub probe: RuntimeProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexAction {
    Install,
    Repair,
    Update,
    Remove,
}

impl CodexAction {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "install" => Some(CodexAction::Install),
            "repair" => Some(CodexAction::Repair),
            "update" => Some(CodexAction::Update),
            "remove" => Some(CodexAction::Remove),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub runtimes: u32,
    pub profiles: u32,
    pub detected_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub ok: bool,
    pub pid: u32,
    /// Identity proof: a reused PID cannot fake this.
    pub nonce: String,
    pub port: u16,
    pub started_at: String,
    pub version: String,
    pub database: String,
    pub sessions: u32,
    pub runs: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventBatch {
    pub run_id: String,
    pub events: Vec<RelayEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamMessage {
    Hello { port: u16, started_at: String },
    Snapshot { snapshot: Box<InspectorSnapshot> },
    Events { batch: EventBatch },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelResult {
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub status: CodexStatus,
    pub messages: Vec<String>,
}

/// `GET /api/runtimes/:id/options`.
pub type RuntimeOptionsView = RuntimeOptions;

/// Body of `PUT /api/config/profiles/:id` — the same shape as a stored profile.
pub type ProfileBody = AgentProfile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyBody {
    pub policy: RelayPolicy,
    #[serde(default)]
    pub workspace_overrides: std::collections::BTreeMap<String, RelayPolicyOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBody {
    #[serde(default)]
    pub adapter_id: String,
    #[serde(default)]
    pub executable_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeBody {
    #[serde(default)]
    pub adapter_id: String,
    #[serde(default)]
    pub executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_core::{AccessMode, Isolation, RunStatus};

    #[test]
    fn stream_messages_keep_their_discriminator() {
        let snapshot = InspectorSnapshot {
            sessions: Vec::new(),
            runtimes: Vec::new(),
            profiles: Vec::new(),
            diagnostics: Vec::new(),
            codex: CodexStatus::unknown(),
            generated_at: "2026-01-01T00:00:00.000Z".into(),
        };
        let message = StreamMessage::Snapshot { snapshot: Box::new(snapshot) };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "snapshot");
        assert!(json["snapshot"]["generatedAt"].is_string());

        let hello = StreamMessage::Hello { port: 7352, started_at: "2026-01-01T00:00:00.000Z".into() };
        assert_eq!(serde_json::to_value(&hello).unwrap()["type"], "hello");
    }

    #[test]
    fn runs_and_sessions_round_trip_through_the_wire_shape() {
        let run = Run {
            id: "run:1".into(),
            host_session_id: "codex:s".into(),
            profile_id: "p".into(),
            task: "t".into(),
            cwd: "/tmp".into(),
            access_mode: AccessMode::ReadOnly,
            isolation: Isolation::Shared,
            status: RunStatus::Running,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        };
        let view = RunView { run, steps: Vec::new(), workers: Vec::new() };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["run"]["accessMode"], "read_only");
        assert_eq!(json["run"]["hostSessionId"], "codex:s");
        let decoded: RunView = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, view);
    }

    #[test]
    fn codex_check_ids_are_the_stable_kebab_names() {
        assert_eq!(CodexCheckId::CodexCli.as_str(), "codex-cli");
        assert_eq!(CodexCheckId::RelayMcp.as_str(), "relay-mcp");
        assert_eq!(
            serde_json::to_value(CodexCheckId::RelayHooks).unwrap(),
            serde_json::json!("relay-hooks")
        );
        assert_eq!(CodexAction::parse("repair"), Some(CodexAction::Repair));
        assert_eq!(CodexAction::parse("nope"), None);
    }
}
