//! Relay's SQLite persistence.
//!
//! This crate owns the schema and nothing else: Run state machines, policy and
//! lifecycle stay in `relay-core`. Storage only answers "append this", "read
//! that".
//!
//! The schema is the clean Rust baseline. Relay's old development database is
//! deliberately not migrated: a file written by an earlier version is reset.

pub mod control_queue;
pub mod event_store;
pub mod host_sessions;
pub mod schema;

use std::path::Path;
use std::sync::Mutex;

use relay_core::{RelayError, Result};
use rusqlite::Connection;

pub use control_queue::{RelayControlCommand, SqliteControlQueue};
pub use event_store::SqliteEventStore;
pub use host_sessions::{end_session, SqliteHostSessionStore};

/// One SQLite connection per process, serialised.
///
/// Relay's write volume is a handful of rows per event, so a mutex around a
/// single connection is faster and far simpler than a pool.
pub struct Database {
    connection: Mutex<Connection>,
    path: String,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    RelayError::new("STORAGE_FAILURE", format!("cannot create {}: {error}", parent.display()))
                })?;
            }
        }
        let connection = Connection::open(&path).map_err(storage_error)?;
        // WAL is required for the daemon-reads / MCP-writes topology.
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(storage_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_error)?;
        let database = Self { connection: Mutex::new(connection), path: path.display().to_string() };
        schema::migrate(&database)?;
        Ok(database)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Runs `work` on the connection while holding the lock.
    pub fn with<T>(&self, work: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let guard = self.connection.lock().unwrap();
        work(&guard).map_err(storage_error)
    }

    /// Runs `work` inside a transaction.
    pub fn transaction<T>(&self, work: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>) -> Result<T> {
        let mut guard = self.connection.lock().unwrap();
        let transaction = guard.transaction().map_err(storage_error)?;
        let value = work(&transaction).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(value)
    }
}

/// Keeps a Relay error's code intact when it travels through `rusqlite::Result`.
pub fn storage_error(error: rusqlite::Error) -> RelayError {
    if let rusqlite::Error::ToSqlConversionFailure(inner) = &error {
        if let Some(relay) = inner.downcast_ref::<RelayError>() {
            return relay.clone();
        }
    }
    RelayError::new("STORAGE_FAILURE", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_fresh_file_creates_the_schema() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("relay.sqlite")).unwrap();
        let version: i64 = database
            .with(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))
            .unwrap();
        assert_eq!(version, schema::SCHEMA_VERSION);
        for table in ["relay_events", "host_sessions", "relay_control_commands"] {
            let found: i64 = database
                .with(|connection| {
                    connection.query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get(0),
                    )
                })
                .unwrap();
            assert_eq!(found, 1, "missing table {table}");
        }
    }

    #[test]
    fn a_database_from_an_older_version_is_reset() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("relay.sqlite");
        {
            // An earlier Relay kept a differently shaped table at version 2.
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE relay_events (id TEXT, legacy_only INTEGER); PRAGMA user_version = 2;",
                )
                .unwrap();
        }
        let database = Database::open(&path).unwrap();
        let version: i64 = database
            .with(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))
            .unwrap();
        assert_eq!(version, schema::SCHEMA_VERSION);
        // Relay's own tables are the Rust baseline, not the old shape.
        let columns: i64 = database
            .with(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('relay_events') WHERE name = 'legacy_only'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(columns, 0);
        // Unrelated tables are left alone.
        database
            .with(|connection| connection.execute_batch("CREATE TABLE unrelated (id TEXT);"))
            .unwrap();
        assert!(Database::open(&path).is_ok());
    }
}
