//! Host sessions: the Codex thread a delegation belongs to.
//!
//! The domain rule lives here — the id is derived from the host, the display name
//! always comes from Codex, and `startedAt`/`endedAt` survive repeated upserts —
//! so every store applies the same rule instead of re-implementing it.

use std::collections::BTreeMap;
use std::sync::RwLock;

use crate::domain::{now, HostSession, HostSessionStatus, HostSessionUpsert, RelayError, Result};

/// Applies one host-reported upsert to the stored session, if any.
pub fn apply_upsert(
    existing: Option<&HostSession>,
    input: &HostSessionUpsert,
    timestamp: &str,
) -> HostSession {
    let id = format!("codex:{}", input.native_session_id);
    let model = input
        .model
        .clone()
        .or_else(|| existing.and_then(|session| session.model.clone()));
    let status = input.status;
    HostSession {
        id,
        host: "codex".to_string(),
        native_session_id: input.native_session_id.clone(),
        display_name: input.display_name.clone(),
        name_source: "codex".to_string(),
        cwd: input.cwd.clone(),
        model,
        status,
        started_at: existing.map(|session| session.started_at.clone()).unwrap_or_else(|| timestamp.to_string()),
        updated_at: timestamp.to_string(),
        ended_at: if status == HostSessionStatus::Ended {
            existing
                .and_then(|session| session.ended_at.clone())
                .or_else(|| Some(timestamp.to_string()))
        } else {
            None
        },
    }
}

/// Store contract used by the MCP service and the daemon projection.
pub trait HostSessionStore: Send + Sync {
    fn upsert_codex(&self, input: HostSessionUpsert) -> Result<HostSession>;
    fn get(&self, id: &str) -> Result<Option<HostSession>>;
    fn list(&self) -> Result<Vec<HostSession>>;

    fn rename_from_codex(&self, native_session_id: &str, display_name: &str) -> Result<HostSession> {
        let id = format!("codex:{native_session_id}");
        let existing = self
            .get(&id)?
            .ok_or_else(|| RelayError::new("HOST_SESSION_NOT_FOUND", format!("Unknown host session {id}")))?;
        self.upsert_codex(HostSessionUpsert {
            native_session_id: native_session_id.to_string(),
            display_name: display_name.to_string(),
            cwd: existing.cwd,
            model: existing.model,
            status: existing.status,
        })
    }
}

/// In-memory session store for tests and memory-only runs.
#[derive(Default)]
pub struct HostSessionRegistry {
    sessions: RwLock<BTreeMap<String, HostSession>>,
}

impl HostSessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HostSessionStore for HostSessionRegistry {
    fn upsert_codex(&self, input: HostSessionUpsert) -> Result<HostSession> {
        let id = format!("codex:{}", input.native_session_id);
        let mut sessions = self.sessions.write().unwrap();
        let session = apply_upsert(sessions.get(&id), &input, &now());
        sessions.insert(id, session.clone());
        Ok(session)
    }

    fn get(&self, id: &str) -> Result<Option<HostSession>> {
        Ok(self.sessions.read().unwrap().get(id).cloned())
    }

    fn list(&self) -> Result<Vec<HostSession>> {
        let mut sessions: Vec<HostSession> = self.sessions.read().unwrap().values().cloned().collect();
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upsert(native_id: &str, name: &str, status: HostSessionStatus) -> HostSessionUpsert {
        HostSessionUpsert {
            native_session_id: native_id.to_string(),
            display_name: name.to_string(),
            cwd: "/tmp/project".to_string(),
            model: None,
            status,
        }
    }

    #[test]
    fn session_ids_are_derived_from_the_host() {
        let store = HostSessionRegistry::new();
        let session = store.upsert_codex(upsert("thread-1", "First", HostSessionStatus::Active)).unwrap();
        assert_eq!(session.id, "codex:thread-1");
        assert_eq!(session.name_source, "codex");
        assert!(session.ended_at.is_none());
    }

    #[test]
    fn reupsert_keeps_the_original_start_time_and_model() {
        let mut first = upsert("thread-2", "First", HostSessionStatus::Active);
        first.model = Some("gpt-5".into());
        let started = apply_upsert(None, &first, "2026-01-01T00:00:00.000Z");
        let mut second = upsert("thread-2", "Renamed", HostSessionStatus::Active);
        second.model = None;
        let updated = apply_upsert(Some(&started), &second, "2026-01-01T00:10:00.000Z");
        assert_eq!(updated.started_at, "2026-01-01T00:00:00.000Z");
        assert_eq!(updated.model.as_deref(), Some("gpt-5"));
        assert_eq!(updated.display_name, "Renamed");
    }

    #[test]
    fn ending_a_session_records_the_end_time() {
        let store = HostSessionRegistry::new();
        store.upsert_codex(upsert("thread-3", "Work", HostSessionStatus::Active)).unwrap();
        let ended = store.upsert_codex(upsert("thread-3", "Work", HostSessionStatus::Ended)).unwrap();
        assert_eq!(ended.status, HostSessionStatus::Ended);
        assert!(ended.ended_at.is_some());
        assert_eq!(store.list().unwrap().len(), 1);
    }
}
