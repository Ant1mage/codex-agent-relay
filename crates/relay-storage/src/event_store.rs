//! The append-only event log, on disk.

use std::sync::Arc;

use relay_core::{RelayEvent, RelayEventType, Result};
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
            let events: i64 = connection.query_row(
                "SELECT COALESCE(MAX(rowid), 0) FROM relay_events",
                [],
                |row| row.get(0),
            )?;
            let sessions: i64 =
                connection.query_row("SELECT COUNT(*) FROM host_sessions", [], |row| row.get(0))?;
            let updated: String = connection.query_row(
                "SELECT COALESCE(MAX(updated_at), '') FROM host_sessions",
                [],
                |row| row.get(0),
            )?;
            Ok(format!("{events}:{sessions}:{updated}"))
        })
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
