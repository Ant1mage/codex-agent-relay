//! Everything the daemon knows about this machine that is not in the event log:
//! which runtimes exist, which Agent Profiles the user configured, and whether
//! the Codex side of the integration is present, current and removable.
//!
//! Runtime detection only *finds* runtimes. It never creates an Agent Profile:
//! the presets in `relay-config` are offered to the user, not applied for them.

use std::collections::BTreeMap;
use std::sync::RwLock;

use async_trait::async_trait;
use relay_adapters::{adapter_ids, adapters};
use relay_api::{
    EnvironmentService, PolicyBody, RelayConfigView, RuntimeBody, RuntimeMutation, RuntimeProbe,
};
use relay_config::{ConfigStore, RelayConfig};
use relay_core::{
    AdapterCapabilities, AgentProfile, ManualRuntime, RelayPolicyOverride, Runtime, RuntimeHealth, RuntimeOptions,
};

use relay_api::environment::Environment;

/// Runs the CLI once to learn its version, so the panel can tell "registered but
/// broken" from "registered and working" before anything is saved.
pub async fn probe_executable(executable_path: &str) -> RuntimeProbe {
    if !std::path::Path::new(executable_path).is_file() {
        return RuntimeProbe { ok: false, version: None, error: Some(format!("找不到可执行文件: {executable_path}")) };
    }
    let args = vec!["--version".to_string()];
    match relay_adapters::probe::capture(executable_path, &args).await {
        Some((code, stdout, stderr)) => {
            if code == 0 {
                let version = stdout.lines().next().unwrap_or_default().trim().to_string();
                RuntimeProbe { ok: true, version: (!version.is_empty()).then_some(version), error: None }
            } else {
                let message = if stderr.trim().is_empty() { stdout } else { stderr };
                RuntimeProbe { ok: false, version: None, error: Some(truncate(message.trim(), 200)) }
            }
        }
        None => RuntimeProbe {
            ok: false,
            version: None,
            error: Some(format!("无法执行 {executable_path}（超时或权限不足）")),
        },
    }
}

/// Builds a Runtime from a hand-registered entry, using its adapter's declared capabilities.
pub fn runtime_from_manual(entry: &ManualRuntime, probe: &RuntimeProbe) -> Runtime {
    let capabilities = relay_adapters::adapter_by_id(&entry.adapter_id)
        .map(|adapter| adapter.capabilities())
        .unwrap_or_else(AdapterCapabilities::default);
    Runtime {
        id: entry.id.clone(),
        adapter_id: entry.adapter_id.clone(),
        executable_path: entry.executable_path.clone(),
        version: probe.version.clone(),
        health: RuntimeHealth::Available,
        capabilities,
    }
}

/// Runtimes on this machine: what the adapters found, plus anything the user
/// registered by hand. Manual entries are additive — detection stays the source
/// of truth for the CLIs it knows how to find.
pub async fn detect_environment(config: &ConfigStore) -> Environment {
    let loaded = config.read();
    let mut runtimes: Vec<Runtime> = Vec::new();
    let mut diagnostics: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for adapter in adapters() {
        let detection = adapter.detect().await;
        for diagnostic in detection.diagnostics {
            if !diagnostics.contains(&diagnostic) {
                diagnostics.push(diagnostic);
            }
        }
        for runtime in detection.runtimes {
            if seen.contains(&runtime.id) {
                continue;
            }
            seen.push(runtime.id.clone());
            runtimes.push(runtime);
        }
    }

    for entry in &loaded.manual_runtimes {
        if seen.contains(&entry.id) {
            continue;
        }
        let probe = probe_executable(&entry.executable_path).await;
        if !probe.ok {
            diagnostics.push(format!(
                "手动运行时 {} 不可用：{}",
                entry.id,
                probe.error.unwrap_or_else(|| "unknown".to_string())
            ));
            continue;
        }
        seen.push(entry.id.clone());
        runtimes.push(runtime_from_manual(entry, &probe));
    }

    diagnostics.extend(loaded.warnings);
    Environment { runtimes, profiles: loaded.profiles, diagnostics }
}

/// Model and reasoning values a runtime's CLI advertises, for the profile editor.
pub async fn runtime_options(runtime_id: &str, runtimes: &[Runtime]) -> RuntimeOptions {
    let Some(runtime) = runtimes.iter().find(|runtime| runtime.id == runtime_id) else {
        return RuntimeOptions::empty(runtime_id, "unknown", "Unknown runtime; Relay cannot read its options");
    };
    match relay_adapters::adapter_by_id(&runtime.adapter_id) {
        Some(adapter) => adapter.report_options(runtime_id).await,
        None => RuntimeOptions::empty(
            runtime_id,
            &runtime.adapter_id,
            format!("{} exposes no model or runtime options", runtime.adapter_id),
        ),
    }
}

/// The daemon's implementation of the HTTP layer's service contract.
pub struct DaemonService {
    config: ConfigStore,
    environment: RwLock<Environment>,
}

impl DaemonService {
    pub async fn new(config: ConfigStore) -> Self {
        let environment = detect_environment(&config).await;
        Self { config, environment: RwLock::new(environment) }
    }

    pub fn environment(&self) -> Environment {
        self.environment.read().unwrap().clone()
    }

    fn set_profiles(&self, profiles: Vec<AgentProfile>) {
        let mut environment = self.environment.write().unwrap();
        environment.profiles = profiles;
    }

    fn view(&self, config: &RelayConfig) -> RelayConfigView {
        RelayConfigView {
            profiles: config.profiles.clone(),
            policy: config.policy,
            workspace_overrides: config.workspace_overrides.clone(),
            manual_runtimes: config.manual_runtimes.clone(),
            warnings: config.warnings.clone(),
            revision: config.revision.clone(),
        }
    }

    async fn redetect(&self) -> (Environment, RelayConfigView) {
        let environment = detect_environment(&self.config).await;
        let view = self.view(&self.config.read());
        *self.environment.write().unwrap() = environment.clone();
        (environment, view)
    }

}

#[async_trait]
impl EnvironmentService for DaemonService {
    fn environment(&self) -> Environment {
        DaemonService::environment(self)
    }

    fn config(&self) -> RelayConfigView {
        self.view(&self.config.read())
    }

    async fn refresh(&self) -> (Environment, RelayConfigView) {
        self.redetect().await
    }

    async fn runtime_options(&self, runtime_id: &str) -> RuntimeOptions {
        let runtimes = self.environment().runtimes;
        runtime_options(runtime_id, &runtimes).await
    }

    async fn probe(&self, _adapter_id: &str, executable_path: &str) -> RuntimeProbe {
        probe_executable(executable_path).await
    }

    async fn save_runtime(&self, id: &str, body: RuntimeBody) -> RuntimeMutation {
        let probe = probe_executable(&body.executable_path).await;
        if !probe.ok {
            return RuntimeMutation { config: self.view(&self.config.read()), probe };
        }
        let entry = ManualRuntime {
            id: id.to_string(),
            adapter_id: body.adapter_id,
            executable_path: body.executable_path,
            label: body.label.filter(|label| !label.trim().is_empty()),
        };
        match self.config.upsert_manual_runtime(&entry) {
            Ok(_) => {
                let (_, view) = self.redetect().await;
                RuntimeMutation { config: view, probe }
            }
            Err(error) => RuntimeMutation {
                config: self.view(&self.config.read()),
                probe: RuntimeProbe { ok: false, version: None, error: Some(error.message().to_string()) },
            },
        }
    }

    fn delete_runtime(&self, id: &str) -> RelayConfigView {
        let view = match self.config.remove_manual_runtime(id) {
            Ok(config) => self.view(&config),
            Err(error) => {
                tracing::warn!("failed to remove runtime {id}: {}", error.message());
                self.view(&self.config.read())
            }
        };
        {
            let mut environment = self.environment.write().unwrap();
            environment.runtimes.retain(|runtime| runtime.id != id);
        }
        view
    }

    fn save_profile(&self, profile: AgentProfile) -> Result<RelayConfigView, String> {
        let config = self
            .config
            .upsert_profile(&profile)
            .map_err(|error| error.message().to_string())?;
        self.set_profiles(config.profiles.clone());
        Ok(self.view(&config))
    }

    fn delete_profile(&self, id: &str) -> Result<RelayConfigView, String> {
        let config = self.config.remove_profile(id).map_err(|error| error.message().to_string())?;
        self.set_profiles(config.profiles.clone());
        Ok(self.view(&config))
    }

    fn save_policy(&self, body: PolicyBody) -> Result<RelayConfigView, String> {
        let overrides: BTreeMap<String, RelayPolicyOverride> = body.workspace_overrides;
        let config = self
            .config
            .write_policy(body.policy, overrides)
            .map_err(|error| error.message().to_string())?;
        Ok(self.view(&config))
    }

    fn adapter_ids(&self) -> Vec<String> {
        adapter_ids()
    }
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

