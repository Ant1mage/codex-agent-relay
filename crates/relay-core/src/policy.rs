//! Policy resolution and enforcement.
//!
//! Policy answers one question before anything is spawned: may this run start,
//! in this workspace, with this profile? Global → workspace → session, most
//! specific wins.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;

use crate::domain::{AgentProfile, RelayError, RelayPolicy, RelayPolicyOverride, Result, RunRequest};

/// Resolves the effective policy for a scope.
#[derive(Debug, Default)]
pub struct PolicyResolver {
    global: RwLock<RelayPolicy>,
    workspace: RwLock<HashMap<String, RelayPolicyOverride>>,
    session: RwLock<HashMap<String, RelayPolicyOverride>>,
}

#[derive(Debug, Clone, Default)]
pub struct PolicyScope<'a> {
    pub workspace: Option<&'a str>,
    pub host_session_id: Option<&'a str>,
}

impl PolicyResolver {
    pub fn new(global: RelayPolicy) -> Self {
        Self {
            global: RwLock::new(global),
            workspace: RwLock::new(HashMap::new()),
            session: RwLock::new(HashMap::new()),
        }
    }

    pub fn set_global(&self, policy: RelayPolicy) {
        *self.global.write().unwrap() = policy;
    }

    pub fn global(&self) -> RelayPolicy {
        *self.global.read().unwrap()
    }

    pub fn set_workspace(&self, workspace: &str, patch: RelayPolicyOverride) {
        self.workspace.write().unwrap().insert(normalize_path(workspace), patch);
    }

    pub fn clear_workspace(&self, workspace: &str) {
        self.workspace.write().unwrap().remove(&normalize_path(workspace));
    }

    pub fn set_all_workspaces(
        &self,
        overrides: impl IntoIterator<Item = (String, RelayPolicyOverride)>,
    ) {
        let mut guard = self.workspace.write().unwrap();
        guard.clear();
        for (workspace, patch) in overrides {
            guard.insert(normalize_path(&workspace), patch);
        }
    }

    pub fn set_session(&self, host_session_id: &str, patch: RelayPolicyOverride) {
        self.session.write().unwrap().insert(host_session_id.to_string(), patch);
    }

    pub fn clear_session(&self, host_session_id: &str) {
        self.session.write().unwrap().remove(host_session_id);
    }

    pub fn resolve(&self, scope: PolicyScope<'_>) -> RelayPolicy {
        let mut policy = self.global();
        if let Some(workspace) = scope.workspace {
            if let Some(patch) = self.workspace.read().unwrap().get(&normalize_path(workspace)) {
                policy = patch.apply_to(policy);
            }
        }
        if let Some(session) = scope.host_session_id {
            if let Some(patch) = self.session.read().unwrap().get(session) {
                policy = patch.apply_to(policy);
            }
        }
        policy
    }
}

/// Rejects a request the policy forbids, before any process is started.
pub fn assert_policy_allows(policy: RelayPolicy, request: &RunRequest, profile: &AgentProfile) -> Result<()> {
    if request.access_mode.is_write() && !policy.allow_write {
        return Err(RelayError::new("CAPABILITY_DENIED", "Workspace writes are disabled by policy"));
    }
    if profile.capabilities.execute_commands && !policy.allow_commands {
        return Err(RelayError::new("CAPABILITY_DENIED", "Command execution is disabled by policy"));
    }
    if profile.capabilities.network_access && !policy.allow_network {
        return Err(RelayError::new("CAPABILITY_DENIED", "Network access is disabled by policy"));
    }
    Ok(())
}

/// Absolute, dot-free path, used as the identity of a workspace.
///
/// `canonicalize` is not usable here: the workspace may be checked before it
/// exists, and symlink resolution would make two spellings of the same directory
/// look like two different workspaces.
pub fn normalize_path(value: &str) -> String {
    let path = Path::new(value);
    let absolute: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AccessMode, CapabilitySet, Isolation};

    fn base_policy() -> RelayPolicy {
        RelayPolicy {
            max_concurrent_runs: 2,
            max_concurrent_writers: 1,
            require_worktree_for_parallel_writers: true,
            allow_write: false,
            allow_commands: false,
            allow_network: false,
        }
    }

    #[test]
    fn session_overrides_win_over_workspace_and_global() {
        let resolver = PolicyResolver::new(base_policy());
        resolver.set_workspace(
            "/tmp/project",
            RelayPolicyOverride { allow_write: Some(true), max_concurrent_runs: Some(3), ..Default::default() },
        );
        resolver.set_session(
            "codex:session",
            RelayPolicyOverride {
                max_concurrent_runs: Some(5),
                allow_network: Some(true),
                ..Default::default()
            },
        );

        let resolved = resolver.resolve(PolicyScope {
            workspace: Some("/tmp/project"),
            host_session_id: Some("codex:session"),
        });
        assert_eq!(resolved.max_concurrent_runs, 5);
        assert!(resolved.allow_write);
        assert!(!resolved.allow_commands);
        assert!(resolved.allow_network);
    }

    #[test]
    fn clearing_a_session_removes_temporary_policy() {
        let resolver = PolicyResolver::new(RelayPolicy::default());
        resolver.set_session("codex:session", RelayPolicyOverride { allow_write: Some(false), ..Default::default() });
        resolver.clear_session("codex:session");
        assert!(resolver.resolve(PolicyScope { host_session_id: Some("codex:session"), ..Default::default() }).allow_write);
    }

    #[test]
    fn trailing_slashes_do_not_create_a_second_workspace() {
        let resolver = PolicyResolver::new(RelayPolicy::default());
        resolver.set_workspace("/tmp/project/", RelayPolicyOverride { allow_write: Some(false), ..Default::default() });
        assert!(!resolver.resolve(PolicyScope { workspace: Some("/tmp/project"), ..Default::default() }).allow_write);
    }

    #[test]
    fn policy_denials_use_stable_codes() {
        let profile = AgentProfile {
            id: "p".into(),
            name: "p".into(),
            runtime_id: "r".into(),
            description: "d".into(),
            instructions: None,
            model: None,
            reasoning: None,
            capabilities: CapabilitySet {
                read_workspace: true,
                write_workspace: true,
                execute_commands: true,
                network_access: true,
            },
            enabled: true,
        };
        let request = RunRequest {
            host_session_id: "codex:s".into(),
            profile_id: "p".into(),
            task: "t".into(),
            cwd: "/tmp".into(),
            access_mode: AccessMode::Write,
            isolation: Isolation::Shared,
        };
        let denied = assert_policy_allows(base_policy(), &request, &profile).unwrap_err();
        assert_eq!(denied.code(), "CAPABILITY_DENIED");

        let allowed = assert_policy_allows(RelayPolicy::default(), &request, &profile);
        assert!(allowed.is_ok());
    }
}
