//! The runtime adapters Relay ships with.
//!
//! One adapter per CLI, all behind `relay_core::AgentAdapter`. Nothing in this
//! crate knows about profiles, policy or the event log: an adapter is handed a
//! task and a working directory and hands back normalized events.

pub mod antigravity;
pub mod cli;
pub mod deepseek;
mod environment;
pub mod grok;
pub mod instructions;
pub mod kimi;
pub mod models;
pub mod probe;
pub mod yaml;
pub mod zai;

use std::sync::Arc;

use relay_core::AgentAdapter;

pub use cli::{ProcessSupervisor, StreamMode, StreamSpec};
pub use deepseek::DeepSeekAdapter;
pub use grok::GrokAdapter;

/// Every adapter this build supports, freshly constructed.
pub fn adapters() -> Vec<Arc<dyn AgentAdapter>> {
    vec![
        Arc::new(deepseek::DeepSeekAdapter::new()),
        Arc::new(antigravity::AntigravityAdapter::new()),
        Arc::new(kimi::KimiAdapter::new()),
        Arc::new(zai::ZaiAdapter::new()),
        Arc::new(grok::GrokAdapter::new()),
    ]
}

/// Adapter ids the control panel may offer when registering a runtime by hand.
pub fn adapter_ids() -> Vec<String> {
    adapters()
        .iter()
        .map(|adapter| adapter.id().to_string())
        .collect()
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
        assert_eq!(
            ids,
            vec![
                "deepseek-harness",
                "antigravity-cli",
                "kimi-code",
                "zai-cli",
                "grok-cli"
            ]
        );
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

        let grok = adapter_by_id("grok-cli").unwrap();
        assert!(grok.capabilities().resume);
        assert!(grok.capabilities().cancel);
        assert!(!grok.capabilities().send);
        assert!(!grok.capabilities().child_sessions);
    }

    /// Relay's policy gates whether a run may start. Only a runtime with a real
    /// sandbox may claim it enforces anything, or the panel would be lying.
    #[test]
    fn enforcement_is_declared_only_where_a_sandbox_exists() {
        let deepseek = adapter_by_id("deepseek-harness").unwrap();
        assert!(deepseek.capabilities().enforcement.workspace);
        assert!(!deepseek.capabilities().enforcement.commands);
        assert!(!deepseek.capabilities().enforcement.network);

        let grok = adapter_by_id("grok-cli").unwrap();
        assert!(grok.capabilities().enforcement.workspace);
        assert!(!grok.capabilities().enforcement.commands);
        assert!(!grok.capabilities().enforcement.network);

        for id in ["kimi-code", "zai-cli", "antigravity-cli"] {
            let adapter = adapter_by_id(id).unwrap();
            assert!(
                !adapter.capabilities().enforcement.enforces_anything(),
                "{id} must not claim enforcement it does not have"
            );
        }
    }
}
