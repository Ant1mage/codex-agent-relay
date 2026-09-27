//! The daemon's read side.
//!
//! The MCP process owns execution and writes events; the daemon only reads the
//! log, projects it, and appends cancellation requests to the control queue. It
//! deliberately never creates a worker.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use relay_core::{
    project_run, AgentProfile, EventStore, HostSessionStore, RelayEvent, RunProjection, RunStatus,
    Runtime, RuntimeHealth,
};
use relay_storage::{SqliteControlQueue, SqliteEventStore, SqliteHostSessionStore};

use crate::contract::{
    CodexStatus, EventBatch, InspectorSnapshot, MenuAgent, MenuBlocked, MenuSession, MenuStatus,
    MenuView, MenuWorker, RunView, SessionView,
};
use crate::environment::Environment;

/// Cached projections, invalidated by a cheap revision stamp.
struct Cache {
    revision: String,
    sessions: Vec<SessionView>,
    runs: Vec<RunView>,
    events: HashMap<String, Vec<RelayEvent>>,
}

pub struct RelayStore {
    events: Arc<SqliteEventStore>,
    sessions: Arc<SqliteHostSessionStore>,
    commands: Arc<SqliteControlQueue>,
    environment: RwLock<Environment>,
    cache: RwLock<Option<Cache>>,
}

impl RelayStore {
    pub fn new(
        events: Arc<SqliteEventStore>,
        sessions: Arc<SqliteHostSessionStore>,
        commands: Arc<SqliteControlQueue>,
    ) -> Self {
        Self {
            events,
            sessions,
            commands,
            environment: RwLock::new(Environment::default()),
            cache: RwLock::new(None),
        }
    }

    pub fn set_environment(&self, environment: Environment) {
        *self.environment.write().unwrap() = environment;
    }

    pub fn environment(&self) -> Environment {
        self.environment.read().unwrap().clone()
    }

    pub fn events(&self) -> Arc<SqliteEventStore> {
        Arc::clone(&self.events)
    }

    pub fn sessions_store(&self) -> Arc<SqliteHostSessionStore> {
        Arc::clone(&self.sessions)
    }

    /// Cheap change stamp for the SSE tick.
    pub fn revision(&self) -> String {
        let events = self
            .events
            .revision()
            .unwrap_or_else(|_| "error".to_string());
        let environment = self.environment.read().unwrap();
        format!(
            "{}|{}|{}",
            events,
            environment.runtimes.len(),
            environment.profiles.len()
        )
    }

    fn projected(
        &self,
    ) -> (
        Vec<SessionView>,
        Vec<RunView>,
        HashMap<String, Vec<RelayEvent>>,
    ) {
        let revision = self.revision();
        if let Some(cache) = self.cache.read().unwrap().as_ref() {
            if cache.revision == revision {
                return (
                    cache.sessions.clone(),
                    cache.runs.clone(),
                    cache.events.clone(),
                );
            }
        }

        let sessions = self.sessions.list().unwrap_or_default();
        let run_ids = self.events.list_run_ids().unwrap_or_default();
        let mut runs: Vec<RunView> = Vec::new();
        let mut events: HashMap<String, Vec<RelayEvent>> = HashMap::new();
        for run_id in run_ids {
            let stored = self.events.list(&run_id).unwrap_or_default();
            if stored.is_empty() {
                continue;
            }
            if let Ok(projection) = project_run(&stored) {
                runs.push(RunView {
                    run: projection.run,
                    steps: projection.steps,
                    workers: projection.workers,
                });
                events.insert(run_id, stored);
            }
        }
        runs.sort_by(|left, right| right.run.created_at.cmp(&left.run.created_at));

        let views: Vec<SessionView> = sessions
            .into_iter()
            .map(|session| {
                let session_runs: Vec<RunView> = runs
                    .iter()
                    .filter(|view| view.run.host_session_id == session.id)
                    .cloned()
                    .collect();
                let active_workers = session_runs
                    .iter()
                    .map(|view| {
                        view.workers
                            .iter()
                            .filter(|worker| worker.status.is_active())
                            .count() as u32
                    })
                    .sum();
                let awaiting_host = session_runs
                    .iter()
                    .filter(|view| view.run.status == RunStatus::AwaitingHost)
                    .count() as u32;
                SessionView {
                    session,
                    runs: session_runs,
                    active_workers,
                    awaiting_host,
                }
            })
            .collect();

        *self.cache.write().unwrap() = Some(Cache {
            revision,
            sessions: views.clone(),
            runs: runs.clone(),
            events: events.clone(),
        });
        (views, runs, events)
    }

    pub fn run_views(&self) -> Vec<RunView> {
        self.projected().1
    }

    pub fn sessions_with_runs(&self) -> Vec<SessionView> {
        self.projected().0
    }

    pub fn projection(&self, run_id: &str) -> Option<RunProjection> {
        let stored = self.events.list(run_id).ok()?;
        if stored.is_empty() {
            return None;
        }
        project_run(&stored).ok()
    }

    pub fn events_for(&self, run_id: &str, after: u64) -> EventBatch {
        let events = self
            .projected()
            .2
            .get(run_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.seq > after)
            .collect();
        EventBatch {
            run_id: run_id.to_string(),
            events,
        }
    }

    /// Highest sequence per run, used as the SSE fan-out cursor.
    pub fn cursors(&self) -> BTreeMap<String, u64> {
        self.projected()
            .2
            .iter()
            .filter_map(|(run_id, events)| events.last().map(|event| (run_id.clone(), event.seq)))
            .collect()
    }

    pub fn snapshot(&self, codex: CodexStatus) -> InspectorSnapshot {
        let environment = self.environment.read().unwrap().clone();
        InspectorSnapshot {
            sessions: self.sessions_with_runs(),
            runtimes: environment.runtimes,
            profiles: environment.profiles,
            diagnostics: environment.diagnostics,
            codex,
            generated_at: relay_core::now(),
        }
    }

    /// The compact projection the tray renders: the same facts as the snapshot,
    /// minus what a menu cannot show, so the two surfaces cannot disagree.
    pub fn menu(&self, codex: CodexStatus) -> MenuView {
        let environment = self.environment.read().unwrap().clone();
        let sessions = self.sessions_with_runs();
        let profile_name = |id: &str| {
            environment
                .profiles
                .iter()
                .find(|profile| profile.id == id)
                .map(|profile| profile.name.clone())
                .unwrap_or_else(|| id.to_string())
        };

        let menu_sessions: Vec<MenuSession> = sessions
            .iter()
            .map(|view| MenuSession {
                id: view.session.id.clone(),
                display_name: view.session.display_name.clone(),
                cwd: view.session.cwd.clone(),
                active_workers: view
                    .runs
                    .iter()
                    .flat_map(|run_view| {
                        run_view
                            .workers
                            .iter()
                            .filter(|worker| worker.status.is_active())
                            .map(|worker| {
                                let step =
                                    run_view.steps.iter().find(|step| step.id == worker.step_id);
                                let profile = profile_name(
                                    step.map(|step| step.profile_id.as_str())
                                        .unwrap_or(&run_view.run.profile_id),
                                );
                                let task = task_preview(
                                    step.map(|step| step.task.as_str())
                                        .unwrap_or(&run_view.run.task),
                                    48,
                                );
                                MenuWorker {
                                    worker_session_id: worker.id.clone(),
                                    run_id: run_view.run.id.clone(),
                                    label: if task.is_empty() {
                                        profile
                                    } else {
                                        format!("{profile} · {task}")
                                    },
                                }
                            })
                    })
                    .collect(),
            })
            .collect();

        let agents: Vec<MenuAgent> = environment
            .profiles
            .iter()
            .map(|profile| {
                let runtime = environment
                    .runtimes
                    .iter()
                    .find(|runtime| runtime.id == profile.runtime_id);
                let blocked = match runtime {
                    None => Some(MenuBlocked::Missing),
                    Some(runtime) if runtime.health == RuntimeHealth::Unavailable => {
                        Some(MenuBlocked::Missing)
                    }
                    Some(runtime) if runtime.health == RuntimeHealth::AuthenticationRequired => {
                        Some(MenuBlocked::Auth)
                    }
                    Some(_) if !profile.enabled => Some(MenuBlocked::Disabled),
                    _ => None,
                };
                MenuAgent {
                    id: profile.id.clone(),
                    name: profile.name.clone(),
                    blocked,
                }
            })
            .collect();

        let running_workers = menu_sessions
            .iter()
            .map(|session| session.active_workers.len() as u32)
            .sum();
        let awaiting_host = sessions.iter().map(|view| view.awaiting_host).sum();
        MenuView {
            status: menu_status(&environment.runtimes, &environment.profiles, &codex),
            running_workers,
            awaiting_host,
            sessions: menu_sessions,
            agents,
            runtimes: environment.runtimes.clone(),
            codex,
        }
    }

    /// Appends to the queue the MCP process drains.
    pub fn cancel_worker(&self, worker_session_id: &str) -> relay_core::Result<()> {
        self.commands.enqueue_cancel(worker_session_id)?;
        Ok(())
    }

    pub fn cancel_session(&self, host_session_id: &str) -> relay_core::Result<u32> {
        let mut active = Vec::new();
        for view in self.projected().1 {
            if view.run.host_session_id != host_session_id {
                continue;
            }
            for worker in view.workers {
                if worker.status.is_active() {
                    active.push(worker.id);
                }
            }
        }
        for worker_session_id in &active {
            self.cancel_worker(worker_session_id)?;
        }
        Ok(active.len() as u32)
    }
}

/// Relay's environment state: a runtime that can run, and Codex wired up.
pub fn menu_status(
    runtimes: &[Runtime],
    profiles: &[AgentProfile],
    codex: &CodexStatus,
) -> MenuStatus {
    if runtimes.is_empty() {
        return MenuStatus::NoRuntime;
    }
    let usable = profiles.iter().any(|profile| {
        profile.enabled
            && runtimes.iter().any(|runtime| {
                runtime.id == profile.runtime_id && runtime.health == RuntimeHealth::Available
            })
    });
    if codex.configured && usable {
        MenuStatus::Ready
    } else {
        MenuStatus::NeedsSetup
    }
}

/// First line of a delegated task, cut to a length a menu can show.
pub fn task_preview(task: &str, max: usize) -> String {
    let line = task
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim())
        .unwrap_or_default();
    let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max {
        let cut: String = collapsed.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_previews_collapse_whitespace_and_truncate() {
        assert_eq!(task_preview("Fix the bug\nmore detail", 48), "Fix the bug");
        assert_eq!(task_preview("   spaced   out   ", 48), "spaced out");
        assert_eq!(task_preview("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn menu_status_reports_the_environment() {
        let codex = CodexStatus {
            checks: Vec::new(),
            configured: true,
        };
        assert_eq!(menu_status(&[], &[], &codex), MenuStatus::NoRuntime);
        assert_eq!(
            menu_status(&[runtime()], &[], &codex),
            MenuStatus::NeedsSetup
        );
        assert_eq!(
            menu_status(&[runtime()], &[profile()], &codex),
            MenuStatus::Ready
        );
        assert_eq!(
            menu_status(&[runtime()], &[profile()], &CodexStatus::unknown()),
            MenuStatus::NeedsSetup
        );
    }

    fn runtime() -> Runtime {
        Runtime {
            id: "runtime:deepseek-harness".into(),
            adapter_id: "deepseek-harness".into(),
            executable_path: "/usr/local/bin/dsh".into(),
            version: None,
            health: RuntimeHealth::Available,
            capabilities: Default::default(),
        }
    }

    fn profile() -> AgentProfile {
        AgentProfile {
            id: "agent-1".into(),
            name: "DeepSeek Code".into(),
            runtime_id: "runtime:deepseek-harness".into(),
            description: "d".into(),
            instructions: None,
            model: None,
            reasoning: None,
            capabilities: Default::default(),
            enabled: true,
        }
    }
}
