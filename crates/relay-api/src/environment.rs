//! Everything the daemon knows about this machine that is not in the event log.

use relay_core::{AgentProfile, Runtime};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Environment {
    pub runtimes: Vec<Runtime>,
    pub profiles: Vec<AgentProfile>,
    pub diagnostics: Vec<String>,
}

impl Environment {
    pub fn runtime(&self, runtime_id: &str) -> Option<&Runtime> {
        self.runtimes
            .iter()
            .find(|runtime| runtime.id == runtime_id)
    }
}
