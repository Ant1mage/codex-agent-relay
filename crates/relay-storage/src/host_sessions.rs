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
            .with(|connection| connection.query_row("SELECT COUNT(*) FROM host_sessions", [], |row| row.get::<_, i64>(0)))
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
        let row: Option<String> = self
            .database
            .with(|connection| {
                connection
                    .query_row("SELECT data_json FROM host_sessions WHERE id = ?1", [id], |row| row.get(0))
                    .optional()
            })?;
        match row {
            Some(json) => Ok(Some(serde_json::from_str::<HostSession>(&json)?)),
            None => Ok(None),
        }
    }

    fn list(&self) -> Result<Vec<HostSession>> {
        let rows: Vec<String> = self.database.with(|connection| {
            let mut statement =
                connection.prepare("SELECT data_json FROM host_sessions ORDER BY updated_at DESC")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect()
        })?;
        let mut sessions = Vec::with_capacity(rows.len());
        for json in rows {
            sessions.push(serde_json::from_str::<HostSession>(json.as_str())?);
        }
        Ok(sessions)
    }
}

/// Ends a session by native id, used by the `--session-end-hook` entry point.
pub fn end_session(store: &dyn HostSessionStore, native_session_id: &str) -> Result<Option<HostSession>> {
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
