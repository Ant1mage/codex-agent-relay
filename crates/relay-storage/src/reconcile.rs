//! Startup reconciliation.
//!
//! The daemon owns every worker process, so the only way a run can be left
//! "in progress" is a daemon that stopped while one of its workers was still
//! starting or running. Nothing else can finish those runs, so the daemon closes
//! them on the way up:
//!
//! * the worker process is gone → `worker/orphaned`
//! * the worker process outlived the daemon → `worker/interrupted`, because its
//!   output pipe died with the daemon and nothing can read its result any more
//! * the run never reached `worker/started` → `worker/interrupted`
//!
//! History is never rewritten: the fix is an appended event, exactly like every
//! other state change in Relay.

use relay_core::{now, EventStore, RelayError, RelayEvent, RelayEventType, Result, WorkerSession};
use uuid::Uuid;

use crate::event_store::{SqliteEventStore, StaleRun};

const PROCESS_GONE: &str =
    "the worker process is gone: the Relay daemon stopped before it finished";
const DAEMON_RESTARTED: &str =
    "the Relay daemon restarted while this worker was running and can no longer read its output";

#[derive(Debug, Default)]
pub struct Reconciliation {
    /// Runs that were converged to a terminal state.
    pub resolved: Vec<String>,
    /// What could not be converged, and why.
    pub diagnostics: Vec<String>,
}

impl Reconciliation {
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty() && self.diagnostics.is_empty()
    }
}

/// Converges every run a previous daemon left behind. Called once, at startup.
pub fn reconcile_stale_runs(events: &SqliteEventStore) -> Result<Reconciliation> {
    let mut reconciliation = Reconciliation::default();
    for stale in events.stale_runs()? {
        reconcile_one(events, &stale, &mut reconciliation);
    }
    Ok(reconciliation)
}

fn reconcile_one(events: &SqliteEventStore, stale: &StaleRun, reconciliation: &mut Reconciliation) {
    let active: Vec<&WorkerSession> = stale
        .workers
        .iter()
        .filter(|worker| worker.status.is_active())
        .collect();

    if active.is_empty() {
        let data = serde_json::json!({
            "reason": "the Relay daemon stopped before this run reached its worker",
            "reconciled": true,
        });
        record(
            events,
            stale,
            None,
            RelayEventType::WorkerInterrupted,
            data,
            reconciliation,
        );
        return;
    }

    for worker in active {
        let alive = worker.process_id.map(process_alive).unwrap_or(false);
        if alive {
            reconciliation.diagnostics.push(format!(
                "worker {} (pid {:?}) outlived the Relay daemon; it is reported as interrupted, not killed, because a reused pid cannot be told apart from the original process",
                worker.id, worker.process_id
            ));
        }
        let (event_type, reason) = if alive {
            (RelayEventType::WorkerInterrupted, DAEMON_RESTARTED)
        } else {
            (RelayEventType::WorkerOrphaned, PROCESS_GONE)
        };
        let data = serde_json::json!({
            "reason": reason,
            "reconciled": true,
            "processId": worker.process_id,
        });
        record(
            events,
            stale,
            Some(worker),
            event_type,
            data,
            reconciliation,
        );
    }
}

fn record(
    events: &SqliteEventStore,
    stale: &StaleRun,
    worker: Option<&WorkerSession>,
    event_type: RelayEventType,
    data: serde_json::Value,
    reconciliation: &mut Reconciliation,
) {
    match append_terminal(
        events,
        &stale.run_id,
        stale.step_id.as_deref(),
        worker.map(|worker| worker.id.as_str()),
        event_type,
        data,
    ) {
        Ok(()) => {
            if !reconciliation.resolved.contains(&stale.run_id) {
                reconciliation.resolved.push(stale.run_id.clone());
            }
        }
        Err(error) => reconciliation.diagnostics.push(format!(
            "could not reconcile run {}: {}",
            stale.run_id,
            error.message()
        )),
    }
}

/// Appends the missing terminal event, re-reading the sequence when another
/// writer got there first.
fn append_terminal(
    events: &SqliteEventStore,
    run_id: &str,
    step_id: Option<&str>,
    worker_session_id: Option<&str>,
    event_type: RelayEventType,
    data: serde_json::Value,
) -> Result<()> {
    for _ in 0..3 {
        let stored = events.list(run_id)?;
        let seq = stored.last().map(|event| event.seq).unwrap_or(0) + 1;
        let event = RelayEvent {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            step_id: step_id.map(str::to_string),
            worker_session_id: worker_session_id.map(str::to_string),
            seq,
            timestamp: now(),
            event_type,
            data: data.clone(),
            native_event: None,
        };
        match events.append(event) {
            Ok(()) => return Ok(()),
            Err(error) if error.code() == "EVENT_SEQUENCE_CONFLICT" => continue,
            Err(error) => return Err(error),
        }
    }
    Err(RelayError::new(
        "EVENT_SEQUENCE_CONFLICT",
        format!("run {run_id} kept changing while Relay reconciled it"),
    ))
}

fn process_alive(pid: u32) -> bool {
    // Signal 0 only performs the permission/existence check.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use relay_core::{RunStatus, WorkerStatus};
    use std::sync::Arc;

    fn database() -> (tempfile::TempDir, Arc<Database>) {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(Database::open(directory.path().join("relay.sqlite")).unwrap());
        (directory, database)
    }

    fn worker_payload(
        id: &str,
        run_id: &str,
        step_id: &str,
        pid: Option<u32>,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "runId": run_id,
            "stepId": step_id,
            "iteration": 1,
            "runtimeId": "runtime:test",
            "processId": pid,
            "status": "running",
            "startedAt": now(),
        })
    }

    fn append(
        store: &SqliteEventStore,
        run_id: &str,
        seq: u64,
        step_id: Option<&str>,
        worker_id: Option<&str>,
        event_type: RelayEventType,
        data: serde_json::Value,
    ) {
        store
            .append(RelayEvent {
                id: Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                step_id: step_id.map(str::to_string),
                worker_session_id: worker_id.map(str::to_string),
                seq,
                timestamp: now(),
                event_type,
                data,
                native_event: None,
            })
            .unwrap();
    }

    fn start_running_run(store: &SqliteEventStore, run_id: &str, pid: Option<u32>) {
        let stamp = now();
        append(
            store,
            run_id,
            1,
            None,
            None,
            RelayEventType::RunCreated,
            serde_json::json!({
                "run": {
                    "id": run_id,
                    "hostSessionId": "codex:test",
                    "profileId": "agent-1",
                    "task": "do it",
                    "cwd": "/tmp",
                    "accessMode": "read_only",
                    "isolation": "shared",
                    "status": "queued",
                    "createdAt": stamp,
                    "updatedAt": stamp,
                }
            }),
        );
        append(
            store,
            run_id,
            2,
            Some("step:1"),
            None,
            RelayEventType::StepCreated,
            serde_json::json!({
                "step": {
                    "id": "step:1",
                    "runId": run_id,
                    "profileId": "agent-1",
                    "task": "do it",
                    "accessMode": "read_only",
                    "isolation": "shared",
                    "status": "starting",
                    "iteration": 1,
                    "createdAt": stamp,
                    "updatedAt": stamp,
                }
            }),
        );
        append(
            store,
            run_id,
            3,
            Some("step:1"),
            Some("worker:1"),
            RelayEventType::WorkerStarted,
            serde_json::json!({ "worker": worker_payload("worker:1", run_id, "step:1", pid) }),
        );
        append(
            store,
            run_id,
            4,
            Some("step:1"),
            Some("worker:1"),
            RelayEventType::ToolRead,
            serde_json::json!({ "path": "a.rs" }),
        );
    }

    /// A worker whose process is gone must not leave the run running forever.
    #[test]
    fn a_dead_workers_run_becomes_orphaned() {
        let (_directory, database) = database();
        let store = SqliteEventStore::new(database);
        // A pid that cannot exist: the process check must say "gone".
        start_running_run(&store, "run:dead", Some(4_000_000));

        let reconciliation = reconcile_stale_runs(&store).unwrap();
        assert_eq!(reconciliation.resolved, vec!["run:dead".to_string()]);

        let projection = relay_core::project_run(&store.list("run:dead").unwrap()).unwrap();
        assert_eq!(projection.run.status, RunStatus::Orphaned);
        assert_eq!(projection.workers[0].status, WorkerStatus::Orphaned);
        assert_eq!(
            store.list("run:dead").unwrap().last().unwrap().event_type,
            RelayEventType::WorkerOrphaned
        );
    }

    /// A run that never reached worker/started is closed as interrupted.
    #[test]
    fn a_run_without_a_worker_becomes_interrupted() {
        let (_directory, database) = database();
        let store = SqliteEventStore::new(database);
        let stamp = now();
        append(
            &store,
            "run:early",
            1,
            None,
            None,
            RelayEventType::RunCreated,
            serde_json::json!({
                "run": {
                    "id": "run:early",
                    "hostSessionId": "codex:test",
                    "profileId": "agent-1",
                    "task": "do it",
                    "cwd": "/tmp",
                    "accessMode": "read_only",
                    "isolation": "shared",
                    "status": "queued",
                    "createdAt": stamp,
                    "updatedAt": stamp,
                }
            }),
        );

        let reconciliation = reconcile_stale_runs(&store).unwrap();
        assert_eq!(reconciliation.resolved, vec!["run:early".to_string()]);
        let projection = relay_core::project_run(&store.list("run:early").unwrap()).unwrap();
        assert_eq!(projection.run.status, RunStatus::Interrupted);
    }

    /// A finished run is left completely alone.
    #[test]
    fn a_completed_run_is_not_touched() {
        let (_directory, database) = database();
        let store = SqliteEventStore::new(database);
        start_running_run(&store, "run:done", Some(4_000_000));
        append(
            &store,
            "run:done",
            5,
            Some("step:1"),
            Some("worker:1"),
            RelayEventType::WorkerCompleted,
            serde_json::json!({ "summary": "done" }),
        );

        let reconciliation = reconcile_stale_runs(&store).unwrap();
        assert!(reconciliation.resolved.is_empty(), "{reconciliation:?}");
        assert_eq!(store.list("run:done").unwrap().len(), 5);
    }

    /// Reconciling twice must not append a second terminal event.
    #[test]
    fn reconciliation_is_idempotent() {
        let (_directory, database) = database();
        let store = SqliteEventStore::new(database);
        start_running_run(&store, "run:once", Some(4_000_000));
        assert_eq!(reconcile_stale_runs(&store).unwrap().resolved.len(), 1);
        let second = reconcile_stale_runs(&store).unwrap();
        assert!(second.resolved.is_empty(), "{second:?}");
        assert_eq!(store.list("run:once").unwrap().len(), 5);
    }
}
