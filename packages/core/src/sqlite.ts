import { DatabaseSync, type StatementSync } from 'node:sqlite'

/**
 * Relay talks to SQLite through Node's built-in driver, not a native addon.
 *
 * Why: the MCP server has to run as a bundle shipped inside the app, and a
 * .node addon would have to be compiled for whichever runtime loads it (system
 * Node for development, Electron's Node inside the packaged app). node:sqlite
 * removes that entire class of failure — and Relay never needed anything
 * better-sqlite3 offered beyond prepared statements and transactions.
 *
 * The small surface below mirrors the handful of calls Relay makes, so the
 * stores stay readable.
 */
export interface SqliteStatement {
  get(...params: SqlValue[]): Record<string, unknown> | undefined
  all(...params: SqlValue[]): Array<Record<string, unknown>>
  run(...params: SqlValue[]): void
}

export type SqlValue = string | number | bigint | null | Uint8Array

export interface SqliteDatabase {
  prepare(sql: string): SqliteStatement
  exec(sql: string): void
  /** Reads a single-value pragma, e.g. `user_version`. */
  pragmaValue(name: string): number
  /** Runs `fn` inside a transaction, mirroring better-sqlite3's helper. */
  transaction<T>(fn: () => T): () => T
  close(): void
}

function statement(raw: StatementSync): SqliteStatement {
  return {
    get: (...params) => raw.get(...params) as Record<string, unknown> | undefined,
    all: (...params) => raw.all(...params) as Array<Record<string, unknown>>,
    run: (...params) => {
      raw.run(...params)
    },
  }
}

export function openSqliteDatabase(path: string): SqliteDatabase {
  const database = new DatabaseSync(path)
  database.exec('PRAGMA journal_mode = WAL')
  return {
    prepare: (sql) => statement(database.prepare(sql)),
    exec: (sql) => {
      database.exec(sql)
    },
    pragmaValue: (name) => {
      const row = database.prepare(`PRAGMA ${name}`).get() as Record<string, unknown> | undefined
      const value = row?.[name]
      return typeof value === 'number' ? value : 0
    },
    transaction: <T,>(fn: () => T) => () => {
      database.exec('BEGIN')
      try {
        const result = fn()
        database.exec('COMMIT')
        return result
      } catch (error) {
        database.exec('ROLLBACK')
        throw error
      }
    },
    close: () => database.close(),
  }
}
