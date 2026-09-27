//! The Run lifecycle: start, supervise, cancel, resume and accept.
//!
//! This is a port of the behaviour Relay already had, not a redesign:
//!
//! ```text
//! worker completed → Run awaiting_host → Codex review → accept → Run completed
//! ```
//!
//! `resume` continues the *same* Step with an incremented iteration, on top of
//! the runtime's own native session resume.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{watch, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::adapter::{AdapterEvent, AgentAdapter, WorkerHandle};
use crate::domain::{
    now, AccessMode, AgentProfile, Isolation, RelayError, Result, ResumeInput, Run, RunRequest,
    RunStatus, RuntimeHealth, StartInput, Step, StepStatus, WorkerSession, WorkerStatus,
};
use crate::event::{bound_native_event, RelayEvent, RelayEventType};
use crate::policy::{assert_policy_allows, normalize_path, PolicyResolver, PolicyScope};
use crate::projection::{project_run, RunProjection};
use crate::registry::{AdapterRegistry, ProfileRegistry, RuntimeRegistry};
use crate::store::EventStore;

/// A run that is currently executing in this process.
#[derive(Debug)]
pub struct ActiveRun {
    pub run_id: String,
    pub step_id: String,
    pub worker_id: String,
    /// Resolves when the run reaches a terminal state.
    pub completion: tokio::task::JoinHandle<()>,
}

/// Everything one executing run needs, shared between the supervisor task and
/// whoever cancels it.
struct RunState {
    run: Mutex<Run>,
    step: Mutex<Step>,
    worker: Mutex<WorkerSession>,
    adapter: Arc<dyn AgentAdapter>,
    events: Arc<dyn EventStore>,
    /// Serialises "reserve a sequence number and write it".
    write: AsyncMutex<()>,
    seq: Mutex<u64>,
    cancel_requested: AtomicBool,
    terminal: AtomicBool,
    native_session_id: Mutex<Option<String>>,
    done: watch::Sender<bool>,
}

impl RunState {
    fn run(&self) -> Run {
        self.run.lock().unwrap().clone()
    }

    fn step(&self) -> Step {
        self.step.lock().unwrap().clone()
    }

    fn worker(&self) -> WorkerSession {
        self.worker.lock().unwrap().clone()
    }

    fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::SeqCst)
    }

    fn ids(&self) -> (String, String, String) {
        (
            self.run.lock().unwrap().id.clone(),
            self.step.lock().unwrap().id.clone(),
            self.worker.lock().unwrap().id.clone(),
        )
    }

    /// Appends one event, keeping `seq` strictly increasing per run.
    async fn append_with(
        &self,
        event_type: RelayEventType,
        data: serde_json::Value,
        native_event: Option<serde_json::Value>,
        include_worker: bool,
        include_step: bool,
    ) -> Result<()> {
        let _guard = self.write.lock().await;
        let (run_id, step_id, worker_id) = self.ids();
        let seq = {
            let mut seq = self.seq.lock().unwrap();
            *seq += 1;
            *seq
        };
        let event = RelayEvent {
            id: Uuid::new_v4().to_string(),
            run_id,
            step_id: include_step.then_some(step_id),
            worker_session_id: include_worker.then_some(worker_id),
            seq,
            timestamp: now(),
            event_type,
            data,
            native_event: native_event.map(bound_native_event),
        };
        let store = Arc::clone(&self.events);
        tokio::task::spawn_blocking(move || store.append(event))
            .await
            .map_err(|error| RelayError::new("STORAGE_FAILURE", error.to_string()))?
    }

    async fn append(&self, event_type: RelayEventType, data: serde_json::Value) -> Result<()> {
        self.append_with(event_type, data, None, true, true).await
    }

    /// Terminal event: records the state change, then the event itself.
    async fn finish(
        &self,
        event_type: RelayEventType,
        data: serde_json::Value,
        native_event: Option<serde_json::Value>,
    ) {
        if self.terminal.swap(true, Ordering::SeqCst) {
            return;
        }
        let status = event_type
            .terminal_worker_status()
            .unwrap_or(WorkerStatus::Failed);
        let run_status = match status {
            WorkerStatus::Completed => RunStatus::AwaitingHost,
            WorkerStatus::Failed => RunStatus::Failed,
            _ => RunStatus::Cancelled,
        };
        let stamp = now();
        {
            let mut worker = self.worker.lock().unwrap();
            worker.status = status;
            worker.ended_at = Some(stamp.clone());
        }
        {
            let mut run = self.run.lock().unwrap();
            run.status = run_status;
            run.updated_at = stamp.clone();
        }
        {
            let mut step = self.step.lock().unwrap();
            step.status = StepStatus::from(run_status);
            step.updated_at = stamp;
        }
        if let Err(error) = self
            .append_with(event_type, data, native_event, true, true)
            .await
        {
            tracing::error!("failed to append terminal event: {}", error.message());
        }
        if status == WorkerStatus::Completed {
            let step_id = self.step.lock().unwrap().id.clone();
            if let Err(error) = self
                .append_with(
                    RelayEventType::RunAwaitingHost,
                    serde_json::json!({ "stepId": step_id }),
                    None,
                    false,
                    false,
                )
                .await
            {
                tracing::error!("failed to append run/awaiting_host: {}", error.message());
            }
        }
    }

    fn mark_done(&self) {
        let _ = self.done.send(true);
    }
}

pub struct RunController {
    events: Arc<dyn EventStore>,
    pub adapters: Arc<AdapterRegistry>,
    pub runtimes: Arc<RuntimeRegistry>,
    pub profiles: Arc<ProfileRegistry>,
    pub policies: Arc<PolicyResolver>,
    active: Arc<Mutex<HashMap<String, Arc<RunState>>>>,
}

impl RunController {
    pub fn new(events: Arc<dyn EventStore>) -> Self {
        Self {
            events,
            adapters: Arc::new(AdapterRegistry::new()),
            runtimes: Arc::new(RuntimeRegistry::new()),
            profiles: Arc::new(ProfileRegistry::new()),
            policies: Arc::new(PolicyResolver::new(Default::default())),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_registries(
        events: Arc<dyn EventStore>,
        adapters: Arc<AdapterRegistry>,
        runtimes: Arc<RuntimeRegistry>,
        profiles: Arc<ProfileRegistry>,
        policies: Arc<PolicyResolver>,
    ) -> Self {
        Self {
            events,
            adapters,
            runtimes,
            profiles,
            policies,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn events(&self) -> Arc<dyn EventStore> {
        Arc::clone(&self.events)
    }

    pub fn list_active(&self) -> Vec<(Run, WorkerSession)> {
        let mut values: Vec<(Run, WorkerSession)> = self
            .active
            .lock()
            .unwrap()
            .values()
            .map(|state| (state.run(), state.worker()))
            .collect();
        values.sort_by(|left, right| left.0.created_at.cmp(&right.0.created_at));
        values
    }

    pub fn active_run_ids(&self) -> Vec<String> {
        self.active.lock().unwrap().keys().cloned().collect()
    }

    fn active_state(&self, run_id: &str) -> Option<Arc<RunState>> {
        self.active.lock().unwrap().get(run_id).cloned()
    }

    pub fn get(&self, run_id: &str) -> Result<RunProjection> {
        let events = self.events.list(run_id)?;
        if events.is_empty() {
            return Err(RelayError::run_not_found(run_id));
        }
        project_run(&events)
    }

    /// Which run owns a worker. Active runs answer first: a worker exists the
    /// moment it starts, not only once its `worker/started` row is committed.
    pub fn run_id_for_worker(&self, worker_session_id: &str) -> Result<String> {
        let active = self
            .active
            .lock()
            .unwrap()
            .values()
            .find(|state| state.worker.lock().unwrap().id == worker_session_id)
            .map(|state| state.run.lock().unwrap().id.clone());
        if let Some(run_id) = active {
            return Ok(run_id);
        }
        self.events
            .find_run_id_by_worker(worker_session_id)?
            .ok_or_else(|| RelayError::worker_not_found(worker_session_id))
    }

    pub fn get_by_worker(&self, worker_session_id: &str) -> Result<RunProjection> {
        let run_id = self.run_id_for_worker(worker_session_id)?;
        self.get(&run_id)
    }

    /// Waits until the run leaves the active set, then returns its projection.
    pub async fn wait(&self, run_id: &str) -> Result<RunProjection> {
        if let Some(state) = self.active_state(run_id) {
            let mut receiver = state.done.subscribe();
            while !*receiver.borrow_and_update() {
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        }
        self.get(run_id)
    }

    pub async fn wait_for_worker(&self, worker_session_id: &str) -> Result<RunProjection> {
        let run_id = self.run_id_for_worker(worker_session_id)?;
        self.wait(&run_id).await
    }

    pub async fn send(&self, worker_session_id: &str, message: &str) -> Result<()> {
        let state = self
            .active
            .lock()
            .unwrap()
            .values()
            .find(|state| state.worker.lock().unwrap().id == worker_session_id)
            .cloned()
            .ok_or_else(|| {
                RelayError::worker_not_found(&format!("Worker {worker_session_id} is not active"))
            })?;
        let native_session_id = state.native_session_id.lock().unwrap().clone();
        let Some(native_session_id) = native_session_id else {
            return Err(RelayError::worker_not_found(&format!(
                "Worker {worker_session_id} is starting"
            )));
        };
        state.adapter.send(&native_session_id, message).await
    }

    pub async fn cancel_worker(&self, worker_session_id: &str) -> Result<()> {
        let run_id = self
            .active
            .lock()
            .unwrap()
            .values()
            .find(|state| state.worker.lock().unwrap().id == worker_session_id)
            .map(|state| state.run.lock().unwrap().id.clone())
            .ok_or_else(|| {
                RelayError::worker_not_found(&format!("Worker {worker_session_id} is not active"))
            })?;
        self.cancel(&run_id).await
    }

    pub fn accept(&self, run_id: &str) -> Result<RunProjection> {
        let events = self.events.list(run_id)?;
        if events.is_empty() {
            return Err(RelayError::run_not_found(run_id));
        }
        let projection = project_run(&events)?;
        if projection.run.status != RunStatus::AwaitingHost {
            return Err(RelayError::invalid_state(format!(
                "Run {run_id} cannot be accepted from {}",
                status_name(projection.run.status)
            )));
        }
        self.events.append(RelayEvent {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            step_id: None,
            worker_session_id: None,
            seq: events.len() as u64 + 1,
            timestamp: now(),
            event_type: RelayEventType::RunAccepted,
            data: serde_json::json!({ "acceptedBy": "host" }),
            native_event: None,
        })?;
        self.get(run_id)
    }

    pub fn accept_worker(&self, worker_session_id: &str) -> Result<RunProjection> {
        let run_id = self.run_id_for_worker(worker_session_id)?;
        self.accept(&run_id)
    }

    /// Starts one delegation.
    pub async fn start(&self, request: RunRequest) -> Result<ActiveRun> {
        let profile = self.profiles.require(&request.profile_id)?;
        if !profile.enabled {
            return Err(RelayError::new(
                "PROFILE_DISABLED",
                format!("Profile {} is disabled", profile.id),
            ));
        }
        if request.access_mode == AccessMode::Write && !profile.capabilities.write_workspace {
            return Err(RelayError::new(
                "CAPABILITY_DENIED",
                format!("Profile {} cannot write to the workspace", profile.id),
            ));
        }
        let runtime = self.runtimes.require(&profile.runtime_id)?;
        if runtime.health != RuntimeHealth::Available {
            return Err(RelayError::new(
                "RUNTIME_UNAVAILABLE",
                format!("Runtime {} is not available", runtime.id),
            ));
        }
        let adapter = self.adapters.get(&runtime.adapter_id).ok_or_else(|| {
            RelayError::new(
                "RUNTIME_UNAVAILABLE",
                format!("No adapter for {}", runtime.adapter_id),
            )
        })?;

        let policy = self.policies.resolve(PolicyScope {
            workspace: Some(&request.cwd),
            host_session_id: Some(&request.host_session_id),
        });
        assert_policy_allows(policy, &request, &profile)?;
        self.assert_concurrency(
            &request,
            policy.max_concurrent_runs,
            policy.max_concurrent_writers,
        )?;
        self.assert_workspace_isolation(&request, policy.require_worktree_for_parallel_writers)?;

        let stamp = now();
        let run = Run {
            id: Uuid::new_v4().to_string(),
            host_session_id: request.host_session_id.clone(),
            profile_id: request.profile_id.clone(),
            task: request.task.clone(),
            cwd: request.cwd.clone(),
            access_mode: request.access_mode,
            isolation: request.isolation,
            status: RunStatus::Queued,
            created_at: stamp.clone(),
            updated_at: stamp.clone(),
        };
        let step = Step {
            id: Uuid::new_v4().to_string(),
            run_id: run.id.clone(),
            profile_id: request.profile_id.clone(),
            task: request.task.clone(),
            access_mode: request.access_mode,
            isolation: request.isolation,
            status: StepStatus::Queued,
            iteration: 1,
            created_at: stamp.clone(),
            updated_at: stamp.clone(),
        };
        let worker = WorkerSession {
            id: Uuid::new_v4().to_string(),
            run_id: run.id.clone(),
            step_id: step.id.clone(),
            iteration: step.iteration,
            runtime_id: runtime.id.clone(),
            native_session_id: None,
            parent_worker_session_id: None,
            process_id: None,
            process: None,
            status: WorkerStatus::Starting,
            started_at: stamp.clone(),
            ended_at: None,
        };

        let state = Arc::new(self.new_state(run, step, worker, adapter));
        let (run_id, step_id, worker_id) = state.ids();

        // The first two rows are committed before the run is observable.
        state
            .append_with(
                RelayEventType::RunCreated,
                serde_json::json!({ "run": state.run() }),
                None,
                false,
                false,
            )
            .await?;
        state
            .append_with(
                RelayEventType::StepCreated,
                serde_json::json!({ "step": state.step() }),
                None,
                false,
                true,
            )
            .await?;

        let starting = now();
        {
            let mut run = state.run.lock().unwrap();
            run.status = RunStatus::Starting;
            run.updated_at = starting.clone();
        }
        {
            let mut step = state.step.lock().unwrap();
            step.status = StepStatus::Starting;
            step.updated_at = starting;
        }

        self.active
            .lock()
            .unwrap()
            .insert(run_id.clone(), Arc::clone(&state));
        let completion = self.spawn_execute(
            Arc::clone(&state),
            profile,
            None,
            runtime.executable_path.clone(),
        );
        Ok(ActiveRun {
            run_id,
            step_id,
            worker_id,
            completion,
        })
    }

    /// Continues the same Step after Codex review, on the original native session.
    pub async fn resume(&self, worker_session_id: &str, feedback: &str) -> Result<ActiveRun> {
        let task = feedback.trim().to_string();
        if task.is_empty() {
            return Err(RelayError::new(
                "INVALID_REQUEST",
                "Resume feedback must not be empty",
            ));
        }
        let run_id = self.run_id_for_worker(worker_session_id)?;
        if self.active_state(&run_id).is_some() {
            return Err(RelayError::invalid_state(format!(
                "Run {run_id} is already active"
            )));
        }
        let events = self.events.list(&run_id)?;
        let projection = project_run(&events)?;
        if projection.run.status != RunStatus::AwaitingHost {
            return Err(RelayError::invalid_state(format!(
                "Run {run_id} cannot resume from {}",
                status_name(projection.run.status)
            )));
        }
        let previous_worker = projection
            .worker(worker_session_id)
            .cloned()
            .ok_or_else(|| RelayError::worker_not_found(worker_session_id))?;
        let previous_step = projection
            .step(&previous_worker.step_id)
            .cloned()
            .ok_or_else(|| {
                RelayError::invalid_state(format!("Worker {worker_session_id} has no Step"))
            })?;
        let profile = self.profiles.require(&previous_step.profile_id)?;
        let runtime = self.runtimes.require(&profile.runtime_id)?;
        let adapter = self.adapters.get(&runtime.adapter_id).ok_or_else(|| {
            RelayError::new(
                "RUNTIME_UNAVAILABLE",
                format!("No adapter for {}", runtime.adapter_id),
            )
        })?;
        let unsupported = || {
            RelayError::new(
                "OPERATION_UNSUPPORTED",
                format!(
                    "Runtime {} cannot resume this worker; start a new Step instead",
                    runtime.id
                ),
            )
        };
        if !runtime.capabilities.resume {
            return Err(unsupported());
        }
        let Some(native_session_id) = previous_worker.native_session_id.clone() else {
            return Err(unsupported());
        };

        let request = RunRequest {
            host_session_id: projection.run.host_session_id.clone(),
            profile_id: previous_step.profile_id.clone(),
            task: previous_step.task.clone(),
            cwd: projection.run.cwd.clone(),
            access_mode: previous_step.access_mode,
            isolation: previous_step.isolation,
        };
        let policy = self.policies.resolve(PolicyScope {
            workspace: Some(&request.cwd),
            host_session_id: Some(&request.host_session_id),
        });
        assert_policy_allows(policy, &request, &profile)?;
        self.assert_concurrency(
            &request,
            policy.max_concurrent_runs,
            policy.max_concurrent_writers,
        )?;
        self.assert_workspace_isolation(&request, policy.require_worktree_for_parallel_writers)?;

        let stamp = now();
        let mut run = projection.run.clone();
        run.status = RunStatus::Starting;
        run.updated_at = stamp.clone();
        let mut step = previous_step.clone();
        step.iteration += 1;
        step.status = StepStatus::Starting;
        step.updated_at = stamp.clone();
        let worker = WorkerSession {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.clone(),
            step_id: step.id.clone(),
            iteration: step.iteration,
            runtime_id: runtime.id.clone(),
            native_session_id: None,
            parent_worker_session_id: None,
            process_id: None,
            process: None,
            status: WorkerStatus::Starting,
            started_at: stamp.clone(),
            ended_at: None,
        };

        let resume_access_mode = step.access_mode;
        let state = Arc::new(self.new_state(run, step, worker, adapter));
        *state.seq.lock().unwrap() = events.len() as u64;
        let (_, step_id, worker_id) = state.ids();
        state
            .append_with(
                RelayEventType::StepIterationStarted,
                serde_json::json!({ "step": state.step() }),
                None,
                false,
                false,
            )
            .await?;

        let resume_input = ResumeInput {
            run_id: run_id.clone(),
            worker_session_id: worker_id.clone(),
            native_session_id,
            task,
            cwd: request.cwd.clone(),
            access_mode: resume_access_mode,
            executable_path: Some(runtime.executable_path.clone()),
            // DeepSeek Harness re-reads its default model selection when it adopts
            // an existing session, so the selection has to travel with the resume:
            // without it the resumed run would quietly use the profile default.
            model: profile.model.clone(),
            reasoning: profile.reasoning.clone(),
            instructions: profile.instructions.clone(),
        };

        self.active
            .lock()
            .unwrap()
            .insert(run_id.clone(), Arc::clone(&state));
        let completion = self.spawn_execute(
            Arc::clone(&state),
            profile,
            Some(resume_input),
            runtime.executable_path.clone(),
        );
        Ok(ActiveRun {
            run_id,
            step_id,
            worker_id,
            completion,
        })
    }

    /// Cancels an active run: native process first, then the terminal event.
    pub async fn cancel(&self, run_id: &str) -> Result<()> {
        let Some(state) = self.active_state(run_id) else {
            return Err(RelayError::new(
                "RUN_NOT_FOUND",
                format!("Run {run_id} is not active"),
            ));
        };
        if state.is_terminal() || state.cancel_requested.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let worker_id = state.worker.lock().unwrap().id.clone();
        let native_session_id = state
            .native_session_id
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(worker_id);
        match state.adapter.cancel(&native_session_id).await {
            Ok(()) => {
                state
                    .finish(
                        RelayEventType::WorkerCancelled,
                        serde_json::json!({ "requestedBy": "host" }),
                        None,
                    )
                    .await;
                Ok(())
            }
            Err(error) => {
                state
                    .finish(
                        RelayEventType::WorkerFailed,
                        serde_json::json!({ "message": error.message(), "operation": "cancel" }),
                        None,
                    )
                    .await;
                Err(RelayError::new(
                    "ADAPTER_FAILURE",
                    format!("Adapter failed to cancel run {run_id}"),
                ))
            }
        }
    }

    fn new_state(
        &self,
        run: Run,
        step: Step,
        worker: WorkerSession,
        adapter: Arc<dyn AgentAdapter>,
    ) -> RunState {
        let (done, _) = watch::channel(false);
        RunState {
            run: Mutex::new(run),
            step: Mutex::new(step),
            worker: Mutex::new(worker),
            adapter,
            events: Arc::clone(&self.events),
            write: AsyncMutex::new(()),
            seq: Mutex::new(0),
            cancel_requested: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
            native_session_id: Mutex::new(None),
            done,
        }
    }

    fn spawn_execute(
        &self,
        state: Arc<RunState>,
        profile: AgentProfile,
        resume_input: Option<ResumeInput>,
        executable_path: String,
    ) -> tokio::task::JoinHandle<()> {
        let run_id = state.run.lock().unwrap().id.clone();
        let active = Arc::clone(&self.active);
        tokio::spawn(async move {
            execute(Arc::clone(&state), profile, resume_input, executable_path).await;
            state.mark_done();
            active.lock().unwrap().remove(&run_id);
        })
    }

    fn assert_concurrency(
        &self,
        request: &RunRequest,
        max_runs: u32,
        max_writers: u32,
    ) -> Result<()> {
        let active = self.active.lock().unwrap();
        if active.len() as u32 >= max_runs {
            return Err(RelayError::new(
                "CONCURRENCY_LIMIT",
                format!("Concurrent run limit of {max_runs} reached"),
            ));
        }
        let writers = active
            .values()
            .filter(|state| state.run.lock().unwrap().access_mode.is_write())
            .count() as u32;
        if request.access_mode.is_write() && writers >= max_writers {
            return Err(RelayError::new(
                "CONCURRENCY_LIMIT",
                format!("Concurrent writer limit of {max_writers} reached"),
            ));
        }
        Ok(())
    }

    fn assert_workspace_isolation(
        &self,
        request: &RunRequest,
        require_worktree: bool,
    ) -> Result<()> {
        if !require_worktree
            || !request.access_mode.is_write()
            || request.isolation == Isolation::Worktree
        {
            return Ok(());
        }
        let workspace = normalize_path(&request.cwd);
        let conflicting = self.active.lock().unwrap().values().find_map(|state| {
            let run = state.run.lock().unwrap();
            (run.access_mode.is_write()
                && run.isolation == Isolation::Shared
                && normalize_path(&run.cwd) == workspace)
                .then(|| run.id.clone())
        });
        if let Some(run_id) = conflicting {
            return Err(RelayError::new(
                "WORKSPACE_CONFLICT",
                format!("Run {run_id} is already writing to {workspace}"),
            ));
        }
        Ok(())
    }
}

/// The supervisor task: start (or resume) the worker, then map its events.
async fn execute(
    state: Arc<RunState>,
    profile: AgentProfile,
    resume_input: Option<ResumeInput>,
    executable_path: String,
) {
    let adapter = Arc::clone(&state.adapter);
    let run = state.run();
    let worker = state.worker();

    let handle: Result<WorkerHandle> = match &resume_input {
        Some(input) => {
            let mut input = input.clone();
            if input.executable_path.is_none() {
                input.executable_path = Some(executable_path.clone());
            }
            adapter.resume(input).await
        }
        None => {
            adapter
                .start(StartInput {
                    run_id: run.id.clone(),
                    worker_session_id: worker.id.clone(),
                    task: run.task.clone(),
                    cwd: run.cwd.clone(),
                    access_mode: run.access_mode,
                    executable_path: Some(executable_path.clone()),
                    model: profile.model.clone(),
                    reasoning: profile.reasoning.clone(),
                    instructions: profile.instructions.clone(),
                })
                .await
        }
    };

    let mut handle = match handle {
        Ok(handle) => handle,
        Err(error) => {
            state
                .finish(
                    RelayEventType::WorkerFailed,
                    serde_json::json!({ "message": error.message() }),
                    None,
                )
                .await;
            return;
        }
    };

    if let Some(native_session_id) = handle.native_session_id.clone() {
        *state.native_session_id.lock().unwrap() = Some(native_session_id.clone());
        state.worker.lock().unwrap().native_session_id = Some(native_session_id);
    }
    if let Some(process) = handle.process.clone() {
        // The full identity is what a later daemon verifies before it ever signals
        // this pid; the bare process id stays for the wire contract.
        let mut worker = state.worker.lock().unwrap();
        worker.process_id = Some(process.pid);
        worker.process = Some(process);
    } else if let Some(process_id) = handle.process_id {
        state.worker.lock().unwrap().process_id = Some(process_id);
    }

    if state.cancel_requested.load(Ordering::SeqCst) {
        let native_session_id = state
            .native_session_id
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| state.worker.lock().unwrap().id.clone());
        let _ = adapter.cancel(&native_session_id).await;
        state
            .finish(
                RelayEventType::WorkerCancelled,
                serde_json::json!({ "requestedBy": "host" }),
                None,
            )
            .await;
        return;
    }

    let stamp = now();
    {
        let mut worker = state.worker.lock().unwrap();
        worker.status = WorkerStatus::Running;
    }
    {
        let mut run = state.run.lock().unwrap();
        run.status = RunStatus::Running;
        run.updated_at = stamp.clone();
    }
    {
        let mut step = state.step.lock().unwrap();
        step.status = StepStatus::Running;
        step.updated_at = stamp;
    }
    if let Err(error) = state
        .append(
            RelayEventType::WorkerStarted,
            serde_json::json!({ "worker": state.worker() }),
        )
        .await
    {
        tracing::error!("failed to append worker/started: {}", error.message());
    }

    while let Some(event) = handle.events.recv().await {
        if state.is_terminal() {
            break;
        }
        let AdapterEvent {
            event_type,
            data,
            native_event,
        } = event;
        if event_type.terminal_worker_status().is_some() {
            state.finish(event_type, data, native_event).await;
        } else if let Err(error) = state
            .append_with(event_type, data, native_event, true, true)
            .await
        {
            tracing::error!("failed to append adapter event: {}", error.message());
        }
    }

    if !state.is_terminal() {
        state
            .finish(
                RelayEventType::WorkerFailed,
                serde_json::json!({
                    "message": format!("Adapter {} ended without a terminal event", adapter.id())
                }),
                None,
            )
            .await;
    }
}

fn status_name(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::Starting => "starting",
        RunStatus::Running => "running",
        RunStatus::AwaitingHost => "awaiting_host",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Interrupted => "interrupted",
        RunStatus::Orphaned => "orphaned",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CapabilitySet, RelayPolicy};
    use crate::test_support::{
        controller_with, fake_profile, fake_request, fake_runtime, ControlledAdapter, FakeAdapter,
    };
    use std::sync::Arc;

    fn events_of(controller: &RunController, run_id: &str) -> Vec<RelayEventType> {
        controller
            .events()
            .list(run_id)
            .unwrap()
            .into_iter()
            .map(|event| event.event_type)
            .collect()
    }

    #[tokio::test]
    async fn runs_a_task_through_a_fake_adapter_and_awaits_the_host() {
        let controller = controller_with(
            Arc::new(FakeAdapter::new()),
            fake_runtime("fake", false),
            fake_profile("runtime:fake"),
        );
        let active = controller
            .start(fake_request(
                "/tmp/project",
                AccessMode::Write,
                "codex:test-session",
            ))
            .await
            .unwrap();
        active.completion.await.unwrap();

        assert_eq!(
            events_of(&controller, &active.run_id),
            vec![
                RelayEventType::RunCreated,
                RelayEventType::StepCreated,
                RelayEventType::WorkerStarted,
                RelayEventType::WorkerMessage,
                RelayEventType::ToolRead,
                RelayEventType::WorkerCompleted,
                RelayEventType::RunAwaitingHost,
            ]
        );
        let projection = controller.get(&active.run_id).unwrap();
        assert_eq!(projection.run.status, RunStatus::AwaitingHost);
        assert_eq!(projection.steps[0].status, StepStatus::AwaitingHost);
        assert_eq!(projection.workers[0].status, WorkerStatus::Completed);
        assert_eq!(projection.result.unwrap()["summary"], "Fake task completed");
        assert!(controller.list_active().is_empty());

        let accepted = controller.accept(&active.run_id).unwrap();
        assert_eq!(accepted.run.status, RunStatus::Completed);
        assert_eq!(accepted.steps[0].status, StepStatus::Completed);
        assert_eq!(
            events_of(&controller, &active.run_id).last(),
            Some(&RelayEventType::RunAccepted)
        );
    }

    #[tokio::test]
    async fn passes_configured_model_and_reasoning_and_executable_to_the_adapter() {
        let adapter = Arc::new(FakeAdapter::new());
        let mut profile = fake_profile("runtime:fake");
        profile.model = Some("DeepSeek Pro".to_string());
        profile.reasoning = Some("high".to_string());
        let controller = controller_with(adapter.clone(), fake_runtime("fake", false), profile);

        let active = controller
            .start(fake_request(
                "/tmp/project",
                AccessMode::ReadOnly,
                "codex:model-settings",
            ))
            .await
            .unwrap();
        active.completion.await.unwrap();

        let input = adapter.last_start_input.lock().unwrap().clone().unwrap();
        assert_eq!(input.model.as_deref(), Some("DeepSeek Pro"));
        assert_eq!(input.reasoning.as_deref(), Some("high"));
        assert_eq!(input.executable_path.as_deref(), Some("/usr/bin/true"));
    }

    #[tokio::test]
    async fn resume_reuses_the_step_with_an_incremented_iteration() {
        let controller = controller_with(
            Arc::new(FakeAdapter::resumable()),
            fake_runtime("fake", true),
            fake_profile("runtime:fake"),
        );
        let first = controller
            .start(fake_request(
                "/tmp/project",
                AccessMode::ReadOnly,
                "codex:resume-session",
            ))
            .await
            .unwrap();
        first.completion.await.unwrap();

        let resumed = controller
            .resume(&first.worker_id, "Please verify the result.")
            .await
            .unwrap();
        resumed.completion.await.unwrap();

        let projection = controller.get(&first.run_id).unwrap();
        assert_eq!(projection.run.status, RunStatus::AwaitingHost);
        assert_eq!(projection.steps.len(), 1);
        assert_eq!(projection.steps[0].id, first.step_id);
        assert_eq!(projection.steps[0].iteration, 2);
        assert_eq!(projection.steps[0].status, StepStatus::AwaitingHost);
        assert_eq!(projection.workers.len(), 2);
        assert_eq!(projection.workers[1].step_id, first.step_id);
        assert_eq!(projection.workers[1].iteration, 2);
        assert_eq!(projection.workers[1].status, WorkerStatus::Completed);
        assert!(
            events_of(&controller, &first.run_id).contains(&RelayEventType::StepIterationStarted)
        );
    }

    #[tokio::test]
    async fn resume_is_refused_when_the_runtime_cannot_resume() {
        let controller = controller_with(
            Arc::new(FakeAdapter::new()),
            fake_runtime("fake", false),
            fake_profile("runtime:fake"),
        );
        let first = controller
            .start(fake_request(
                "/tmp/project",
                AccessMode::ReadOnly,
                "codex:no-resume",
            ))
            .await
            .unwrap();
        first.completion.await.unwrap();
        let error = controller
            .resume(&first.worker_id, "again")
            .await
            .unwrap_err();
        assert_eq!(error.code(), "OPERATION_UNSUPPORTED");
    }

    #[tokio::test]
    async fn rejects_two_shared_writers_in_the_same_workspace() {
        let policy = RelayPolicy {
            max_concurrent_writers: 2,
            ..RelayPolicy::default()
        };
        let controller = Arc::new(RunController::new(Arc::new(
            crate::store::MemoryEventStore::new(),
        )));
        controller.policies.set_global(policy);
        controller
            .adapters
            .register(Arc::new(ControlledAdapter::new()))
            .unwrap();
        controller
            .runtimes
            .register(fake_runtime("controlled", false))
            .unwrap();
        controller
            .profiles
            .register(fake_profile("runtime:controlled"))
            .unwrap();

        let first = controller
            .start(fake_request(
                "/tmp/relay-project",
                AccessMode::Write,
                "codex:one",
            ))
            .await
            .unwrap();
        let error = controller
            .start(fake_request(
                "/tmp/relay-project",
                AccessMode::Write,
                "codex:two",
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "WORKSPACE_CONFLICT");

        controller.cancel(&first.run_id).await.unwrap();
        let _ = first.completion.await;
    }

    #[tokio::test]
    async fn cancellation_is_the_terminal_event() {
        let controller = Arc::new(RunController::new(Arc::new(
            crate::store::MemoryEventStore::new(),
        )));
        controller
            .adapters
            .register(Arc::new(ControlledAdapter::new()))
            .unwrap();
        controller
            .runtimes
            .register(fake_runtime("controlled", false))
            .unwrap();
        controller
            .profiles
            .register(fake_profile("runtime:controlled"))
            .unwrap();

        let active = controller
            .start(fake_request(
                "/tmp/relay-cancel",
                AccessMode::Write,
                "codex:cancel",
            ))
            .await
            .unwrap();
        // Give the supervisor a moment to publish worker/started.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        controller.cancel(&active.run_id).await.unwrap();
        active.completion.await.unwrap();

        let projection = controller.get(&active.run_id).unwrap();
        assert_eq!(projection.run.status, RunStatus::Cancelled);
        assert_eq!(projection.workers[0].status, WorkerStatus::Cancelled);
        assert_eq!(
            events_of(&controller, &active.run_id).last(),
            Some(&RelayEventType::WorkerCancelled)
        );
        assert!(controller.list_active().is_empty());
    }

    #[tokio::test]
    async fn a_disabled_profile_or_missing_capability_never_starts_a_worker() {
        let controller = controller_with(
            Arc::new(FakeAdapter::new()),
            fake_runtime("fake", false),
            fake_profile("runtime:fake"),
        );
        let mut disabled = fake_profile("runtime:fake");
        disabled.id = "profile:disabled".into();
        disabled.enabled = false;
        controller.profiles.register(disabled).unwrap();
        let mut request = fake_request("/tmp/project", AccessMode::ReadOnly, "codex:s");
        request.profile_id = "profile:disabled".into();
        assert_eq!(
            controller.start(request).await.unwrap_err().code(),
            "PROFILE_DISABLED"
        );

        let mut read_only = fake_profile("runtime:fake");
        read_only.id = "profile:read-only".into();
        read_only.capabilities = CapabilitySet {
            write_workspace: false,
            ..read_only.capabilities
        };
        controller.profiles.register(read_only).unwrap();
        let mut request = fake_request("/tmp/project", AccessMode::Write, "codex:s");
        request.profile_id = "profile:read-only".into();
        assert_eq!(
            controller.start(request).await.unwrap_err().code(),
            "CAPABILITY_DENIED"
        );
    }

    #[tokio::test]
    async fn wait_returns_the_projection_after_completion() {
        let controller = controller_with(
            Arc::new(FakeAdapter::new()),
            fake_runtime("fake", false),
            fake_profile("runtime:fake"),
        );
        let active = controller
            .start(fake_request(
                "/tmp/project",
                AccessMode::ReadOnly,
                "codex:wait",
            ))
            .await
            .unwrap();
        let projection = controller.wait_for_worker(&active.worker_id).await.unwrap();
        assert_eq!(projection.run.status, RunStatus::AwaitingHost);
        assert_eq!(projection.result.unwrap()["summary"], "Fake task completed");
    }
}
