//! The schema Relay owns.
//!
//! Version 2 is the Rust baseline after execution moved into the daemon. A file
//! carrying any other version is treated as old development data: Relay's own
//! tables are dropped and recreated rather than migrated, because the previous
//! implementation's data is not something a user keeps.

use relay_core::Result;

use crate::Database;

/// 2 removed the cross-process control queue: the daemon owns every worker, so
/// cancellation no longer has to travel through SQLite.
pub const SCHEMA_VERSION: i64 = 2;

const TABLES: [&str; 2] = ["relay_events", "host_sessions"];

/// Tables an earlier Relay owned and this one does not. They are dropped with
/// the rest when a file from another version is reset.
const REMOVED_TABLES: [&str; 1] = ["relay_control_commands"];

pub fn migrate(database: &Database) -> Result<()> {
    let version: i64 = database
        .with(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))?;

    if version != SCHEMA_VERSION {
        if version != 0 {
            tracing::warn!(
                "relay.sqlite carries schema version {version}; resetting Relay's tables to version {SCHEMA_VERSION}"
            );
            for table in TABLES.into_iter().chain(REMOVED_TABLES) {
                database.with(|connection| {
                    connection.execute(&format!("DROP TABLE IF EXISTS {table}"), [])
                })?;
            }
        }
        create(database)?;
    }

    // Cheap safety net: an interrupted first start could leave the file at 0.
    create(database)
}

fn create(database: &Database) -> Result<()> {
    database.with(|connection| {
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS relay_events (
              id TEXT NOT NULL UNIQUE,
              run_id TEXT NOT NULL,
              step_id TEXT,
              worker_session_id TEXT,
              seq INTEGER NOT NULL CHECK (seq > 0),
              timestamp TEXT NOT NULL,
              type TEXT NOT NULL,
              data_json TEXT NOT NULL,
              native_event_json TEXT,
              PRIMARY KEY (run_id, seq)
            );
            CREATE INDEX IF NOT EXISTS relay_events_timestamp_idx ON relay_events(timestamp);
            CREATE INDEX IF NOT EXISTS relay_events_worker_idx ON relay_events(worker_session_id);
            CREATE INDEX IF NOT EXISTS relay_events_type_idx ON relay_events(type);

            CREATE TABLE IF NOT EXISTS host_sessions (
              id TEXT PRIMARY KEY,
              native_session_id TEXT NOT NULL UNIQUE,
              display_name TEXT NOT NULL,
              data_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS host_sessions_updated_idx ON host_sessions(updated_at DESC);
            "#,
        )?;
        connection.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
    })
}
