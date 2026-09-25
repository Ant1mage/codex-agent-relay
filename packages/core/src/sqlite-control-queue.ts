import { randomUUID } from 'node:crypto'
import Database from 'better-sqlite3'

export interface RelayControlCommand {
  id: string
  type: 'cancel-worker'
  workerSessionId: string
  status: 'pending' | 'processing' | 'completed' | 'failed'
  createdAt: string
  error?: string
}

interface CommandRow {
  id: string
  type: RelayControlCommand['type']
  worker_session_id: string
  status: RelayControlCommand['status']
  created_at: string
  error: string | null
}

function fromRow(row: CommandRow): RelayControlCommand {
  return {
    id: row.id,
    type: row.type,
    workerSessionId: row.worker_session_id,
    status: row.status,
    createdAt: row.created_at,
    ...(row.error ? { error: row.error } : {}),
  }
}

export class SqliteControlQueue {
  readonly #database: Database.Database

  constructor(path: string) {
    this.#database = new Database(path)
    this.#database.pragma('journal_mode = WAL')
    this.#database.exec(`
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
    `)
    this.#database
      .prepare("UPDATE relay_control_commands SET status = 'pending' WHERE status = 'processing'")
      .run()
  }

  enqueueCancel(workerSessionId: string): RelayControlCommand {
    const command: RelayControlCommand = {
      id: randomUUID(),
      type: 'cancel-worker',
      workerSessionId,
      status: 'pending',
      createdAt: new Date().toISOString(),
    }
    this.#database
      .prepare(`
        INSERT INTO relay_control_commands (
          id, type, worker_session_id, status, created_at, error
        ) VALUES (?, ?, ?, ?, ?, NULL)
      `)
      .run(command.id, command.type, command.workerSessionId, command.status, command.createdAt)
    return command
  }

  claimNext(): RelayControlCommand | undefined {
    return this.#database.transaction(() => {
      const row = this.#database
        .prepare(`
          SELECT * FROM relay_control_commands
          WHERE status = 'pending'
          ORDER BY created_at ASC
          LIMIT 1
        `)
        .get() as CommandRow | undefined
      if (!row) return undefined
      this.#database
        .prepare("UPDATE relay_control_commands SET status = 'processing' WHERE id = ?")
        .run(row.id)
      return fromRow({ ...row, status: 'processing' })
    })()
  }

  complete(id: string): void {
    this.#database
      .prepare("UPDATE relay_control_commands SET status = 'completed', error = NULL WHERE id = ?")
      .run(id)
  }

  fail(id: string, error: string): void {
    this.#database
      .prepare("UPDATE relay_control_commands SET status = 'failed', error = ? WHERE id = ?")
      .run(error, id)
  }

  get(id: string): RelayControlCommand | undefined {
    const row = this.#database
      .prepare('SELECT * FROM relay_control_commands WHERE id = ?')
      .get(id) as CommandRow | undefined
    return row ? fromRow(row) : undefined
  }

  close(): void {
    this.#database.close()
  }
}
