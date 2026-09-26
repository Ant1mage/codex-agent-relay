import { DatabaseSync } from 'node:sqlite'
import type { AgentProfile, Runtime } from '@relay/protocol'

/** Seeds a database that looks like the MCP process had just run a delegation. */
export function seedDatabase(path: string): void {
  const database = new DatabaseSync(path)
  database.exec(`
    CREATE TABLE IF NOT EXISTS relay_events (
      id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL, step_id TEXT, worker_session_id TEXT,
      seq INTEGER NOT NULL CHECK (seq > 0), timestamp TEXT NOT NULL, type TEXT NOT NULL,
      data_json TEXT NOT NULL, native_event_json TEXT, PRIMARY KEY (run_id, seq)
    );
    CREATE TABLE IF NOT EXISTS host_sessions (
      id TEXT PRIMARY KEY, native_session_id TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL,
      data_json TEXT NOT NULL, updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS relay_control_commands (
      id TEXT PRIMARY KEY, type TEXT NOT NULL, worker_session_id TEXT NOT NULL, status TEXT NOT NULL,
      created_at TEXT NOT NULL, error TEXT
    );
  `)
  const at = (minutes: number) => new Date(Date.UTC(2026, 0, 1, 10, minutes)).toISOString()
  const session = {
    id: 'codex:one',
    host: 'codex',
    nativeSessionId: 'one',
    displayName: 'Refine the inspector',
    nameSource: 'codex',
    cwd: '/tmp/relay',
    status: 'active',
    startedAt: at(0),
    updatedAt: at(4),
  }
  database
    .prepare('INSERT INTO host_sessions (id, native_session_id, display_name, data_json, updated_at) VALUES (?, ?, ?, ?, ?)')
    .run(session.id, session.nativeSessionId, session.displayName, JSON.stringify(session), session.updatedAt)

  const insert = database.prepare(
    'INSERT INTO relay_events (id, run_id, step_id, worker_session_id, seq, timestamp, type, data_json, native_event_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)',
  )
  const run = {
    id: 'run-1',
    hostSessionId: session.id,
    profileId: 'deepseek-code',
    task: 'Wire the tray to the daemon\nsecond line',
    cwd: '/tmp/relay',
    accessMode: 'write',
    isolation: 'shared',
    status: 'running',
    createdAt: at(2),
    updatedAt: at(4),
  }
  const step = {
    id: 'step-1',
    runId: run.id,
    profileId: 'deepseek-code',
    task: run.task,
    accessMode: 'write',
    isolation: 'shared',
    status: 'running',
    iteration: 1,
    createdAt: at(2),
    updatedAt: at(4),
  }
  const worker = {
    id: 'worker-1',
    runId: run.id,
    stepId: step.id,
    iteration: 1,
    runtimeId: 'runtime:deepseek',
    status: 'running',
    startedAt: at(2),
  }
  insert.run('e1', run.id, null, null, 1, at(2), 'run/created', JSON.stringify({ run }))
  insert.run('e2', run.id, step.id, null, 2, at(2), 'step/created', JSON.stringify({ step }))
  insert.run('e3', run.id, step.id, worker.id, 3, at(2), 'worker/started', JSON.stringify({ worker }))
  insert.run('e4', run.id, step.id, worker.id, 4, at(3), 'tool/read', JSON.stringify({ path: 'src/a.ts' }))
  insert.run(
    'e5',
    run.id,
    step.id,
    worker.id,
    5,
    at(4),
    'tool/edit',
    JSON.stringify({ path: 'src/b.ts', diff: { additions: 4, deletions: 1 } }),
  )
  database.close()
}

export const runtimes: Runtime[] = [
  {
    id: 'runtime:deepseek',
    adapterId: 'deepseek-harness',
    executablePath: 'dsh',
    health: 'available',
    capabilities: {
      nonInteractive: true,
      structuredEvents: true,
      cwd: true,
      resume: true,
      send: false,
      cancel: true,
      childSessions: false,
    },
  },
  {
    id: 'runtime:kimi',
    adapterId: 'kimi-code',
    executablePath: 'kimi',
    health: 'authentication_required',
    capabilities: {
      nonInteractive: true,
      structuredEvents: true,
      cwd: true,
      resume: false,
      send: false,
      cancel: true,
      childSessions: false,
    },
  },
]

export const profiles: AgentProfile[] = [
  {
    id: 'deepseek-code',
    name: 'DeepSeek Code',
    runtimeId: 'runtime:deepseek',
    description: 'Coding worker',
    capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: false },
    enabled: true,
  },
  {
    id: 'kimi-code',
    name: 'Kimi Code',
    runtimeId: 'runtime:kimi',
    description: 'Coding worker',
    capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: true },
    enabled: true,
  },
]
