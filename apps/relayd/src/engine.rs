//! The daemon's execution engine.
//!
//! Relay owns execution, never orchestration: the engine starts an external
//! Agent CLI, supervises its process, normalizes what it prints into Relay events
//! and answers status/cancel/resume. It never plans, never reasons and never
//! routes between agents — the host (Codex) does that.
//!
//! Every front-end reaches this one instance over loopback HTTP: the tray, the
//! control panel, and the MCP server Codex talks to. Because the daemon is the
//! only worker owner, a front-end dying — including Codex's MCP process — cannot
//! take a running worker with it.

use std::sync::Arc;

use async_trait::async_trait;
use relay_api::{RunProjectionView, RunService, RunStartBody, RunStarted, SessionContext};
use relay_codex::CodexThreadMetadataResolver;
use relay_core::{
    AccessMode, AgentProfile, HostSession, HostSessionStatus, HostSessionStore, HostSessionUpsert,
    RelayError, RunController, RunRequest, RuntimeHealth,
};

use crate::reloader::RuntimeConfigReloader;

pub struct RelayEngine {
    controller: Arc<RunController>,
    sessions: Arc<dyn HostSessionStore>,
    codex_threads: Arc<dyn CodexThreadMetadataResolver>,
    reloader: Arc<RuntimeConfigReloader>,
}

impl RelayEngine {
    pub fn new(
        controller: Arc<RunController>,
        sessions: Arc<dyn HostSessionStore>,
        codex_threads: Arc<dyn CodexThreadMetadataResolver>,
        reloader: Arc<RuntimeConfigReloader>,
    ) -> Self {
        Self {
            controller,
            sessions,
            codex_threads,
            reloader,
        }
    }

    pub fn controller(&self) -> Arc<RunController> {
        Arc::clone(&self.controller)
    }

    async fn sync(&self, session: &SessionContext) -> Result<HostSession, RelayError> {
        let metadata = self.codex_threads.resolve(&session.thread_id).await?;
        self.sessions.upsert_codex(HostSessionUpsert {
            native_session_id: metadata.id,
            display_name: metadata.display_name,
            cwd: metadata.cwd,
            model: metadata.model,
            status: HostSessionStatus::Active,
        })
    }

    /// Profiles this host session can actually run: enabled, and backed by a
    /// runtime whose CLI answered detection.
    fn usable_profiles(&self) -> Vec<AgentProfile> {
        let available: Vec<String> = self
            .controller
            .runtimes
            .list()
            .into_iter()
            .filter(|runtime| runtime.health == RuntimeHealth::Available)
            .map(|runtime| runtime.id)
            .collect();
        self.controller
            .profiles
            .list(true)
            .into_iter()
            .filter(|profile| available.contains(&profile.runtime_id))
            .collect()
    }

    fn view(&self, projection: relay_core::RunProjection) -> RunProjectionView {
        RunProjectionView::from_projection(&projection)
    }
}

/// Relay's error taxonomy is useful to a host; the HTTP layer only carries text,
/// so the message (not the code) is what a tool call reports.
fn service_error(error: RelayError) -> String {
    error.message().to_string()
}

#[async_trait]
impl RunService for RelayEngine {
    async fn list_agents(&self, session: &SessionContext) -> Result<Vec<AgentProfile>, String> {
        self.reloader.refresh().await.map_err(service_error)?;
        self.sync(session).await.map_err(service_error)?;
        Ok(self.usable_profiles())
    }

    async fn sync_session(&self, session: &SessionContext) -> Result<HostSession, String> {
        self.sync(session).await.map_err(service_error)
    }

    async fn end_session(&self, session: &SessionContext) -> Result<Option<HostSession>, String> {
        let session = self.sync(session).await.map_err(service_error)?;
        let ended = self
            .sessions
            .upsert_codex(HostSessionUpsert {
                native_session_id: session.native_session_id.clone(),
                display_name: session.display_name.clone(),
                cwd: session.cwd.clone(),
                model: session.model.clone(),
                status: HostSessionStatus::Ended,
            })
            .map_err(service_error)?;
        self.controller.policies.clear_session(&session.id);
        Ok(Some(ended))
    }

    async fn end_session_by_native_id(
        &self,
        native_session_id: &str,
    ) -> Result<Option<HostSession>, String> {
        let id = format!("codex:{native_session_id}");
        let Some(existing) = self.sessions.get(&id).map_err(service_error)? else {
            return Ok(None);
        };
        let ended = self
            .sessions
            .upsert_codex(HostSessionUpsert {
                native_session_id: native_session_id.to_string(),
                display_name: existing.display_name,
                cwd: existing.cwd,
                model: existing.model,
                status: HostSessionStatus::Ended,
            })
            .map_err(service_error)?;
        self.controller.policies.clear_session(&ended.id);
        Ok(Some(ended))
    }

    async fn start(&self, body: RunStartBody) -> Result<RunStarted, String> {
        let session = self.sync(&body.session).await.map_err(service_error)?;
        // Configuration first, then policy for this workspace, then the run.
        self.reloader.refresh().await.map_err(service_error)?;
        self.reloader.apply_workspace(&session.cwd);
        let active = self
            .controller
            .start(RunRequest {
                host_session_id: session.id.clone(),
                profile_id: body.agent_id,
                task: body.task,
                cwd: session.cwd.clone(),
                access_mode: body.access_mode.unwrap_or(AccessMode::ReadOnly),
                isolation: body.isolation.unwrap_or_default(),
            })
            .await
            .map_err(service_error)?;
        Ok(RunStarted {
            run_id: active.run_id,
            worker_session_id: active.worker_id,
            host_session_display_name: session.display_name,
        })
    }

    async fn resume(&self, worker_session_id: &str, feedback: &str) -> Result<RunStarted, String> {
        self.reloader.refresh().await.map_err(service_error)?;
        let active = self
            .controller
            .resume(worker_session_id, feedback)
            .await
            .map_err(service_error)?;
        Ok(RunStarted {
            run_id: active.run_id,
            worker_session_id: active.worker_id,
            host_session_display_name: String::new(),
        })
    }

    async fn status(&self, worker_session_id: &str) -> Result<RunProjectionView, String> {
        self.controller
            .get_by_worker(worker_session_id)
            .map(|projection| self.view(projection))
            .map_err(service_error)
    }

    async fn wait(&self, worker_session_id: &str) -> Result<RunProjectionView, String> {
        self.controller
            .wait_for_worker(worker_session_id)
            .await
            .map(|projection| self.view(projection))
            .map_err(service_error)
    }

    async fn send(&self, worker_session_id: &str, message: &str) -> Result<(), String> {
        self.controller
            .send(worker_session_id, message)
            .await
            .map_err(service_error)
    }

    async fn cancel(&self, worker_session_id: &str) -> Result<(), String> {
        self.controller
            .cancel_worker(worker_session_id)
            .await
            .map_err(service_error)
    }

    async fn accept(&self, worker_session_id: &str) -> Result<RunProjectionView, String> {
        self.controller
            .accept_worker(worker_session_id)
            .map(|projection| self.view(projection))
            .map_err(service_error)
    }

    async fn cancel_session(&self, host_session_id: &str) -> Result<u32, String> {
        let doomed: Vec<String> = self
            .controller
            .list_active()
            .into_iter()
            .filter(|(run, _worker)| run.host_session_id == host_session_id)
            .map(|(run, _worker)| run.id)
            .collect();
        let mut cancelled = 0;
        for run_id in doomed {
            if self.controller.cancel(&run_id).await.is_ok() {
                cancelled += 1;
            }
        }
        Ok(cancelled)
    }

    async fn delete_session(&self, host_session_id: &str) -> Result<(), String> {
        let doomed: Vec<String> = self
            .controller
            .list_active()
            .into_iter()
            .filter(|(run, _worker)| run.host_session_id == host_session_id)
            .map(|(run, _worker)| run.id)
            .collect();
        for run_id in doomed {
            let _ = self.controller.cancel(&run_id).await;
        }

        self.controller.policies.clear_session(host_session_id);

        self.sessions
            .delete(host_session_id)
            .map_err(service_error)?;

        Ok(())
    }
}

/// Ends every worker this daemon owns, then waits for the supervisor tasks to
/// finish. Called on the way out: the daemon is the parent of these processes, so
/// they must not outlive it.
pub async fn shutdown(controller: &Arc<RunController>) {
    let active = controller.list_active();
    if active.is_empty() {
        controller.adapters.dispose_all().await;
        return;
    }
    for (run, _worker) in active {
        if let Err(error) = controller.cancel(&run.id).await {
            tracing::warn!(
                "could not cancel run {} on shutdown: {}",
                run.id,
                error.message()
            );
        }
    }
    // The supervisor tasks append the terminal events; give them a moment, then
    // make sure nothing is left running.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    controller.adapters.dispose_all().await;
}
