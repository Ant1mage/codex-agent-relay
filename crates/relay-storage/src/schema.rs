//! The schema Relay owns.
//!
//! Version 1 is the Rust baseline. A file carrying any other version is treated
//! as old development data: Relay's own tables are dropped and recreated rather
//! than migrated, because the previous implementation's data is not something a
//! user keeps.

use relay_core::Result;

use crate::Database;

pub const SCHEMA_VERSION: i64 = 1;

const TABLES: [&str; 3] = ["relay_events", "host_sessions", "relay_control_commands"];

pub fn migrate(database: &Database) -> Result<()> {
    let version: i64 = database.with(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))?;

    if version != SCHEMA_VERSION {
        if version != 0 {
            tracing::warn!(
                "relay.sqlite carries schema version {version}; resetting Relay's tables to version {SCHEMA_VERSION}"
            );
            for table in TABLES {
                database.with(|connection| connection.execute(&format!("DROP TABLE IF EXISTS {table}"), []))?;
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

            CREATE TABLE IF NOT EXISTS host_sessions (
              id TEXT PRIMARY KEY,
              native_session_id TEXT NOT NULL UNIQUE,
              display_name TEXT NOT NULL,
              data_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS host_sessions_updated_idx ON host_sessions(updated_at DESC);

            CREATE TABLE IF NOT EXISTS relay_control_commands (
              id TEXT PRIMARY KEY,
              type TEXT NOT NULL,
              worker_session_id TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              error TEXT
            );
            CREATE INDEX IF NOT EXISTS relay_control_pending_idx
              ON relay_control_commands(status, created_at);
            "#,
        )?;
        connection.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
    })
}
