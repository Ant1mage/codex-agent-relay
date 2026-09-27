//! Keeps the MCP process aligned with daemon-owned files and changing local CLIs.
//!
//! Configuration changes must not require restarting Codex: `list_agents` and
//! `run_agent` re-read the config file and sync the differences into the
//! registries, and policy is resolved per workspace before every run.

use std::collections::BTreeSet;
use std::sync::Mutex;

use relay_adapters::adapters;
use relay_config::ConfigStore;
use relay_core::{RunController, Runtime, RuntimeHealth};

pub struct RuntimeConfigReloader {
    controller: std::sync::Arc<RunController>,
    config: ConfigStore,
    reported: Mutex<BTreeSet<String>>,
}

impl RuntimeConfigReloader {
    pub fn new(controller: std::sync::Arc<RunController>, config: ConfigStore) -> Self {
        Self {
            controller,
            config,
            reported: Mutex::new(BTreeSet::new()),
        }
    }

    /// Re-detects runtimes and re-reads configuration.
    pub async fn refresh(&self) -> relay_core::Result<()> {
        let mut runtimes: Vec<Runtime> = Vec::new();
        let mut diagnostics: Vec<String> = Vec::new();
        for adapter in adapters() {
            let detection = adapter.detect().await;
            diagnostics.extend(detection.diagnostics);
            runtimes.extend(detection.runtimes);
        }

        let config = self.config.read();
        for entry in &config.manual_runtimes {
            if runtimes.iter().any(|runtime| runtime.id == entry.id) {
                continue;
            }
            if !std::path::Path::new(&entry.executable_path).is_file() {
                self.report_once(&format!(
                    "Manual runtime {} is not executable: {}",
                    entry.id, entry.executable_path
                ));
                continue;
            }
            let capabilities = relay_adapters::adapter_by_id(&entry.adapter_id)
                .map(|adapter| adapter.capabilities())
                .unwrap_or_default();
            runtimes.push(Runtime {
                id: entry.id.clone(),
                adapter_id: entry.adapter_id.clone(),
                executable_path: entry.executable_path.clone(),
                version: None,
                health: RuntimeHealth::Available,
                capabilities,
            });
        }

        for diagnostic in diagnostics {
            self.report_once(&diagnostic);
        }
        for warning in &config.warnings {
            self.report_once(warning);
        }

        self.controller.runtimes.sync(runtimes);
        self.controller.profiles.sync(config.profiles);
        self.controller.policies.set_global(config.policy);
        self.controller
            .policies
            .set_all_workspaces(config.workspace_overrides);
        Ok(())
    }

    /// Applies the workspace override after `run_agent`'s global refresh.
    pub fn apply_workspace(&self, workspace: &str) {
        let loaded = self.config.read();
        self.controller.policies.clear_workspace(workspace);
        if let Some(override_policy) = loaded.workspace_overrides.get(workspace) {
            self.controller
                .policies
                .set_workspace(workspace, *override_policy);
        }
    }

    fn report_once(&self, message: &str) {
        let mut reported = self.reported.lock().unwrap();
        if reported.insert(message.to_string()) {
            tracing::warn!("{message}");
        }
    }
}
