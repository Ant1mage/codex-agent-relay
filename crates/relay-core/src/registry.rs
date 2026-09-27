//! Registries for adapters, runtimes and profiles.
//!
//! Runtimes and profiles are re-synced from detection and from configuration
//! while workers are running, so `sync` never disturbs an active worker: it only
//! changes what future runs observe.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::adapter::AgentAdapter;
use crate::domain::{AgentProfile, RelayError, Result, Runtime};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

impl SyncReport {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

#[derive(Default)]
pub struct AdapterRegistry {
    items: RwLock<BTreeMap<String, Arc<dyn AgentAdapter>>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, adapter: Arc<dyn AgentAdapter>) -> Result<()> {
        let id = adapter.id().to_string();
        let mut items = self.items.write().unwrap();
        if items.contains_key(&id) {
            return Err(RelayError::new(
                "ADAPTER_CONFLICT",
                format!("Adapter {id} is already registered"),
            ));
        }
        items.insert(id, adapter);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AgentAdapter>> {
        self.items.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Arc<dyn AgentAdapter>> {
        self.items.read().unwrap().values().cloned().collect()
    }

    pub fn ids(&self) -> Vec<String> {
        self.items.read().unwrap().keys().cloned().collect()
    }

    pub async fn dispose_all(&self) {
        let adapters = self.list();
        for adapter in adapters {
            adapter.dispose().await;
        }
    }
}

#[derive(Default)]
pub struct RuntimeRegistry {
    items: RwLock<BTreeMap<String, Runtime>>,
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, runtime: Runtime) -> Result<()> {
        let mut items = self.items.write().unwrap();
        if items.contains_key(&runtime.id) {
            return Err(RelayError::new(
                "RUNTIME_CONFLICT",
                format!("Runtime {} is already registered", runtime.id),
            ));
        }
        items.insert(runtime.id.clone(), runtime);
        Ok(())
    }

    pub fn require(&self, id: &str) -> Result<Runtime> {
        self.items
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| RelayError::runtime_not_found(id))
    }

    pub fn get(&self, id: &str) -> Option<Runtime> {
        self.items.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Runtime> {
        self.items.read().unwrap().values().cloned().collect()
    }

    /// Replaces the detected set without disturbing active workers.
    pub fn sync(&self, runtimes: Vec<Runtime>) -> SyncReport {
        let next: BTreeMap<String, Runtime> = runtimes
            .into_iter()
            .map(|runtime| (runtime.id.clone(), runtime))
            .collect();
        let mut items = self.items.write().unwrap();
        let mut report = SyncReport::default();
        for (id, runtime) in next.iter() {
            match items.get(id) {
                None => report.added.push(id.clone()),
                Some(current) if current != runtime => report.updated.push(id.clone()),
                Some(_) => {}
            }
            items.insert(id.clone(), runtime.clone());
        }
        let removed: Vec<String> = items
            .keys()
            .filter(|id| !next.contains_key(*id))
            .cloned()
            .collect();
        for id in &removed {
            items.remove(id);
        }
        report.removed = removed;
        report
    }
}

#[derive(Default)]
pub struct ProfileRegistry {
    items: RwLock<BTreeMap<String, AgentProfile>>,
}

impl ProfileRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, profile: AgentProfile) -> Result<()> {
        let mut items = self.items.write().unwrap();
        if items.contains_key(&profile.id) {
            return Err(RelayError::new(
                "PROFILE_CONFLICT",
                format!("Profile {} is already registered", profile.id),
            ));
        }
        items.insert(profile.id.clone(), profile);
        Ok(())
    }

    pub fn require(&self, id: &str) -> Result<AgentProfile> {
        self.items
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| RelayError::profile_not_found(id))
    }

    pub fn get(&self, id: &str) -> Option<AgentProfile> {
        self.items.read().unwrap().get(id).cloned()
    }

    pub fn list(&self, enabled_only: bool) -> Vec<AgentProfile> {
        self.items
            .read()
            .unwrap()
            .values()
            .filter(|profile| !enabled_only || profile.enabled)
            .cloned()
            .collect()
    }

    /// Replaces the registered set with what is on disk now: profiles are user
    /// configuration, so a long-running process follows edits without a restart.
    pub fn sync(&self, profiles: Vec<AgentProfile>) -> SyncReport {
        let next: BTreeMap<String, AgentProfile> = profiles
            .into_iter()
            .map(|profile| (profile.id.clone(), profile))
            .collect();
        let mut items = self.items.write().unwrap();
        let mut report = SyncReport::default();
        for (id, profile) in next.iter() {
            match items.get(id) {
                None => report.added.push(id.clone()),
                Some(current) if current != profile => report.updated.push(id.clone()),
                Some(_) => {}
            }
            items.insert(id.clone(), profile.clone());
        }
        let removed: Vec<String> = items
            .keys()
            .filter(|id| !next.contains_key(*id))
            .cloned()
            .collect();
        for id in &removed {
            items.remove(id);
        }
        report.removed = removed;
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AdapterCapabilities, CapabilitySet, RuntimeHealth};

    fn runtime(id: &str, version: &str) -> Runtime {
        Runtime {
            id: id.into(),
            adapter_id: "fake".into(),
            executable_path: "/usr/bin/true".into(),
            version: Some(version.into()),
            health: RuntimeHealth::Available,
            capabilities: AdapterCapabilities::default(),
        }
    }

    fn profile(id: &str, enabled: bool) -> AgentProfile {
        AgentProfile {
            id: id.into(),
            name: id.into(),
            runtime_id: "runtime:fake".into(),
            description: "d".into(),
            instructions: None,
            model: None,
            reasoning: None,
            capabilities: CapabilitySet::default(),
            enabled,
        }
    }

    #[test]
    fn runtime_sync_reports_additions_updates_and_removals() {
        let registry = RuntimeRegistry::new();
        let report = registry.sync(vec![runtime("a", "1"), runtime("b", "1")]);
        assert_eq!(report.added, vec!["a".to_string(), "b".to_string()]);

        let report = registry.sync(vec![runtime("a", "2"), runtime("c", "1")]);
        assert_eq!(report.updated, vec!["a".to_string()]);
        assert_eq!(report.added, vec!["c".to_string()]);
        assert_eq!(report.removed, vec!["b".to_string()]);
    }

    #[test]
    fn profile_sync_keeps_disabled_profiles_visible() {
        let registry = ProfileRegistry::new();
        registry.sync(vec![profile("a", false), profile("b", true)]);
        assert_eq!(registry.list(false).len(), 2);
        assert_eq!(registry.list(true).len(), 1);
        assert!(registry.require("missing").is_err());
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let registry = RuntimeRegistry::new();
        registry.register(runtime("a", "1")).unwrap();
        assert_eq!(
            registry.register(runtime("a", "1")).unwrap_err().code(),
            "RUNTIME_CONFLICT"
        );
    }
}
