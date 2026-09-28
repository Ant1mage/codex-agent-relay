//! The append-only event log, on disk.

use std::sync::Arc;

use relay_core::{EventStore, RelayEvent, RelayEventType, Result, RunStatus, WorkerSession};
use rusqlite::{OptionalExtension, Row};

use crate::Database;

pub struct SqliteEventStore {
    database: Arc<Database>,
}

impl SqliteEventStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn database(&self) -> Arc<Database> {
        Arc::clone(&self.database)
    }

    fn row_to_event(row: &Row<'_>) -> rusqlite::Result<RelayEvent> {
        let event_type: String = row.get("type")?;
        let data_json: String = row.get("data_json")?;
        let native_json: Option<String> = row.get("native_event_json")?;
        Ok(RelayEvent {
            id: row.get("id")?,
            run_id: row.get("run_id")?,
            step_id: row.get("step_id")?,
            worker_session_id: row.get("worker_session_id")?,
            seq: row.get::<_, i64>("seq")? as u64,
            timestamp: row.get("timestamp")?,
            event_type: RelayEventType::parse(&event_type).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    0,
                    event_type.clone(),
                    rusqlite::types::Type::Text,
                )
            })?,
            data: serde_json::from_str(&data_json).unwrap_or(serde_json::Value::Null),
            native_event: native_json.and_then(|value| serde_json::from_str(&value).ok()),
        })
    }

    pub fn list_run_ids(&self) -> Result<Vec<String>> {
        self.database.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT run_id FROM relay_events GROUP BY run_id ORDER BY MIN(timestamp) ASC",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect()
        })
    }

    pub fn count_runs(&self) -> Result<u64> {
        self.database
            .with(|connection| {
                connection.query_row(
                    "SELECT COUNT(DISTINCT run_id) FROM relay_events",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .map(|value| value as u64)
    }

    /// Cheap change stamp for the daemon's SSE tick.
    pub fn revision(&self) -> Result<String> {
        self.database.with(|connection| {
            let (event_count, max_rowid): (i64, i64) = connection.query_row(
                "SELECT COUNT(*), COALESCE(MAX(rowid), 0) FROM relay_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let sessions: i64 =
                connection.query_row("SELECT COUNT(*) FROM host_sessions", [], |row| row.get(0))?;
            let updated: String = connection.query_row(
                "SELECT COALESCE(MAX(updated_at), '') FROM host_sessions",
                [],
                |row| row.get(0),
            )?;
            Ok(format!("{event_count}:{max_rowid}:{sessions}:{updated}"))
        })
    }
}

/// A run the previous daemon left in progress.
///
/// The daemon owns every worker process, so a run whose projection is still
/// queued/starting/running when the daemon starts up can only be a leftover: its
/// owner is gone. Reconciliation appends the missing terminal event for it.
#[derive(Debug, Clone, PartialEq)]
pub struct StaleRun {
    pub run_id: String,
    pub step_id: Option<String>,
    /// Sequence number of the run's newest event.
    pub last_seq: u64,
    pub status: RunStatus,
    pub workers: Vec<WorkerSession>,
}

/// Events that mean "this run is not in progress any more".
const SETTLED_EVENTS: &str = "'worker/completed','worker/failed','worker/cancelled','worker/interrupted','worker/orphaned','run/awaiting_host','run/accepted'";

impl SqliteEventStore {
    /// Runs whose newest event is not a settled one.
    ///
    /// The SQL is a superset filter over the event index; the projection below
    /// decides. Called once at daemon startup, never on a request path.
    pub fn stale_runs(&self) -> Result<Vec<StaleRun>> {
        let candidates: Vec<String> = self.database.with(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT e.run_id
                 FROM relay_events e
                 JOIN (SELECT run_id, MAX(seq) AS max_seq FROM relay_events GROUP BY run_id) latest
                   ON latest.run_id = e.run_id AND latest.max_seq = e.seq
                 WHERE e.type NOT IN ({SETTLED_EVENTS})
                 ORDER BY e.run_id ASC"
            ))?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect()
        })?;

        let mut stale: Vec<StaleRun> = Vec::new();
        for run_id in candidates {
            let events = self.list(&run_id)?;
            let Ok(projection) = relay_core::project_run(&events) else {
                continue;
            };
            // A run that never reached worker/started still projects as queued,
            // which is exactly the leftover this exists to converge.
            if !matches!(
                projection.run.status,
                RunStatus::Queued | RunStatus::Starting | RunStatus::Running
            ) {
                continue;
            }
            let step_id = projection
                .workers
                .iter()
                .rev()
                .find(|worker| worker.status.is_active())
                .map(|worker| worker.step_id.clone())
                .or_else(|| {
                    projection
                        .last_event
                        .as_ref()
                        .and_then(|event| event.step_id.clone())
                })
                .or_else(|| projection.steps.first().map(|step| step.id.clone()));
            stale.push(StaleRun {
                run_id,
                step_id,
                last_seq: events.last().map(|event| event.seq).unwrap_or(0),
                status: projection.run.status,
                workers: projection.workers,
            });
        }
        Ok(stale)
    }
}

impl relay_core::EventStore for SqliteEventStore {
    fn append(&self, event: RelayEvent) -> Result<()> {
        let data_json = serde_json::to_string(&event.data)?;
        let native_json = match &event.native_event {
            Some(value) => Some(serde_json::to_string(value)?),
            None => None,
        };
        self.database.transaction(|transaction| {
            let last: Option<i64> = transaction
                .query_row("SELECT MAX(seq) FROM relay_events WHERE run_id = ?1", [&event.run_id], |row| {
                    row.get(0)
                })
                .optional()?
                .flatten();
            let expected = last.unwrap_or(0) + 1;
            if event.seq as i64 != expected {
                return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                    relay_core::RelayError::new(
                        "EVENT_SEQUENCE_CONFLICT",
                        format!(
                            "Expected sequence {expected} for run {}, received {}",
                            event.run_id, event.seq
                        ),
                    ),
                )));
            }
            transaction.execute(
                "INSERT INTO relay_events (
                   id, run_id, step_id, worker_session_id, seq, timestamp, type, data_json, native_event_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    event.id,
                    event.run_id,
                    event.step_id,
                    event.worker_session_id,
                    event.seq as i64,
                    event.timestamp,
                    event.event_type.as_str(),
                    data_json,
                    native_json,
                ],
            )?;
            Ok(())
        })
    }

    fn list(&self, run_id: &str) -> Result<Vec<RelayEvent>> {
        self.database.with(|connection| {
            let mut statement = connection
                .prepare("SELECT * FROM relay_events WHERE run_id = ?1 ORDER BY seq ASC")?;
            let rows = statement.query_map([run_id], SqliteEventStore::row_to_event)?;
            rows.collect()
        })
    }

    fn find_run_id_by_worker(&self, worker_session_id: &str) -> Result<Option<String>> {
        self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT run_id FROM relay_events WHERE worker_session_id = ?1 LIMIT 1",
                    [worker_session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
        })
    }

    fn list_run_ids(&self) -> Result<Vec<String>> {
        SqliteEventStore::list_run_ids(self)
    }
}
