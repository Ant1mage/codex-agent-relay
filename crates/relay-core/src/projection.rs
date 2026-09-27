//! Pure projection: events in, current Run / Step / WorkerSession state out.
//!
//! `result` is the data carried by the latest terminal worker event, which is
//! what `wait_agent` hands back to Codex for review. Losing that link would break
//! the delegation contract, so it is asserted in the tests below.

use crate::domain::{
    now, AccessMode, Isolation, RelayError, Result, Run, RunStatus, Step, StepStatus,
    WorkerSession, WorkerStatus,
};
use crate::event::{RelayEvent, RelayEventType};

#[derive(Debug, Clone, PartialEq)]
pub struct RunProjection {
    pub run: Run,
    pub steps: Vec<Step>,
    pub workers: Vec<WorkerSession>,
    /// Data carried by the latest terminal worker event, for host review.
    pub result: Option<serde_json::Value>,
    pub last_event: Option<RelayEvent>,
}

impl RunProjection {
    pub fn step(&self, step_id: &str) -> Option<&Step> {
        self.steps.iter().find(|step| step.id == step_id)
    }

    pub fn worker(&self, worker_id: &str) -> Option<&WorkerSession> {
        self.workers.iter().find(|worker| worker.id == worker_id)
    }
}

fn as_object(value: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    value.as_object()
}

/// Steps and workers are kept in the order they were first seen: the log is the
/// order, and a UI that shows "the last worker" depends on it.
fn insert_step(
    steps: &mut Vec<Step>,
    index: &mut std::collections::HashMap<String, usize>,
    step: Step,
) {
    match index.get(&step.id).copied() {
        Some(position) => steps[position] = step,
        None => {
            index.insert(step.id.clone(), steps.len());
            steps.push(step);
        }
    }
}

fn insert_worker(
    workers: &mut Vec<WorkerSession>,
    index: &mut std::collections::HashMap<String, usize>,
    worker: WorkerSession,
) {
    match index.get(&worker.id).copied() {
        Some(position) => workers[position] = worker,
        None => {
            index.insert(worker.id.clone(), workers.len());
            workers.push(worker);
        }
    }
}

fn step_mut<'a>(
    steps: &'a mut [Step],
    index: &std::collections::HashMap<String, usize>,
    id: &str,
) -> Option<&'a mut Step> {
    index
        .get(id)
        .copied()
        .and_then(move |position| steps.get_mut(position))
}

fn worker_mut<'a>(
    workers: &'a mut [WorkerSession],
    index: &std::collections::HashMap<String, usize>,
    id: &str,
) -> Option<&'a mut WorkerSession> {
    index
        .get(id)
        .copied()
        .and_then(move |position| workers.get_mut(position))
}

pub fn project_run(events: &[RelayEvent]) -> Result<RunProjection> {
    let Some(first) = events.first() else {
        return Err(RelayError::new(
            "RUN_NOT_FOUND",
            "Run projection requires run/created",
        ));
    };
    if first.event_type != RelayEventType::RunCreated {
        return Err(RelayError::new(
            "INVALID_STATE",
            "Run projection requires run/created",
        ));
    }
    let raw_run = as_object(&first.data)
        .and_then(|data| data.get("run"))
        .cloned()
        .ok_or_else(|| {
            RelayError::new("INVALID_STATE", "run/created is missing its run payload")
        })?;
    let mut run: Run = serde_json::from_value(raw_run)?;

    let mut steps: Vec<Step> = Vec::new();
    let mut step_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut workers: Vec<WorkerSession> = Vec::new();
    let mut worker_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let uses_step_lifecycle = events
        .iter()
        .any(|event| event.event_type == RelayEventType::StepCreated);

    for event in events.iter().skip(1) {
        run.updated_at = event.timestamp.clone();

        match event.event_type {
            RelayEventType::StepCreated | RelayEventType::StepIterationStarted => {
                if let Some(step) = as_object(&event.data).and_then(|data| data.get("step")) {
                    let step: Step = serde_json::from_value(step.clone())?;
                    insert_step(&mut steps, &mut step_index, step);
                }
                if event.event_type == RelayEventType::StepIterationStarted {
                    run.status = RunStatus::Starting;
                }
                continue;
            }
            RelayEventType::WorkerStarted => {
                if let Some(raw_worker) = as_object(&event.data).and_then(|data| data.get("worker"))
                {
                    let mut worker: WorkerSession = serde_json::from_value(raw_worker.clone())?;
                    let step_id = event
                        .step_id
                        .clone()
                        .filter(|id| step_index.contains_key(id))
                        .or_else(|| steps.first().map(|step| step.id.clone()));
                    if let Some(step_id) = step_id {
                        worker.step_id = step_id.clone();
                        if let Some(position) = step_index.get(&step_id).copied() {
                            worker.iteration = steps[position].iteration;
                            steps[position].status = StepStatus::Running;
                            steps[position].updated_at = event.timestamp.clone();
                        }
                    }
                    insert_worker(&mut workers, &mut worker_index, worker);
                    run.status = RunStatus::Running;
                }
                continue;
            }
            _ => {}
        }

        let worker_step_id = event
            .worker_session_id
            .as_ref()
            .and_then(|id| worker_index.get(id).copied())
            .map(|position| workers[position].step_id.clone());
        let step_id = event.step_id.clone().or(worker_step_id);

        let worker_id = event.worker_session_id.clone();
        let step_target = step_id.clone();
        match event.event_type {
            RelayEventType::WorkerCompleted => {
                run.status = if uses_step_lifecycle {
                    RunStatus::AwaitingHost
                } else {
                    RunStatus::Completed
                };
                if let Some(worker) = worker_id
                    .as_ref()
                    .and_then(|id| worker_mut(&mut workers, &worker_index, id))
                {
                    worker.status = WorkerStatus::Completed;
                    worker.ended_at = Some(event.timestamp.clone());
                }
                if let Some(step) = step_target
                    .as_ref()
                    .and_then(|id| step_mut(&mut steps, &step_index, id))
                {
                    step.status = if uses_step_lifecycle {
                        StepStatus::AwaitingHost
                    } else {
                        StepStatus::Completed
                    };
                    step.updated_at = event.timestamp.clone();
                }
            }
            RelayEventType::WorkerFailed => {
                run.status = RunStatus::Failed;
                if let Some(worker) = worker_id
                    .as_ref()
                    .and_then(|id| worker_mut(&mut workers, &worker_index, id))
                {
                    worker.status = WorkerStatus::Failed;
                    worker.ended_at = Some(event.timestamp.clone());
                }
                if let Some(step) = step_target
                    .as_ref()
                    .and_then(|id| step_mut(&mut steps, &step_index, id))
                {
                    step.status = StepStatus::Failed;
                    step.updated_at = event.timestamp.clone();
                }
            }
            RelayEventType::WorkerCancelled => {
                run.status = RunStatus::Cancelled;
                if let Some(worker) = worker_id
                    .as_ref()
                    .and_then(|id| worker_mut(&mut workers, &worker_index, id))
                {
                    worker.status = WorkerStatus::Cancelled;
                    worker.ended_at = Some(event.timestamp.clone());
                }
                if let Some(step) = step_target
                    .as_ref()
                    .and_then(|id| step_mut(&mut steps, &step_index, id))
                {
                    step.status = StepStatus::Cancelled;
                    step.updated_at = event.timestamp.clone();
                }
            }
            RelayEventType::WorkerInterrupted => {
                run.status = RunStatus::Interrupted;
                if let Some(worker) = worker_id
                    .as_ref()
                    .and_then(|id| worker_mut(&mut workers, &worker_index, id))
                {
                    worker.status = WorkerStatus::Interrupted;
                }
                if let Some(step) = step_target
                    .as_ref()
                    .and_then(|id| step_mut(&mut steps, &step_index, id))
                {
                    step.status = StepStatus::Interrupted;
                }
            }
            RelayEventType::WorkerOrphaned => {
                run.status = RunStatus::Orphaned;
                if let Some(worker) = worker_id
                    .as_ref()
                    .and_then(|id| worker_mut(&mut workers, &worker_index, id))
                {
                    worker.status = WorkerStatus::Orphaned;
                }
                if let Some(step) = step_target
                    .as_ref()
                    .and_then(|id| step_mut(&mut steps, &step_index, id))
                {
                    step.status = StepStatus::Orphaned;
                }
            }
            RelayEventType::RunAwaitingHost => {
                run.status = RunStatus::AwaitingHost;
            }
            RelayEventType::RunAccepted => {
                run.status = RunStatus::Completed;
                for step in steps.iter_mut() {
                    if step.status == StepStatus::AwaitingHost {
                        step.status = StepStatus::Completed;
                        step.updated_at = event.timestamp.clone();
                    }
                }
            }
            _ => {}
        }
    }

    if steps.is_empty() {
        let legacy = Step {
            id: format!("legacy-step:{}", run.id),
            run_id: run.id.clone(),
            profile_id: run.profile_id.clone(),
            task: run.task.clone(),
            access_mode: run.access_mode,
            isolation: run.isolation,
            status: StepStatus::from(run.status),
            iteration: 1,
            created_at: run.created_at.clone(),
            updated_at: run.updated_at.clone(),
        };
        insert_step(&mut steps, &mut step_index, legacy);
    }

    let terminal = events
        .iter()
        .rev()
        .find(|event| event.event_type.terminal_worker_status().is_some());

    Ok(RunProjection {
        run,
        steps,
        workers,
        result: terminal.map(|event| event.data.clone()),
        last_event: events.last().cloned(),
    })
}

/// A run with no events at all, used where the caller needs a placeholder.
pub fn empty_run(id: &str, host_session_id: &str, cwd: &str) -> Run {
    let stamp = now();
    Run {
        id: id.to_string(),
        host_session_id: host_session_id.to_string(),
        profile_id: String::new(),
        task: String::new(),
        cwd: cwd.to_string(),
        access_mode: AccessMode::ReadOnly,
        isolation: Isolation::Shared,
        status: RunStatus::Queued,
        created_at: stamp.clone(),
        updated_at: stamp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{event, run_payload, step_payload, worker_payload};

    fn terminal_sequence() -> Vec<RelayEvent> {
        let run = serde_json::json!({ "run": run_payload("run:1") });
        let step = serde_json::json!({ "step": step_payload("step:1", "run:1") });
        let worker = serde_json::json!({ "worker": worker_payload("worker:1", "run:1", "step:1") });
        vec![
            event(1, RelayEventType::RunCreated, Some(run), None, None),
            event(
                2,
                RelayEventType::StepCreated,
                Some(step),
                Some("step:1"),
                None,
            ),
            event(
                3,
                RelayEventType::WorkerStarted,
                Some(worker),
                Some("step:1"),
                Some("worker:1"),
            ),
            event(
                4,
                RelayEventType::WorkerMessage,
                Some(serde_json::json!({ "text": "working" })),
                Some("step:1"),
                Some("worker:1"),
            ),
            event(
                5,
                RelayEventType::WorkerCompleted,
                Some(serde_json::json!({ "summary": "done" })),
                Some("step:1"),
                Some("worker:1"),
            ),
            event(
                6,
                RelayEventType::RunAwaitingHost,
                Some(serde_json::json!({ "stepId": "step:1" })),
                None,
                None,
            ),
        ]
    }

    #[test]
    fn worker_completion_becomes_the_run_result() {
        let projection = project_run(&terminal_sequence()).unwrap();
        assert_eq!(projection.run.status, RunStatus::AwaitingHost);
        assert_eq!(projection.steps[0].status, StepStatus::AwaitingHost);
        assert_eq!(projection.workers[0].status, WorkerStatus::Completed);
        assert_eq!(projection.result.unwrap()["summary"], "done");
        assert_eq!(
            projection.last_event.unwrap().event_type,
            RelayEventType::RunAwaitingHost
        );
    }

    #[test]
    fn acceptance_completes_the_run_and_its_steps() {
        let mut events = terminal_sequence();
        events.push(event(
            7,
            RelayEventType::RunAccepted,
            Some(serde_json::json!({ "acceptedBy": "host" })),
            None,
            None,
        ));
        let projection = project_run(&events).unwrap();
        assert_eq!(projection.run.status, RunStatus::Completed);
        assert_eq!(projection.steps[0].status, StepStatus::Completed);
        assert_eq!(projection.result.unwrap()["summary"], "done");
    }

    #[test]
    fn a_second_iteration_reuses_the_step_and_adds_a_worker() {
        let mut events = terminal_sequence();
        let mut step = step_payload("step:1", "run:1");
        step["iteration"] = serde_json::json!(2);
        step["status"] = serde_json::json!("starting");
        events.push(event(
            7,
            RelayEventType::StepIterationStarted,
            Some(serde_json::json!({ "step": step })),
            None,
            None,
        ));
        let worker = worker_payload("worker:2", "run:1", "step:1");
        events.push(event(
            8,
            RelayEventType::WorkerStarted,
            Some(serde_json::json!({ "worker": worker })),
            Some("step:1"),
            Some("worker:2"),
        ));
        events.push(event(
            9,
            RelayEventType::WorkerCompleted,
            Some(serde_json::json!({ "summary": "second pass" })),
            Some("step:1"),
            Some("worker:2"),
        ));

        let projection = project_run(&events).unwrap();
        assert_eq!(projection.steps.len(), 1);
        assert_eq!(projection.steps[0].iteration, 2);
        assert_eq!(projection.workers.len(), 2);
        assert_eq!(projection.result.unwrap()["summary"], "second pass");
    }

    #[test]
    fn a_run_without_a_step_created_event_gets_a_legacy_step() {
        let events = vec![
            event(
                1,
                RelayEventType::RunCreated,
                Some(serde_json::json!({ "run": run_payload("run:legacy") })),
                None,
                None,
            ),
            event(
                2,
                RelayEventType::WorkerCompleted,
                Some(serde_json::json!({ "summary": "old style" })),
                None,
                None,
            ),
        ];
        let projection = project_run(&events).unwrap();
        assert_eq!(projection.steps.len(), 1);
        assert_eq!(projection.steps[0].id, "legacy-step:run:legacy");
        assert_eq!(projection.run.status, RunStatus::Completed);
    }

    #[test]
    fn projection_requires_run_created_first() {
        let events = vec![event(
            1,
            RelayEventType::WorkerMessage,
            Some(serde_json::json!({})),
            None,
            None,
        )];
        assert!(project_run(&events).is_err());
        assert!(project_run(&[]).is_err());
    }
}
