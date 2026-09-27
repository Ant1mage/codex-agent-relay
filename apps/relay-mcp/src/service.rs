//! The Relay application service behind the MCP tools.
//!
//! MCP request → Relay core call → MCP response. Core never learns about MCP.
//! The session lifecycle is deliberate:
//!
//! ```text
//! worker completed → Run awaiting_host → Codex review → accept → Run completed
//! ```

use std::sync::Arc;

use relay_codex::CodexThreadMetadataResolver;
use relay_core::{
    AgentProfile, HostSession, HostSessionStore, HostSessionUpsert, Result, RunController,
    RunProjection,
};

use crate::config_reloader::RuntimeConfigReloader;
use crate::context::CodexInvocationContext;

#[derive(Debug, Clone, Default)]
pub struct RunAgentInput {
    pub agent_id: String,
    pub task: String,
    pub access_mode: Option<relay_core::AccessMode>,
    pub isolation: Option<relay_core::Isolation>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentResult {
    pub run_id: String,
    pub worker_session_id: String,
    pub host_session_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeResult {
    pub run_id: String,
    pub worker_session_id: String,
}

pub struct RelayService {
    pub controller: Arc<RunController>,
    pub sessions: Arc<dyn HostSessionStore>,
    pub codex_threads: Arc<dyn CodexThreadMetadataResolver>,
    pub reloader: Arc<RuntimeConfigReloader>,
}

impl RelayService {
    pub async fn sync_session(&self, context: &CodexInvocationContext) -> Result<HostSession> {
        let metadata = self.codex_threads.resolve(&context.thread_id).await?;
        self.sessions.upsert_codex(HostSessionUpsert {
            native_session_id: metadata.id,
            display_name: metadata.display_name,
            cwd: metadata.cwd,
            model: metadata.model,
            status: relay_core::HostSessionStatus::Active,
        })
    }

    pub async fn end_session(&self, context: &CodexInvocationContext) -> Result<()> {
        let session = self.sync_session(context).await?;
        self.sessions.upsert_codex(HostSessionUpsert {
            native_session_id: session.native_session_id.clone(),
            display_name: session.display_name.clone(),
            cwd: session.cwd.clone(),
            model: session.model.clone(),
            status: relay_core::HostSessionStatus::Ended,
        })?;
        self.controller.policies.clear_session(&session.id);
        Ok(())
    }

    /// Ends a session by native id, used by the `--session-end-hook` entry point.
    pub fn end_session_by_native_id(&self, native_session_id: &str) -> Result<Option<HostSession>> {
        let id = format!("codex:{native_session_id}");
        let Some(existing) = self.sessions.get(&id)? else {
            return Ok(None);
        };
        let session = self.sessions.upsert_codex(HostSessionUpsert {
            native_session_id: native_session_id.to_string(),
            display_name: existing.display_name,
            cwd: existing.cwd,
            model: existing.model,
            status: relay_core::HostSessionStatus::Ended,
        })?;
        self.controller.policies.clear_session(&session.id);
        Ok(Some(session))
    }

    pub async fn list_agents(&self, context: &CodexInvocationContext) -> Result<Vec<AgentProfile>> {
        self.reloader.refresh().await?;
        self.sync_session(context).await?;
        let available: Vec<String> = self
            .controller
            .runtimes
            .list()
            .into_iter()
            .filter(|runtime| runtime.health == relay_core::RuntimeHealth::Available)
            .map(|runtime| runtime.id)
            .collect();
        Ok(self
            .controller
            .profiles
            .list(true)
            .into_iter()
            .filter(|profile| available.contains(&profile.runtime_id))
            .collect())
    }

    pub async fn run_agent(
        &self,
        context: &CodexInvocationContext,
        input: RunAgentInput,
    ) -> Result<RunAgentResult> {
        let session = self.sync_session(context).await?;
        // Configuration first, then policy for this workspace, then the run.
        self.reloader.refresh().await?;
        self.reloader.apply_workspace(&session.cwd);
        let active = self
            .controller
            .start(relay_core::RunRequest {
                host_session_id: session.id.clone(),
                profile_id: input.agent_id,
                task: input.task,
                cwd: session.cwd.clone(),
                access_mode: input
                    .access_mode
                    .unwrap_or(relay_core::AccessMode::ReadOnly),
                isolation: input.isolation.unwrap_or_default(),
            })
            .await?;
        Ok(RunAgentResult {
            run_id: active.run_id,
            worker_session_id: active.worker_id,
            host_session_display_name: session.display_name,
        })
    }

    pub fn status(&self, worker_session_id: &str) -> Result<RunProjection> {
        self.controller.get_by_worker(worker_session_id)
    }

    pub async fn wait(&self, worker_session_id: &str) -> Result<RunProjection> {
        self.controller.wait_for_worker(worker_session_id).await
    }

    pub async fn send(&self, worker_session_id: &str, message: &str) -> Result<()> {
        self.controller.send(worker_session_id, message).await
    }

    pub async fn cancel(&self, worker_session_id: &str) -> Result<()> {
        self.controller.cancel_worker(worker_session_id).await
    }

    pub fn accept(&self, worker_session_id: &str) -> Result<RunProjection> {
        self.controller.accept_worker(worker_session_id)
    }

    pub async fn resume(&self, worker_session_id: &str, feedback: &str) -> Result<ResumeResult> {
        self.reloader.refresh().await?;
        let active = self.controller.resume(worker_session_id, feedback).await?;
        Ok(ResumeResult {
            run_id: active.run_id,
            worker_session_id: active.worker_id,
        })
    }
}
