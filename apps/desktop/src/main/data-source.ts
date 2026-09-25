import { randomUUID } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import type { AgentProfile, RelayPolicy, RelayPolicyOverride, Runtime } from '@relay/protocol'
import {
  hostSessionSchema,
  agentProfileSchema,
  relayEventSchema,
  relayPolicyOverrideSchema,
  relayPolicySchema,
} from '@relay/protocol'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { defaultPolicy, projectRun } from '@relay/core'
import type { DesktopSettings, DesktopSnapshot } from '../shared/api.js'

interface EventRow {
  id: string
  run_id: string
  worker_session_id: string | null
  seq: number
  timestamp: string
  type: string
  data_json: string
  native_event_json: string | null
}

export class DesktopDataSource {
  readonly #database: DatabaseSync
  readonly #settingsPath: string
  readonly #profilesPath: string
  readonly #adapter = new DeepSeekAdapter()
  #runtimes: Runtime[] = []
  #profiles: AgentProfile[] = []
  #diagnostics: string[] = []

  constructor(databasePath: string, settingsPath: string) {
    this.#database = new DatabaseSync(databasePath)
    this.#database.exec('PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;')
    this.#database.exec(`
      CREATE TABLE IF NOT EXISTS relay_events (
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
      CREATE INDEX IF NOT EXISTS relay_events_timestamp_idx ON relay_events(timestamp);
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
    `)
    this.#settingsPath = settingsPath
    this.#profilesPath = join(dirname(settingsPath), 'profiles.json')
  }

  async initialize(): Promise<void> {
    const detection = await this.#adapter.detect()
    this.#runtimes = detection.runtimes
    this.#diagnostics = detection.diagnostics
    const runtime = detection.runtimes[0]
    const defaults: AgentProfile[] = runtime
      ? [
          {
            id: 'deepseek-code',
            name: 'DeepSeek Code',
            runtimeId: runtime.id,
            description: 'Coding worker with workspace write and command capabilities.',
            capabilities: {
              readWorkspace: true,
              writeWorkspace: true,
              executeCommands: true,
              networkAccess: false,
            },
            enabled: true,
          },
          {
            id: 'deepseek-research',
            name: 'DeepSeek Research',
            runtimeId: runtime.id,
            description: 'Read-only research worker with network access.',
            capabilities: {
              readWorkspace: true,
              writeWorkspace: false,
              executeCommands: false,
              networkAccess: true,
            },
            enabled: true,
          },
        ]
      : []
    if (!runtime || !existsSync(this.#profilesPath)) {
      this.#profiles = defaults
      return
    }
    try {
      const stored = agentProfileSchema.array().parse(
        JSON.parse(readFileSync(this.#profilesPath, 'utf8')),
      )
      this.#profiles = stored.map((profile) => ({ ...profile, runtimeId: runtime.id }))
    } catch {
      this.#profiles = defaults
      this.#diagnostics.push('Invalid profiles.json; using built-in profiles.')
    }
  }

  settings(): DesktopSettings {
    if (existsSync(this.#settingsPath)) {
      try {
        const parsed = JSON.parse(readFileSync(this.#settingsPath, 'utf8')) as {
          policy?: unknown
          workspaceOverrides?: Record<string, unknown>
        }
        const workspaceOverrides: Record<string, RelayPolicyOverride> = {}
        for (const [workspace, override] of Object.entries(parsed.workspaceOverrides ?? {})) {
          workspaceOverrides[workspace] = relayPolicyOverrideSchema.parse(override)
        }
        return { policy: relayPolicySchema.parse(parsed.policy), workspaceOverrides }
      } catch {
        // Invalid user settings fall back to conservative defaults.
      }
    }
    return { policy: defaultPolicy, workspaceOverrides: {} }
  }

  saveSettings(input: DesktopSettings): DesktopSettings {
    const workspaceOverrides: Record<string, RelayPolicyOverride> = {}
    for (const [workspace, override] of Object.entries(input.workspaceOverrides)) {
      workspaceOverrides[workspace] = relayPolicyOverrideSchema.parse(override)
    }
    const settings = { policy: relayPolicySchema.parse(input.policy), workspaceOverrides }
    writeFileSync(this.#settingsPath, `${JSON.stringify(settings, null, 2)}\n`, 'utf8')
    return settings
  }

  saveProfile(input: AgentProfile): AgentProfile {
    const profile = agentProfileSchema.parse(input)
    if (!this.#runtimes.some((runtime) => runtime.id === profile.runtimeId)) {
      throw new Error(`Unknown runtime ${profile.runtimeId}`)
    }
    const index = this.#profiles.findIndex((candidate) => candidate.id === profile.id)
    if (index === -1) this.#profiles.push(profile)
    else this.#profiles[index] = profile
    writeFileSync(this.#profilesPath, `${JSON.stringify(this.#profiles, null, 2)}\n`, 'utf8')
    return profile
  }

  snapshot(): DesktopSnapshot {
    const sessions = this.#database
      .prepare('SELECT data_json FROM host_sessions ORDER BY updated_at DESC')
      .all()
      .map((row) => hostSessionSchema.parse(JSON.parse(String(row.data_json))))
    const runIds = this.#database
      .prepare('SELECT DISTINCT run_id FROM relay_events ORDER BY timestamp ASC')
      .all()
      .map((row) => String(row.run_id))
    const eventQuery = this.#database.prepare(
      'SELECT * FROM relay_events WHERE run_id = ? ORDER BY seq ASC',
    )
    const runs = runIds.map((runId) => {
      const events = (eventQuery.all(runId) as unknown as EventRow[]).map((row) =>
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
      const projection = projectRun(events)
      return { run: projection.run, workers: projection.workers, events }
    })
    return {
      sessions,
      runs: runs.sort((left, right) => right.run.createdAt.localeCompare(left.run.createdAt)),
      runtimes: this.#runtimes,
      profiles: this.#profiles,
      diagnostics: this.#diagnostics,
      settings: this.settings(),
      refreshedAt: new Date().toISOString(),
    }
  }

  cancelWorker(workerSessionId: string): { accepted: boolean; message: string } {
    this.#database
      .prepare(`
        INSERT INTO relay_control_commands (
          id, type, worker_session_id, status, created_at, error
        ) VALUES (?, 'cancel-worker', ?, 'pending', ?, NULL)
      `)
      .run(randomUUID(), workerSessionId, new Date().toISOString())
    return { accepted: true, message: 'Cancellation requested' }
  }

  close(): void {
    this.#database.close()
    void this.#adapter.dispose()
  }
}
