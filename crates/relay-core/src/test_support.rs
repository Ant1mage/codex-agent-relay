//! Test fixtures shared by the core tests.

use serde_json::json;

use crate::domain::{AccessMode, Isolation, RunStatus, StepStatus, WorkerStatus};
use crate::event::{RelayEvent, RelayEventType};

pub fn event(
    seq: u64,
    event_type: RelayEventType,
    data: Option<serde_json::Value>,
    step_id: Option<&str>,
    worker_session_id: Option<&str>,
) -> RelayEvent {
    RelayEvent {
        id: format!("event-{seq}"),
        run_id: "run:1".to_string(),
        step_id: step_id.map(str::to_string),
        worker_session_id: worker_session_id.map(str::to_string),
        seq,
        timestamp: format!("2026-01-01T00:00:{seq:02}.000Z"),
        event_type,
        data: data.unwrap_or_else(|| json!({})),
        native_event: None,
    }
}

pub fn run_payload(run_id: &str) -> serde_json::Value {
    json!({
        "id": run_id,
        "hostSessionId": "codex:test-session",
        "profileId": "profile:fake-code",
        "task": "Prove the vertical slice",
        "cwd": "/tmp/project",
        "accessMode": AccessMode::Write,
        "isolation": Isolation::Shared,
        "status": RunStatus::Queued,
        "createdAt": "2026-01-01T00:00:00.000Z",
        "updatedAt": "2026-01-01T00:00:00.000Z",
    })
}

pub fn step_payload(step_id: &str, run_id: &str) -> serde_json::Value {
    json!({
        "id": step_id,
        "runId": run_id,
        "profileId": "profile:fake-code",
        "task": "Prove the vertical slice",
        "accessMode": AccessMode::Write,
        "isolation": Isolation::Shared,
        "status": StepStatus::Queued,
        "iteration": 1,
        "createdAt": "2026-01-01T00:00:00.000Z",
        "updatedAt": "2026-01-01T00:00:00.000Z",
    })
}

pub fn worker_payload(worker_id: &str, run_id: &str, step_id: &str) -> serde_json::Value {
    json!({
        "id": worker_id,
        "runId": run_id,
        "stepId": step_id,
        "iteration": 1,
        "runtimeId": "runtime:fake",
        "status": WorkerStatus::Starting,
        "startedAt": "2026-01-01T00:00:00.000Z",
    })
}

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::adapter::{AdapterEvent, AgentAdapter, DetectionResult, WorkerHandle};
use crate::domain::{
    AdapterCapabilities, AgentProfile, CapabilitySet, RelayError, Result, ResumeInput, RunRequest,
    Runtime, RuntimeHealth, StartInput,
};
use crate::run::RunController;
use crate::store::MemoryEventStore;

/// The vertical-slice adapter: three events, then completion.
pub struct FakeAdapter {
    pub last_start_input: Mutex<Option<StartInput>>,
    pub resume_supported: bool,
}

impl FakeAdapter {
    pub fn new() -> Self {
        Self {
            last_start_input: Mutex::new(None),
            resume_supported: false,
        }
    }

    pub fn resumable() -> Self {
        Self {
            last_start_input: Mutex::new(None),
            resume_supported: true,
        }
    }
}

impl Default for FakeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentAdapter for FakeAdapter {
    fn id(&self) -> &str {
        "fake"
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            non_interactive: true,
            structured_events: true,
            cwd: true,
            resume: self.resume_supported,
            send: false,
            cancel: true,
            child_sessions: false,
            model_selection: None,
        }
    }

    async fn detect(&self) -> DetectionResult {
        DetectionResult::default()
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        *self.last_start_input.lock().unwrap() = Some(input.clone());
        let (sender, receiver) = mpsc::channel(16);
        let task = input.task.clone();
        let session = format!("fake:{}", input.worker_session_id);
        tokio::spawn(async move {
            let _ = sender
                .send(AdapterEvent::new(
                    crate::event::RelayEventType::WorkerMessage,
                    json!({ "text": format!("Working on: {task}") }),
                ))
                .await;
            let _ = sender
                .send(AdapterEvent::new(
                    crate::event::RelayEventType::ToolRead,
                    json!({ "path": "README.md" }),
                ))
                .await;
            let _ = sender
                .send(AdapterEvent::new(
                    crate::event::RelayEventType::WorkerCompleted,
                    json!({ "summary": "Fake task completed" }),
                ))
                .await;
        });
        Ok(WorkerHandle {
            native_session_id: Some(session),
            process_id: None,
            events: receiver,
        })
    }

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        if !self.resume_supported {
            return Err(RelayError::new(
                "OPERATION_UNSUPPORTED",
                "fake adapter does not resume",
            ));
        }
        self.start(StartInput {
            run_id: input.run_id,
            worker_session_id: input.worker_session_id,
            task: input.task,
            cwd: input.cwd,
            access_mode: input.access_mode,
            executable_path: input.executable_path,
            model: None,
            reasoning: None,
            instructions: input.instructions,
        })
        .await
    }

    async fn cancel(&self, _native_session_id: &str) -> Result<()> {
        Ok(())
    }
}

/// An adapter that only finishes when it is cancelled.
pub struct ControlledAdapter {
    releases: Mutex<Vec<tokio::sync::oneshot::Sender<()>>>,
}

impl ControlledAdapter {
    pub fn new() -> Self {
        Self {
            releases: Mutex::new(Vec::new()),
        }
    }
}

impl Default for ControlledAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentAdapter for ControlledAdapter {
    fn id(&self) -> &str {
        "controlled"
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            non_interactive: true,
            structured_events: true,
            cwd: true,
            resume: false,
            send: false,
            cancel: true,
            child_sessions: false,
            model_selection: None,
        }
    }

    async fn detect(&self) -> DetectionResult {
        DetectionResult::default()
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        let (sender, receiver) = mpsc::channel(16);
        let (release, released) = tokio::sync::oneshot::channel();
        self.releases.lock().unwrap().push(release);
        let task = input.task.clone();
        tokio::spawn(async move {
            let _ = sender
                .send(AdapterEvent::new(
                    crate::event::RelayEventType::WorkerMessage,
                    json!({ "task": task }),
                ))
                .await;
            let _ = released.await;
        });
        Ok(WorkerHandle {
            native_session_id: Some(format!("controlled:{}", input.worker_session_id)),
            process_id: None,
            events: receiver,
        })
    }

    async fn cancel(&self, _native_session_id: &str) -> Result<()> {
        if let Some(release) = self.releases.lock().unwrap().pop() {
            let _ = release.send(());
        }
        Ok(())
    }
}

pub fn fake_runtime(adapter_id: &str, resume: bool) -> Runtime {
    Runtime {
        id: format!("runtime:{adapter_id}"),
        adapter_id: adapter_id.to_string(),
        executable_path: "/usr/bin/true".to_string(),
        version: Some("1.0.0".to_string()),
        health: RuntimeHealth::Available,
        capabilities: AdapterCapabilities {
            resume,
            ..AdapterCapabilities::default()
        },
    }
}

pub fn fake_profile(runtime_id: &str) -> AgentProfile {
    AgentProfile {
        id: "profile:fake-code".to_string(),
        name: "Fake Code".to_string(),
        runtime_id: runtime_id.to_string(),
        description: "Exercises the provider-independent run path.".to_string(),
        instructions: None,
        model: None,
        reasoning: None,
        capabilities: CapabilitySet {
            read_workspace: true,
            write_workspace: true,
            execute_commands: true,
            network_access: false,
        },
        enabled: true,
    }
}

pub fn fake_request(cwd: &str, access_mode: AccessMode, host_session_id: &str) -> RunRequest {
    RunRequest {
        host_session_id: host_session_id.to_string(),
        profile_id: "profile:fake-code".to_string(),
        task: "Prove the vertical slice".to_string(),
        cwd: cwd.to_string(),
        access_mode,
        isolation: Isolation::default(),
    }
}

/// Controller wired with one adapter, runtime and profile.
pub fn controller_with(
    adapter: Arc<dyn AgentAdapter>,
    runtime: Runtime,
    profile: AgentProfile,
) -> Arc<RunController> {
    let controller = Arc::new(RunController::new(Arc::new(MemoryEventStore::new())));
    controller.adapters.register(adapter).unwrap();
    controller.runtimes.register(runtime).unwrap();
    controller.profiles.register(profile).unwrap();
    controller
}
