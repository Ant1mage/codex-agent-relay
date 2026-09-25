import Database from 'better-sqlite3'
import {
  hostSessionSchema,
  hostSessionUpsertSchema,
  type HostSession,
  type HostSessionUpsert,
} from '@relay/protocol'
import type { HostSessionStore } from './host-sessions.js'

export class SqliteHostSessionStore implements HostSessionStore {
  readonly #database: Database.Database

  constructor(path: string) {
    this.#database = new Database(path)
    this.#database.pragma('journal_mode = WAL')
    this.#database.exec(`
      CREATE TABLE IF NOT EXISTS host_sessions (
        id TEXT PRIMARY KEY,
        native_session_id TEXT NOT NULL UNIQUE,
        display_name TEXT NOT NULL,
        data_json TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS host_sessions_updated_idx ON host_sessions(updated_at DESC);
    `)
  }

  upsertCodex(input: HostSessionUpsert): HostSession {
    const update = hostSessionUpsertSchema.parse(input)
    const id = `codex:${update.nativeSessionId}`
    const existing = this.get(id)
    const now = new Date().toISOString()
    const session = hostSessionSchema.parse({
      id,
      host: 'codex',
      nativeSessionId: update.nativeSessionId,
      displayName: update.displayName,
      nameSource: 'codex',
      cwd: update.cwd,
      ...(update.model ? { model: update.model } : existing?.model ? { model: existing.model } : {}),
      status: update.status,
      startedAt: existing?.startedAt ?? now,
      updatedAt: now,
      ...(update.status === 'ended' ? { endedAt: existing?.endedAt ?? now } : {}),
    })
    this.#database
      .prepare(`
        INSERT INTO host_sessions (id, native_session_id, display_name, data_json, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
          native_session_id = excluded.native_session_id,
          display_name = excluded.display_name,
          data_json = excluded.data_json,
          updated_at = excluded.updated_at
      `)
      .run(id, session.nativeSessionId, session.displayName, JSON.stringify(session), session.updatedAt)
    return session
  }

  get(id: string): HostSession | undefined {
    const row = this.#database
      .prepare('SELECT data_json FROM host_sessions WHERE id = ?')
      .get(id) as { data_json: string } | undefined
    return row ? hostSessionSchema.parse(JSON.parse(row.data_json)) : undefined
  }

  list(): HostSession[] {
    const rows = this.#database
      .prepare('SELECT data_json FROM host_sessions ORDER BY updated_at DESC')
      .all() as Array<{ data_json: string }>
    return rows.map((row) => hostSessionSchema.parse(JSON.parse(row.data_json)))
  }

  close(): void {
    this.#database.close()
  }
}
