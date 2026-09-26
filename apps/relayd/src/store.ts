import { randomUUID } from 'node:crypto'
import { DatabaseSync } from 'node:sqlite'
import { projectRun } from '@relay/core/projection'
import {
  hostSessionSchema,
  relayEventSchema,
  type AgentProfile,
  type HostSession,
  type RelayEvent,
  type Runtime,
} from '@relay/protocol'
import type {
  CodexStatus,
  EventBatch,
  InspectorSnapshot,
  MenuView,
  RunView,
  SessionView,
} from '@relay/relay-api'

/**
 * Read side of the Relay event store. The MCP process (packages/mcp) owns
 * execution and is the only writer of runs and events; the daemon only reads
 * them and appends cancellation requests to the same control queue the desktop
 * used, which is what keeps the two processes from needing a leader or a lock.
 */

const SCHEMA = `
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
`

const ACTIVE = new Set(['starting', 'running'])

interface EventRow {
  id: string
  run_id: string
  step_id: string | null
  worker_session_id: string | null
  seq: number
  timestamp: string
  type: string
  data_json: string
  native_event_json: string | null
}

export interface ProjectionInput {
  runtimes: Runtime[]
  profiles: AgentProfile[]
  diagnostics: string[]
  codex: CodexStatus
}

/** First line of a delegated task, cut to a length a menu can show. */
export function taskPreview(task: string, max = 48): string {
  const line = task.split('\n').find((candidate) => candidate.trim().length > 0)?.trim() ?? ''
  const collapsed = line.replace(/\s+/g, ' ')
  return collapsed.length > max ? `${collapsed.slice(0, max - 1).trimEnd()}…` : collapsed
}

export class RelayStore {
  readonly #database: DatabaseSync
  #cache:
    | { revision: string; sessions: HostSession[]; runs: RunView[]; events: Map<string, RelayEvent[]> }
    | undefined
  #environment: { runtimes: Runtime[]; profiles: AgentProfile[] } = { runtimes: [], profiles: [] }

  constructor(databasePath: string) {
    this.#database = new DatabaseSync(databasePath)
    this.#database.exec('PRAGMA journal_mode = WAL;')
    this.#database.exec(SCHEMA)
  }

  close(): void {
    this.#database.close()
  }

  /**
   * Cheap change stamp. PRAGMA data_version moves when another connection
   * writes, which is exactly this topology: the MCP process appends events.
   */
  revision(): string {
    const events = this.#database
      .prepare('SELECT COALESCE(MAX(rowid), 0) AS last FROM relay_events')
      .get() as { last: number } | undefined
    const sessions = this.#database
      .prepare("SELECT COUNT(*) AS n, COALESCE(MAX(updated_at), '') AS at FROM host_sessions")
      .get() as { n: number; at: string } | undefined
    const version = this.#database.prepare('PRAGMA data_version').get() as
      | { data_version: number }
      | undefined
    return `${events?.last ?? 0}:${sessions?.n ?? 0}:${sessions?.at ?? ''}:${version?.data_version ?? 0}`
  }

  #parseEvent(row: EventRow): RelayEvent {
    return relayEventSchema.parse({
      id: row.id,
      runId: row.run_id,
      ...(row.step_id ? { stepId: row.step_id } : {}),
      ...(row.worker_session_id ? { workerSessionId: row.worker_session_id } : {}),
      seq: row.seq,
      timestamp: row.timestamp,
      type: row.type,
      data: JSON.parse(row.data_json) as unknown,
      ...(row.native_event_json ? { nativeEvent: JSON.parse(row.native_event_json) as unknown } : {}),
    })
  }

  /** Every stored run, projected. Cached until the store changes. */
  #projected(): { sessions: HostSession[]; runs: RunView[]; events: Map<string, RelayEvent[]> } {
    const revision = this.revision()
    if (this.#cache && this.#cache.revision === revision) return this.#cache

    const sessions = this.#database
      .prepare('SELECT data_json FROM host_sessions ORDER BY updated_at DESC')
      .all()
      .map((row) => hostSessionSchema.parse(JSON.parse(String(row.data_json))))

    const runIds = this.#database
      .prepare('SELECT DISTINCT run_id FROM relay_events ORDER BY timestamp ASC')
      .all()
      .map((row) => String(row.run_id))
    const query = this.#database.prepare('SELECT * FROM relay_events WHERE run_id = ? ORDER BY seq ASC')
    const events = new Map<string, RelayEvent[]>()
    const runs: RunView[] = []
    for (const runId of runIds) {
      const parsed = (query.all(runId) as unknown as EventRow[]).map((row) => this.#parseEvent(row))
      const projection = projectRun(parsed)
      events.set(runId, parsed)
      runs.push({ run: projection.run, steps: projection.steps, workers: projection.workers })
    }
    runs.sort((left, right) => right.run.createdAt.localeCompare(left.run.createdAt))
    const value = { revision, sessions, runs, events }
    this.#cache = value
    return value
  }

  /** Runs ordered by creation, newest first. */
  runViews(): RunView[] {
    return this.#projected().runs
  }

  eventsFor(runId: string, after = 0): EventBatch {
    const events = (this.#projected().events.get(runId) ?? []).filter((event) => event.seq > after)
    return { runId, events }
  }

  /** Highest sequence per run, used as the SSE fan-out cursor. */
  cursors(): Map<string, number> {
    const cursors = new Map<string, number>()
    for (const [runId, events] of this.#projected().events) {
      const last = events.at(-1)
      if (last) cursors.set(runId, last.seq)
    }
    return cursors
  }

  sessionsWithRuns(): SessionView[] {
    const { sessions, runs } = this.#projected()
    return sessions.map((session) => {
      const sessionRuns = runs.filter((view) => view.run.hostSessionId === session.id)
      const activeWorkers = sessionRuns.reduce(
        (total, view) => total + view.workers.filter((worker) => ACTIVE.has(worker.status)).length,
        0,
      )
      const awaitingHost = sessionRuns.filter((view) => view.run.status === 'awaiting_host').length
      return { session, runs: sessionRuns, activeWorkers, awaitingHost }
    })
  }

  /** The daemon detects runtimes on its own schedule; the store just holds them. */
  setEnvironment(value: { runtimes: Runtime[]; profiles: AgentProfile[] }): void {
    this.#environment = value
  }

  runtimesAndProfiles(): { runtimes: Runtime[]; profiles: AgentProfile[] } {
    return this.#environment
  }

  snapshot(input: ProjectionInput): InspectorSnapshot {
    return {
      sessions: this.sessionsWithRuns(),
      runtimes: input.runtimes,
      profiles: input.profiles,
      diagnostics: input.diagnostics,
      codex: input.codex,
      generatedAt: new Date().toISOString(),
    }
  }

  /**
   * The compact projection the tray renders: the same facts as the snapshot,
   * minus what a menu cannot show, so the two surfaces cannot disagree.
   */
  menu(input: { codex: CodexStatus }): MenuView {
    const { runtimes, profiles } = this.#environment
    const sessions = this.sessionsWithRuns()
    const profileName = (id: string) => profiles.find((profile) => profile.id === id)?.name ?? id
    const menuSessions = sessions.map((view) => ({
      id: view.session.id,
      displayName: view.session.displayName,
      cwd: view.session.cwd,
      activeWorkers: view.runs.flatMap((runView) =>
        runView.workers
          .filter((worker) => ACTIVE.has(worker.status))
          .map((worker) => {
            const step = runView.steps.find((candidate) => candidate.id === worker.stepId)
            const profile = profileName(step?.profileId ?? runView.run.profileId)
            const task = taskPreview(step?.task ?? runView.run.task)
            return {
              workerSessionId: worker.id,
              runId: runView.run.id,
              label: task ? `${profile} · ${task}` : profile,
            }
          }),
      ),
    }))
    const agents = profiles.map((profile) => {
      const runtime = runtimes.find((candidate) => candidate.id === profile.runtimeId)
      const blocked: 'auth' | 'missing' | 'disabled' | undefined =
        !runtime || runtime.health === 'unavailable'
          ? 'missing'
          : runtime.health === 'authentication_required'
            ? 'auth'
            : profile.enabled
              ? undefined
              : 'disabled'
      return { id: profile.id, name: profile.name, ...(blocked ? { blocked } : {}) }
    })
    const runningWorkers = menuSessions.reduce((total, session) => total + session.activeWorkers.length, 0)
    const awaitingHost = sessions.reduce((total, view) => total + view.awaitingHost, 0)
    return {
      status: menuStatus(runtimes, profiles, input.codex),
      runningWorkers,
      awaitingHost,
      sessions: menuSessions,
      agents,
      codex: input.codex,
    }
  }

  cancelWorker(workerSessionId: string): { accepted: boolean; message: string } {
    this.#database
      .prepare(`
        INSERT INTO relay_control_commands (id, type, worker_session_id, status, created_at, error)
        VALUES (?, 'cancel-worker', ?, 'pending', ?, NULL)
      `)
      .run(randomUUID(), workerSessionId, new Date().toISOString())
    return { accepted: true, message: 'Cancellation requested' }
  }

  cancelSession(hostSessionId: string): { accepted: boolean; count: number } {
    const active = new Set<string>()
    for (const view of this.#projected().runs) {
      if (view.run.hostSessionId !== hostSessionId) continue
      for (const worker of view.workers) {
        if (ACTIVE.has(worker.status)) active.add(worker.id)
      }
    }
    for (const workerSessionId of active) this.cancelWorker(workerSessionId)
    return { accepted: true, count: active.size }
  }
}

/** Relay's environment state: a runtime that can run, and Codex wired up. */
export function menuStatus(
  runtimes: Runtime[],
  profiles: AgentProfile[],
  codex: CodexStatus,
): MenuView['status'] {
  if (runtimes.length === 0) return 'noRuntime'
  const usable = profiles.some(
    (profile) =>
      profile.enabled &&
      runtimes.some((runtime) => runtime.id === profile.runtimeId && runtime.health === 'available'),
  )
  return codex.configured && usable ? 'ready' : 'needsSetup'
}
