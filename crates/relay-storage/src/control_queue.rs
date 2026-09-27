//! The control queue: how the daemon asks the MCP process to cancel a worker.
//!
//! The daemon never owns a worker process, so cancellation crosses processes
//! through this table. The MCP process drains it; the daemon only appends.

use std::sync::Arc;

use relay_core::{now, Result};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::Database;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayControlCommand {
    pub id: String,
    pub command_type: String,
    pub worker_session_id: String,
    pub status: String,
    pub created_at: String,
    pub error: Option<String>,
}

impl RelayControlCommand {
    pub fn is_cancel_worker(&self) -> bool {
        self.command_type == "cancel-worker"
    }
}

pub struct SqliteControlQueue {
    database: Arc<Database>,
}

impl SqliteControlQueue {
    pub fn new(database: Arc<Database>) -> Self {
        let queue = Self { database };
        queue.requeue_interrupted();
        queue
    }

    /// A crash mid-claim must not strand a cancellation.
    fn requeue_interrupted(&self) {
        let _ = self.database.with(|connection| {
            connection.execute(
                "UPDATE relay_control_commands SET status = 'pending' WHERE status = 'processing'",
                [],
            )?;
            Ok(())
        });
    }

    pub fn enqueue_cancel(&self, worker_session_id: &str) -> Result<RelayControlCommand> {
        let command = RelayControlCommand {
            id: Uuid::new_v4().to_string(),
            command_type: "cancel-worker".to_string(),
            worker_session_id: worker_session_id.to_string(),
            status: "pending".to_string(),
            created_at: now(),
            error: None,
        };
        self.database.with(|connection| {
            connection.execute(
                "INSERT INTO relay_control_commands (id, type, worker_session_id, status, created_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                rusqlite::params![
                    command.id,
                    command.command_type,
                    command.worker_session_id,
                    command.status,
                    command.created_at,
                ],
            )?;
            Ok(())
        })?;
        Ok(command)
    }

    pub fn claim_next(&self) -> Result<Option<RelayControlCommand>> {
        self.database.transaction(|transaction| {
            let row = transaction
                .query_row(
                    "SELECT id, type, worker_session_id, status, created_at, error
                     FROM relay_control_commands
                     WHERE status = 'pending'
                     ORDER BY created_at ASC
                     LIMIT 1",
                    [],
                    |row| {
                        Ok(RelayControlCommand {
                            id: row.get(0)?,
                            command_type: row.get(1)?,
                            worker_session_id: row.get(2)?,
                            status: row.get(3)?,
                            created_at: row.get(4)?,
                            error: row.get(5)?,
                        })
                    },
                )
                .optional()?;
            let Some(mut command) = row else {
                return Ok(None);
            };
            transaction.execute(
                "UPDATE relay_control_commands SET status = 'processing' WHERE id = ?1",
                [&command.id],
            )?;
            command.status = "processing".to_string();
            Ok(Some(command))
        })
    }

    pub fn complete(&self, id: &str) -> Result<()> {
        self.database.with(|connection| {
            connection.execute(
                "UPDATE relay_control_commands SET status = 'completed', error = NULL WHERE id = ?1",
                [id],
            )?;
            Ok(())
        })
    }

    pub fn fail(&self, id: &str, error: &str) -> Result<()> {
        self.database.with(|connection| {
            connection.execute(
                "UPDATE relay_control_commands SET status = 'failed', error = ?2 WHERE id = ?1",
                rusqlite::params![id, error],
            )?;
            Ok(())
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<RelayControlCommand>> {
        self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT id, type, worker_session_id, status, created_at, error
                     FROM relay_control_commands WHERE id = ?1",
                    [id],
                    |row| {
                        Ok(RelayControlCommand {
                            id: row.get(0)?,
                            command_type: row.get(1)?,
                            worker_session_id: row.get(2)?,
                            status: row.get(3)?,
                            created_at: row.get(4)?,
                            error: row.get(5)?,
                        })
                    },
                )
                .optional()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> (tempfile::TempDir, SqliteControlQueue) {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(Database::open(directory.path().join("relay.sqlite")).unwrap());
        let queue = SqliteControlQueue::new(database);
        (directory, queue)
    }

    #[test]
    fn a_command_is_claimed_once_and_can_be_completed() {
        let (_directory, queue) = queue();
        let command = queue.enqueue_cancel("worker:1").unwrap();
        let claimed = queue.claim_next().unwrap().unwrap();
        assert_eq!(claimed.id, command.id);
        assert_eq!(claimed.status, "processing");
        assert!(queue.claim_next().unwrap().is_none());
        queue.complete(&claimed.id).unwrap();
        assert_eq!(queue.get(&claimed.id).unwrap().unwrap().status, "completed");
    }

    #[test]
    fn a_claimed_command_is_requeued_after_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("relay.sqlite");
        {
            let database = Arc::new(Database::open(&path).unwrap());
            let queue = SqliteControlQueue::new(database);
            queue.enqueue_cancel("worker:2").unwrap();
            queue.claim_next().unwrap().unwrap();
        }
        let database = Arc::new(Database::open(&path).unwrap());
        let queue = SqliteControlQueue::new(database);
        let claimed = queue.claim_next().unwrap().unwrap();
        assert_eq!(claimed.worker_session_id, "worker:2");
    }

    #[test]
    fn failures_are_recorded_with_their_message() {
        let (_directory, queue) = queue();
        let command = queue.enqueue_cancel("worker:3").unwrap();
        queue.fail(&command.id, "worker is gone").unwrap();
        let stored = queue.get(&command.id).unwrap().unwrap();
        assert_eq!(stored.status, "failed");
        assert_eq!(stored.error.as_deref(), Some("worker is gone"));
    }
}
