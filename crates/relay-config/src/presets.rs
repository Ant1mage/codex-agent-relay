//! Profile templates.
//!
//! Runtime detection never creates an Agent Profile: finding a CLI is not a
//! decision about what to run with it. These presets are offered to the user as
//! starting points, and are only written to configuration when someone picks one.

use relay_core::{AgentProfile, CapabilitySet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfilePreset {
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub capabilities: CapabilitySet,
}

const CODING: ProfilePreset = ProfilePreset {
    key: "coding",
    name: "Coding",
    description: "Coding worker with workspace write and command capabilities.",
    capabilities: CapabilitySet {
        read_workspace: true,
        write_workspace: true,
        execute_commands: true,
        network_access: false,
    },
};

const RESEARCH: ProfilePreset = ProfilePreset {
    key: "research",
    name: "Research",
    description: "Read-only research worker with network access.",
    capabilities: CapabilitySet {
        read_workspace: true,
        write_workspace: false,
        execute_commands: false,
        network_access: true,
    },
};

/// The presets the control panel offers. Applying one produces a normal
/// `AgentProfile` bound to the runtime the user picked.
pub fn profile_presets() -> &'static [ProfilePreset] {
    &[CODING, RESEARCH]
}

/// Builds a profile from a preset. The caller supplies the id and the runtime.
pub fn profile_from_preset(preset: &ProfilePreset, id: &str, runtime_id: &str) -> AgentProfile {
    AgentProfile {
        id: id.to_string(),
        name: preset.name.to_string(),
        runtime_id: runtime_id.to_string(),
        description: preset.description.to_string(),
        instructions: None,
        model: None,
        reasoning: None,
        capabilities: preset.capabilities,
        enabled: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_templates_not_instances() {
        let presets = profile_presets();
        assert_eq!(presets.len(), 2);
        let profile = profile_from_preset(&presets[0], "agent-1", "runtime:deepseek-harness");
        assert_eq!(profile.id, "agent-1");
        assert_eq!(profile.runtime_id, "runtime:deepseek-harness");
        assert!(profile.capabilities.write_workspace);
        assert!(!presets[1].capabilities.write_workspace);
    }
}
