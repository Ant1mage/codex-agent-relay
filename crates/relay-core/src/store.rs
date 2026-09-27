//! The event store contract Relay Core writes through.
//!
//! Storage implements this; Core never learns SQL.

use crate::domain::Result;
use crate::event::RelayEvent;

pub trait EventStore: Send + Sync {
    fn append(&self, event: RelayEvent) -> Result<()>;
    fn list(&self, run_id: &str) -> Result<Vec<RelayEvent>>;
    fn find_run_id_by_worker(&self, worker_session_id: &str) -> Result<Option<String>>;
    /// Every run id in the log, oldest first.
    fn list_run_ids(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

/// In-memory store: tests, and any run that must not touch the database.
#[derive(Default)]
pub struct MemoryEventStore {
    events: std::sync::Mutex<std::collections::BTreeMap<String, Vec<RelayEvent>>>,
}

impl MemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventStore for MemoryEventStore {
    fn append(&self, event: RelayEvent) -> Result<()> {
        let mut guard = self.events.lock().unwrap();
        let events = guard.entry(event.run_id.clone()).or_default();
        let expected = events.len() as u64 + 1;
        if event.seq != expected {
            return Err(crate::domain::RelayError::new(
                "EVENT_SEQUENCE_CONFLICT",
                format!(
                    "Expected sequence {expected} for run {}, received {}",
                    event.run_id, event.seq
                ),
            ));
        }
        events.push(event);
        Ok(())
    }

    fn list(&self, run_id: &str) -> Result<Vec<RelayEvent>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .get(run_id)
            .cloned()
            .unwrap_or_default())
    }

    fn find_run_id_by_worker(&self, worker_session_id: &str) -> Result<Option<String>> {
        for (run_id, events) in self.events.lock().unwrap().iter() {
            if events
                .iter()
                .any(|event| event.worker_session_id.as_deref() == Some(worker_session_id))
            {
                return Ok(Some(run_id.clone()));
            }
        }
        Ok(None)
    }

    fn list_run_ids(&self) -> Result<Vec<String>> {
        Ok(self.events.lock().unwrap().keys().cloned().collect())
    }
}
