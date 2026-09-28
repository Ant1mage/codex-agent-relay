//! Host sessions on disk.

use std::sync::Arc;

use relay_core::{
    apply_upsert, now, HostSession, HostSessionStatus, HostSessionStore, HostSessionUpsert, Result,
};
use rusqlite::OptionalExtension;

use crate::Database;

pub struct SqliteHostSessionStore {
    database: Arc<Database>,
}

impl SqliteHostSessionStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn count(&self) -> Result<u64> {
        self.database
            .with(|connection| {
                connection.query_row("SELECT COUNT(*) FROM host_sessions", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .map(|value| value as u64)
    }
}

impl HostSessionStore for SqliteHostSessionStore {
    fn upsert_codex(&self, input: HostSessionUpsert) -> Result<HostSession> {
        let id = format!("codex:{}", input.native_session_id);
        let existing = self.get(&id)?;
        let session = apply_upsert(existing.as_ref(), &input, &now());
        let data_json = serde_json::to_string(&session)?;
        self.database.with(|connection| {
            connection.execute(
                "INSERT INTO host_sessions (id, native_session_id, display_name, data_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                   native_session_id = excluded.native_session_id,
                   display_name = excluded.display_name,
                   data_json = excluded.data_json,
                   updated_at = excluded.updated_at",
                rusqlite::params![
                    session.id,
                    session.native_session_id,
                    session.display_name,
                    data_json,
                    session.updated_at,
                ],
            )?;
            Ok(())
        })?;
        Ok(session)
    }

    fn get(&self, id: &str) -> Result<Option<HostSession>> {
        let row: Option<String> = self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT data_json FROM host_sessions WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
        })?;
        match row {
            Some(json) => Ok(Some(serde_json::from_str::<HostSession>(&json)?)),
            None => Ok(None),
        }
    }

    fn list(&self) -> Result<Vec<HostSession>> {
        let rows: Vec<String> = self.database.with(|connection| {
            let mut statement = connection
                .prepare("SELECT data_json FROM host_sessions ORDER BY updated_at DESC")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect()
        })?;
        let mut sessions = Vec::with_capacity(rows.len());
        for json in rows {
            sessions.push(serde_json::from_str::<HostSession>(json.as_str())?);
        }
        Ok(sessions)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        let full_id = if id.starts_with("codex:") {
            id.to_string()
        } else {
            format!("codex:{id}")
        };
        let native_id = id.strip_prefix("codex:").unwrap_or(id).to_string();

        self.database.transaction(|tx| {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT run_id FROM relay_events
                 WHERE type = 'run/created'
                   AND (json_extract(data_json, '$.run.host_session_id') = ?1
                        OR json_extract(data_json, '$.run.host_session_id') = ?2)",
            )?;
            let run_ids: Vec<String> = stmt
                .query_map(rusqlite::params![full_id, native_id], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            let mut events_deleted = 0;
            for run_id in &run_ids {
                events_deleted += tx.execute(
                    "DELETE FROM relay_events WHERE run_id = ?1",
                    rusqlite::params![run_id],
                )?;
            }

            let session_deleted = tx.execute(
                "DELETE FROM host_sessions WHERE id = ?1 OR native_session_id = ?2",
                rusqlite::params![full_id, native_id],
            )?;

            Ok(session_deleted > 0 || events_deleted > 0)
        })
    }
}

/// Ends a session by native id, used by the `--session-end-hook` entry point.
pub fn end_session(
    store: &dyn HostSessionStore,
    native_session_id: &str,
) -> Result<Option<HostSession>> {
    let id = format!("codex:{native_session_id}");
    let Some(existing) = store.get(&id)? else {
        return Ok(None);
    };
    let session = store.upsert_codex(HostSessionUpsert {
        native_session_id: native_session_id.to_string(),
        display_name: existing.display_name,
        cwd: existing.cwd,
        model: existing.model,
        status: HostSessionStatus::Ended,
    })?;
    Ok(Some(session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_core::{EventStore, RelayEvent, RelayEventType};

    #[test]
    fn delete_cascades_session_and_events() {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(Database::open(directory.path().join("relay.sqlite")).unwrap());
        let session_store = SqliteHostSessionStore::new(Arc::clone(&database));
        let event_store = crate::SqliteEventStore::new(Arc::clone(&database));

        let session = session_store
            .upsert_codex(HostSessionUpsert {
                native_session_id: "thread-100".into(),
                display_name: "Test Session".into(),
                cwd: "/tmp/project".into(),
                model: None,
                status: HostSessionStatus::Active,
            })
            .unwrap();

        assert_eq!(session_store.count().unwrap(), 1);

        // Append run events for this session
        let run_created_event = RelayEvent {
            id: "evt-1".into(),
            run_id: "run-100".into(),
            step_id: None,
            worker_session_id: None,
            seq: 1,
            timestamp: now(),
            event_type: RelayEventType::RunCreated,
            data: serde_json::json!({
                "run": {
                    "id": "run-100",
                    "host_session_id": session.id,
                }
            }),
            native_event: None,
        };
        event_store.append(run_created_event).unwrap();

        let step_event = RelayEvent {
            id: "evt-2".into(),
            run_id: "run-100".into(),
            step_id: Some("step-100".into()),
            worker_session_id: None,
            seq: 2,
            timestamp: now(),
            event_type: RelayEventType::StepCreated,
            data: serde_json::json!({}),
            native_event: None,
        };
        event_store.append(step_event).unwrap();

        assert_eq!(event_store.list("run-100").unwrap().len(), 2);

        // Delete using session id
        let deleted = session_store.delete(&session.id).unwrap();
        assert!(deleted);

        // Session gone
        assert_eq!(session_store.count().unwrap(), 0);
        assert!(session_store.get(&session.id).unwrap().is_none());

        // Events for that run gone
        assert!(event_store.list("run-100").unwrap().is_empty());

        // Second delete returns false
        assert!(!session_store.delete(&session.id).unwrap());
    }
}
