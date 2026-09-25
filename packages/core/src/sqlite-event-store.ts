import Database from 'better-sqlite3'
import { RelayError, relayEventSchema, type RelayEvent } from '@relay/protocol'
import type { EventStore } from './memory-event-store.js'

interface EventRow {
  id: string
  run_id: string
  worker_session_id: string | null
  seq: number
  timestamp: string
  type: RelayEvent['type']
  data_json: string
  native_event_json: string | null
}

export class SqliteEventStore implements EventStore {
  readonly #database: Database.Database

  constructor(path: string) {
    this.#database = new Database(path)
    this.#database.pragma('journal_mode = WAL')
    this.#database.pragma('foreign_keys = ON')
    this.#migrate()
  }

  #migrate(): void {
    const version = this.#database.pragma('user_version', { simple: true }) as number
    if (version > 1) throw new Error(`Unsupported Relay database version ${version}`)
    if (version === 0) {
      this.#database.exec(`
        CREATE TABLE relay_events (
          id TEXT NOT NULL UNIQUE,
          run_id TEXT NOT NULL,
          worker_session_id TEXT,
          seq INTEGER NOT NULL CHECK (seq > 0),
          timestamp TEXT NOT NULL,
          type TEXT NOT NULL,
          data_json TEXT NOT NULL,
          native_event_json TEXT,
          PRIMARY KEY (run_id, seq)
        );
        CREATE INDEX relay_events_timestamp_idx ON relay_events(timestamp);
        PRAGMA user_version = 1;
      `)
    }
  }

  append(input: RelayEvent): void {
    const event = relayEventSchema.parse(input)
    const transaction = this.#database.transaction(() => {
      const last = this.#database
        .prepare('SELECT MAX(seq) AS seq FROM relay_events WHERE run_id = ?')
        .get(event.runId) as { seq: number | null }
      const expected = (last.seq ?? 0) + 1
      if (event.seq !== expected) {
        throw new RelayError(
          'EVENT_SEQUENCE_CONFLICT',
          `Expected sequence ${expected} for run ${event.runId}, received ${event.seq}`,
        )
      }
      this.#database
        .prepare(`
          INSERT INTO relay_events (
            id, run_id, worker_session_id, seq, timestamp, type, data_json, native_event_json
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        `)
        .run(
          event.id,
          event.runId,
          event.workerSessionId ?? null,
          event.seq,
          event.timestamp,
          event.type,
          JSON.stringify(event.data),
          event.nativeEvent === undefined ? null : JSON.stringify(event.nativeEvent),
        )
    })
    transaction()
  }

  list(runId: string): RelayEvent[] {
    const rows = this.#database
      .prepare('SELECT * FROM relay_events WHERE run_id = ? ORDER BY seq ASC')
      .all(runId) as EventRow[]
    return rows.map((row) =>
      relayEventSchema.parse({
        id: row.id,
        runId: row.run_id,
        ...(row.worker_session_id ? { workerSessionId: row.worker_session_id } : {}),
        seq: row.seq,
        timestamp: row.timestamp,
        type: row.type,
        data: JSON.parse(row.data_json) as unknown,
        ...(row.native_event_json
          ? { nativeEvent: JSON.parse(row.native_event_json) as unknown }
          : {}),
      }),
    )
  }

  listRunIds(): string[] {
    const rows = this.#database
      .prepare('SELECT DISTINCT run_id FROM relay_events ORDER BY timestamp ASC')
      .all() as Array<{ run_id: string }>
    return rows.map((row) => row.run_id)
  }

  findRunIdByWorker(workerSessionId: string): string | undefined {
    const row = this.#database
      .prepare('SELECT run_id FROM relay_events WHERE worker_session_id = ? LIMIT 1')
      .get(workerSessionId) as { run_id: string } | undefined
    return row?.run_id
  }

  close(): void {
    this.#database.close()
  }
}
