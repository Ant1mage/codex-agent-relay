//! The runtime adapters Relay ships with.
//!
//! One adapter per CLI, all behind `relay_core::AgentAdapter`. Nothing in this
//! crate knows about profiles, policy or the event log: an adapter is handed a
//! task and a working directory and hands back normalized events.

pub mod antigravity;
pub mod cli;
pub mod deepseek;
pub mod kimi;
pub mod models;
pub mod probe;
pub mod zai;

use std::sync::Arc;

use relay_core::AgentAdapter;

pub use cli::{ProcessSupervisor, StreamMode, StreamSpec};
pub use deepseek::DeepSeekAdapter;

/// Every adapter this build supports, freshly constructed.
pub fn adapters() -> Vec<Arc<dyn AgentAdapter>> {
    vec![
        Arc::new(deepseek::DeepSeekAdapter::new()),
        Arc::new(antigravity::AntigravityAdapter::new()),
        Arc::new(kimi::KimiAdapter::new()),
        Arc::new(zai::ZaiAdapter::new()),
    ]
}

/// Adapter ids the control panel may offer when registering a runtime by hand.
pub fn adapter_ids() -> Vec<String> {
    adapters().iter().map(|adapter| adapter.id().to_string()).collect()
}

/// Finds the adapter that owns an id.
pub fn adapter_by_id(id: &str) -> Option<Arc<dyn AgentAdapter>> {
    adapters().into_iter().find(|adapter| adapter.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_matches_the_supported_runtimes() {
        let ids = adapter_ids();
        assert_eq!(ids, vec!["deepseek-harness", "antigravity-cli", "kimi-code", "zai-cli"]);
        assert!(adapter_by_id("deepseek-harness").is_some());
        assert!(adapter_by_id("missing").is_none());
    }

    #[test]
    fn capabilities_are_declared_honestly() {
        let deepseek = adapter_by_id("deepseek-harness").unwrap();
        assert!(deepseek.capabilities().cancel);
        assert!(!deepseek.capabilities().send);

        let kimi = adapter_by_id("kimi-code").unwrap();
        assert!(!kimi.capabilities().resume);

        let antigravity = adapter_by_id("antigravity-cli").unwrap();
        assert!(antigravity.capabilities().resume);
        assert!(antigravity.capabilities().child_sessions);
    }
}
