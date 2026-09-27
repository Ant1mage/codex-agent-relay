//! Reading and writing `~/.relay/config.toml`.
//!
//! The file format is Relay's own; it is deliberately simple and human-editable.
//! Writes are atomic, and a file that fails to parse is reported as a warning and
//! never silently overwritten — the panel shows the warning instead of pretending
//! the defaults are the user's settings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use relay_core::{
    AgentProfile, CapabilitySet, ManualRuntime, RelayPolicy, RelayPolicyOverride, Result, Runtime,
};
use serde::{Deserialize, Serialize};

use crate::paths::config_path;

#[derive(Debug, Clone, PartialEq)]
pub struct RelayConfig {
    pub profiles: Vec<AgentProfile>,
    pub policy: RelayPolicy,
    pub workspace_overrides: BTreeMap<String, RelayPolicyOverride>,
    pub manual_runtimes: Vec<ManualRuntime>,
    pub warnings: Vec<String>,
    /// Cheap change stamp (mtime + size) for hot reload.
    pub revision: String,
}

/* ------------------------------------------------------------------ */
/* File shape                                                          */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigDocument {
    #[serde(default, skip_serializing_if = "PolicyDocument::is_empty")]
    policy: PolicyDocument,
    /// Per-workspace policy overrides, keyed by absolute path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    workspaces: BTreeMap<String, PolicyDocument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    agents: Vec<AgentDocument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    runtimes: Vec<RuntimeDocument>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_concurrent_runs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_concurrent_writers: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    require_worktree_for_parallel_writers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_write: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_commands: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_network: Option<bool>,
}

impl PolicyDocument {
    fn is_empty(&self) -> bool {
        self.max_concurrent_runs.is_none()
            && self.max_concurrent_writers.is_none()
            && self.require_worktree_for_parallel_writers.is_none()
            && self.allow_write.is_none()
            && self.allow_commands.is_none()
            && self.allow_network.is_none()
    }

    fn from_policy(policy: RelayPolicy) -> Self {
        Self {
            max_concurrent_runs: Some(policy.max_concurrent_runs),
            max_concurrent_writers: Some(policy.max_concurrent_writers),
            require_worktree_for_parallel_writers: Some(
                policy.require_worktree_for_parallel_writers,
            ),
            allow_write: Some(policy.allow_write),
            allow_commands: Some(policy.allow_commands),
            allow_network: Some(policy.allow_network),
        }
    }

    fn to_policy(&self) -> RelayPolicy {
        let base = RelayPolicy::default();
        self.to_override().apply_to(base)
    }

    fn to_override(&self) -> RelayPolicyOverride {
        RelayPolicyOverride {
            max_concurrent_runs: self.max_concurrent_runs,
            max_concurrent_writers: self.max_concurrent_writers,
            require_worktree_for_parallel_writers: self.require_worktree_for_parallel_writers,
            allow_write: self.allow_write,
            allow_commands: self.allow_commands,
            allow_network: self.allow_network,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentDocument {
    id: String,
    name: String,
    runtime_id: String,
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    capabilities: CapabilityDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityDocument {
    #[serde(default = "default_true")]
    read_workspace: bool,
    #[serde(default)]
    write_workspace: bool,
    #[serde(default)]
    execute_commands: bool,
    #[serde(default)]
    network_access: bool,
}

impl Default for CapabilityDocument {
    fn default() -> Self {
        Self {
            read_workspace: true,
            write_workspace: false,
            execute_commands: false,
            network_access: false,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeDocument {
    id: String,
    adapter_id: String,
    executable_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

impl From<&AgentProfile> for AgentDocument {
    fn from(profile: &AgentProfile) -> Self {
        Self {
            id: profile.id.clone(),
            name: profile.name.clone(),
            runtime_id: profile.runtime_id.clone(),
            description: profile.description.clone(),
            instructions: profile.instructions.clone(),
            model: profile.model.clone(),
            reasoning: profile.reasoning.clone(),
            enabled: profile.enabled,
            capabilities: CapabilityDocument {
                read_workspace: profile.capabilities.read_workspace,
                write_workspace: profile.capabilities.write_workspace,
                execute_commands: profile.capabilities.execute_commands,
                network_access: profile.capabilities.network_access,
            },
        }
    }
}

impl From<&AgentDocument> for AgentProfile {
    fn from(document: &AgentDocument) -> Self {
        Self {
            id: document.id.clone(),
            name: document.name.clone(),
            runtime_id: document.runtime_id.clone(),
            description: document.description.clone(),
            instructions: document.instructions.clone(),
            model: document.model.clone(),
            reasoning: document.reasoning.clone(),
            capabilities: CapabilitySet {
                read_workspace: document.capabilities.read_workspace,
                write_workspace: document.capabilities.write_workspace,
                execute_commands: document.capabilities.execute_commands,
                network_access: document.capabilities.network_access,
            },
            enabled: document.enabled,
        }
    }
}

impl From<&ManualRuntime> for RuntimeDocument {
    fn from(runtime: &ManualRuntime) -> Self {
        Self {
            id: runtime.id.clone(),
            adapter_id: runtime.adapter_id.clone(),
            executable_path: runtime.executable_path.clone(),
            label: runtime.label.clone(),
        }
    }
}

impl From<&RuntimeDocument> for ManualRuntime {
    fn from(document: &RuntimeDocument) -> Self {
        Self {
            id: document.id.clone(),
            adapter_id: document.adapter_id.clone(),
            executable_path: document.executable_path.clone(),
            label: document.label.clone(),
        }
    }
}

/* ------------------------------------------------------------------ */
/* Store                                                               */
/* ------------------------------------------------------------------ */

/// A path, nothing else: cloning one is how the daemon hands the same
/// configuration to the environment service and to the execution engine.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new(config_path())
    }
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Never fails: an unreadable file becomes a warning and Relay keeps running
    /// on defaults.
    pub fn read(&self) -> RelayConfig {
        let mut warnings = Vec::new();
        let document = self.read_document(&mut warnings).unwrap_or_default();
        RelayConfig {
            profiles: document.agents.iter().map(AgentProfile::from).collect(),
            policy: document.policy.to_policy(),
            workspace_overrides: document
                .workspaces
                .iter()
                .map(|(workspace, policy)| (workspace.clone(), policy.to_override()))
                .collect(),
            manual_runtimes: document.runtimes.iter().map(ManualRuntime::from).collect(),
            warnings,
            revision: self.revision(),
        }
    }

    fn read_document(&self, warnings: &mut Vec<String>) -> Option<ConfigDocument> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Some(ConfigDocument::default())
            }
            Err(error) => {
                warnings.push(format!("{} 无法读取：{error}", self.path.display()));
                return None;
            }
        };
        if contents.trim().is_empty() {
            return Some(ConfigDocument::default());
        }
        match toml::from_str::<ConfigDocument>(&contents) {
            Ok(document) => Some(document),
            Err(error) => {
                warnings.push(format!("config.toml 无法解析，正在使用默认配置：{error}"));
                None
            }
        }
    }

    /// Refuses to write over a file that failed to parse: the panel edits what it
    /// read, and a corrupt file has no readable content to merge with.
    fn assert_readable(&self) -> Result<()> {
        let mut warnings = Vec::new();
        self.read_document(&mut warnings);
        if let Some(warning) = warnings.first() {
            return Err(relay_core::RelayError::new(
                "CONFIG_UNREADABLE",
                format!("拒绝覆盖损坏的 config.toml；请先备份并修复或移走原文件。{warning}"),
            ));
        }
        Ok(())
    }

    fn write_document(&self, document: &ConfigDocument) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                relay_core::RelayError::new(
                    "CONFIG_WRITE_FAILED",
                    format!("{}: {error}", parent.display()),
                )
            })?;
        }
        let body = toml::to_string_pretty(document).map_err(|error| {
            relay_core::RelayError::new("CONFIG_WRITE_FAILED", error.to_string())
        })?;
        let temporary = self.path.with_extension("toml.tmp");
        std::fs::write(&temporary, body).map_err(|error| {
            relay_core::RelayError::new(
                "CONFIG_WRITE_FAILED",
                format!("{}: {error}", temporary.display()),
            )
        })?;
        std::fs::rename(&temporary, &self.path).map_err(|error| {
            relay_core::RelayError::new(
                "CONFIG_WRITE_FAILED",
                format!("{}: {error}", self.path.display()),
            )
        })?;
        Ok(())
    }

    fn mutate(&self, change: impl FnOnce(&mut ConfigDocument)) -> Result<RelayConfig> {
        self.assert_readable()?;
        let mut warnings = Vec::new();
        let mut document = self.read_document(&mut warnings).unwrap_or_default();
        change(&mut document);
        self.write_document(&document)?;
        Ok(self.read())
    }

    pub fn upsert_profile(&self, profile: &AgentProfile) -> Result<RelayConfig> {
        let document = AgentDocument::from(profile);
        self.mutate(|config| {
            match config
                .agents
                .iter_mut()
                .find(|agent| agent.id == document.id)
            {
                Some(existing) => *existing = document,
                None => config.agents.push(document),
            }
        })
    }

    pub fn remove_profile(&self, profile_id: &str) -> Result<RelayConfig> {
        self.mutate(|config| config.agents.retain(|agent| agent.id != profile_id))
    }

    pub fn write_policy(
        &self,
        policy: RelayPolicy,
        workspace_overrides: BTreeMap<String, RelayPolicyOverride>,
    ) -> Result<RelayConfig> {
        self.mutate(|config| {
            config.policy = PolicyDocument::from_policy(policy);
            config.workspaces = workspace_overrides
                .into_iter()
                .map(|(workspace, patch)| {
                    (
                        workspace,
                        PolicyDocument {
                            max_concurrent_runs: patch.max_concurrent_runs,
                            max_concurrent_writers: patch.max_concurrent_writers,
                            require_worktree_for_parallel_writers: patch
                                .require_worktree_for_parallel_writers,
                            allow_write: patch.allow_write,
                            allow_commands: patch.allow_commands,
                            allow_network: patch.allow_network,
                        },
                    )
                })
                .collect();
        })
    }

    pub fn upsert_manual_runtime(&self, runtime: &ManualRuntime) -> Result<RelayConfig> {
        let document = RuntimeDocument::from(runtime);
        self.mutate(|config| {
            match config
                .runtimes
                .iter_mut()
                .find(|entry| entry.id == document.id)
            {
                Some(existing) => *existing = document,
                None => config.runtimes.push(document),
            }
        })
    }

    pub fn remove_manual_runtime(&self, runtime_id: &str) -> Result<RelayConfig> {
        self.mutate(|config| config.runtimes.retain(|entry| entry.id != runtime_id))
    }

    /// `mtime:size`, or `absent`.
    pub fn revision(&self) -> String {
        match std::fs::metadata(&self.path) {
            Ok(metadata) => {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis())
                    .unwrap_or(0);
                format!("{modified}:{}", metadata.len())
            }
            Err(_) => "absent".to_string(),
        }
    }

    /// Profiles whose runtime is missing are kept: hiding user configuration
    /// would make the loss invisible.
    pub fn profiles_for(&self, _runtimes: &[Runtime]) -> Vec<AgentProfile> {
        self.read().profiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ConfigStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.toml"));
        (directory, store)
    }

    fn profile(id: &str) -> AgentProfile {
        AgentProfile {
            id: id.to_string(),
            name: "DeepSeek Code".to_string(),
            runtime_id: "runtime:deepseek-harness".to_string(),
            description: "Coding worker".to_string(),
            instructions: Some("Be brief".to_string()),
            model: Some("deepseek-chat".to_string()),
            reasoning: Some("high".to_string()),
            capabilities: CapabilitySet {
                read_workspace: true,
                write_workspace: true,
                execute_commands: true,
                network_access: false,
            },
            enabled: true,
        }
    }

    #[test]
    fn a_missing_file_reads_as_an_empty_configuration() {
        let (_directory, store) = store();
        let config = store.read();
        assert!(
            config.profiles.is_empty(),
            "detection must not create agents"
        );
        assert_eq!(config.policy, RelayPolicy::default());
        assert!(config.warnings.is_empty());
        assert_eq!(config.revision, "absent");
    }

    #[test]
    fn profiles_round_trip_through_the_file() {
        let (_directory, store) = store();
        store.upsert_profile(&profile("agent-1")).unwrap();
        store.upsert_profile(&profile("agent-2")).unwrap();
        let config = store.read();
        assert_eq!(config.profiles.len(), 2);
        assert_eq!(config.profiles[0].instructions.as_deref(), Some("Be brief"));
        assert_eq!(config.profiles[0].model.as_deref(), Some("deepseek-chat"));
        assert_eq!(config.profiles[0].reasoning.as_deref(), Some("high"));
        assert!(config.profiles[0].capabilities.write_workspace);

        store.remove_profile("agent-1").unwrap();
        let config = store.read();
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.profiles[0].id, "agent-2");
    }

    #[test]
    fn policy_and_workspace_overrides_round_trip() {
        let (_directory, store) = store();
        let policy = RelayPolicy {
            max_concurrent_runs: 6,
            allow_network: false,
            ..RelayPolicy::default()
        };
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "/tmp/project".to_string(),
            RelayPolicyOverride {
                allow_write: Some(false),
                ..Default::default()
            },
        );
        store.write_policy(policy, overrides).unwrap();

        let config = store.read();
        assert_eq!(config.policy.max_concurrent_runs, 6);
        assert!(!config.policy.allow_network);
        assert_eq!(
            config.workspace_overrides["/tmp/project"].allow_write,
            Some(false)
        );
    }

    #[test]
    fn manual_runtimes_round_trip_and_are_additive() {
        let (_directory, store) = store();
        store
            .upsert_manual_runtime(&ManualRuntime {
                id: "manual:deepseek".to_string(),
                adapter_id: "deepseek-harness".to_string(),
                executable_path: "/usr/local/bin/dsh".to_string(),
                label: Some("Work dsh".to_string()),
            })
            .unwrap();
        let config = store.read();
        assert_eq!(config.manual_runtimes.len(), 1);
        assert_eq!(config.manual_runtimes[0].label.as_deref(), Some("Work dsh"));
        store.remove_manual_runtime("manual:deepseek").unwrap();
        assert!(store.read().manual_runtimes.is_empty());
    }

    #[test]
    fn a_broken_file_is_reported_and_never_overwritten() {
        let (directory, store) = store();
        std::fs::write(
            directory.path().join("config.toml"),
            "this is not toml = = =",
        )
        .unwrap();
        let config = store.read();
        assert_eq!(config.warnings.len(), 1);
        let error = store.upsert_profile(&profile("agent-1")).unwrap_err();
        assert_eq!(error.code(), "CONFIG_UNREADABLE");
        // The original bytes survive.
        assert!(
            std::fs::read_to_string(directory.path().join("config.toml"))
                .unwrap()
                .contains("not toml")
        );
    }

    #[test]
    fn the_revision_changes_when_the_file_changes() {
        let (_directory, store) = store();
        let before = store.revision();
        store.upsert_profile(&profile("agent-1")).unwrap();
        assert_ne!(before, store.revision());
    }

    #[test]
    fn an_empty_file_is_valid_configuration() {
        let (directory, store) = store();
        std::fs::write(directory.path().join("config.toml"), "\n").unwrap();
        let config = store.read();
        assert!(config.warnings.is_empty());
        assert!(config.profiles.is_empty());
    }
}
