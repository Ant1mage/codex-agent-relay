import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { DatabaseSync } from 'node:sqlite'

/**
 * Writes a small, realistic event log so the inspector and the tray can be used
 * without waiting for a real Codex delegation.
 *
 *   pnpm seed:demo                       # -> output/demo/relay.sqlite
 *   RELAY_DB_PATH=<path> pnpm relayd     # serve it
 *   open http://127.0.0.1:7352/#t=<token from ~/.relay/server.json>
 *
 * Never point this at a database you care about: it drops the three Relay
 * tables first.
 */

const target = resolve(process.argv[2] ?? 'output/demo/relay.sqlite')
mkdirSync(dirname(target), { recursive: true })
const database = new DatabaseSync(target)
database.exec(`
  DROP TABLE IF EXISTS relay_events;
  DROP TABLE IF EXISTS host_sessions;
  DROP TABLE IF EXISTS relay_control_commands;
  CREATE TABLE relay_events (
    id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL, step_id TEXT, worker_session_id TEXT,
    seq INTEGER NOT NULL CHECK (seq > 0), timestamp TEXT NOT NULL, type TEXT NOT NULL,
    data_json TEXT NOT NULL, native_event_json TEXT, PRIMARY KEY (run_id, seq)
  );
  CREATE TABLE host_sessions (
    id TEXT PRIMARY KEY, native_session_id TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL,
    data_json TEXT NOT NULL, updated_at TEXT NOT NULL
  );
  CREATE TABLE relay_control_commands (
    id TEXT PRIMARY KEY, type TEXT NOT NULL, worker_session_id TEXT NOT NULL, status TEXT NOT NULL,
    created_at TEXT NOT NULL, error TEXT
  );
`)

const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString()

const sessions = [
  { id: 'codex:inspector', name: 'Wire the inspector to the daemon', cwd: process.cwd(), started: minutesAgo(46) },
  { id: 'codex:adapters', name: 'Clean up the adapter registry', cwd: resolve(process.cwd(), 'packages'), started: minutesAgo(180) },
]
const insertSession = database.prepare(
  'INSERT INTO host_sessions (id, native_session_id, display_name, data_json, updated_at) VALUES (?, ?, ?, ?, ?)',
)
for (const [index, session] of sessions.entries()) {
  const updatedAt = minutesAgo(index === 0 ? 1 : 12)
  insertSession.run(
    session.id,
    session.id.replace('codex:', ''),
    session.name,
    JSON.stringify({
      id: session.id,
      host: 'codex',
      nativeSessionId: session.id.replace('codex:', ''),
      displayName: session.name,
      nameSource: 'codex',
      cwd: session.cwd,
      status: 'active',
      startedAt: session.started,
      updatedAt,
    }),
    updatedAt,
  )
}

const insertEvent = database.prepare(
  'INSERT INTO relay_events (id, run_id, step_id, worker_session_id, seq, timestamp, type, data_json, native_event_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)',
)

interface SeedRun {
  id: string
  sessionId: string
  profileId: string
  task: string
  cwd: string
  status: 'running' | 'awaiting_host'
  startedMinutesAgo: number
  workerRuntime: string
}

function seedRun(run: SeedRun): void {
  const started = minutesAgo(run.startedMinutesAgo)
  const stepId = `${run.id}:step-1`
  const workerId = `${run.id}:worker-1`
  const entity = {
    run: {
      id: run.id,
      hostSessionId: run.sessionId,
      profileId: run.profileId,
      task: run.task,
      cwd: run.cwd,
      accessMode: 'write',
      isolation: 'shared',
      status: run.status === 'running' ? 'running' : 'awaiting_host',
      createdAt: started,
      updatedAt: minutesAgo(1),
    },
    step: {
      id: stepId,
      runId: run.id,
      profileId: run.profileId,
      task: run.task,
      accessMode: 'write',
      isolation: 'shared',
      status: run.status === 'running' ? 'running' : 'awaiting_host',
      iteration: 1,
      createdAt: started,
      updatedAt: minutesAgo(1),
    },
    worker: {
      id: workerId,
      runId: run.id,
      stepId,
      iteration: 1,
      runtimeId: run.workerRuntime,
      status: run.status === 'running' ? 'running' : 'completed',
      startedAt: started,
      ...(run.status === 'running' ? {} : { endedAt: minutesAgo(2) }),
    },
  }
  insertEvent.run(`${run.id}:e1`, run.id, null, null, 1, started, 'run/created', JSON.stringify({ run: entity.run }))
  insertEvent.run(`${run.id}:e2`, run.id, stepId, null, 2, started, 'step/created', JSON.stringify({ step: entity.step }))
  insertEvent.run(`${run.id}:e3`, run.id, stepId, workerId, 3, started, 'worker/started', JSON.stringify({ worker: entity.worker }))

  const activity: Array<[RelayEventType, unknown]> = [
    ['tool/read', { path: 'apps/relayd/src/server.ts' }],
    ['tool/read', { path: 'apps/relayd/src/store.ts' }],
    ['tool/read', { path: 'apps/web/src/App.tsx' }],
    ['tool/search', { query: 'EventSource' }],
    ['tool/command', { command: 'pnpm typecheck' }],
    ['test/result', { summary: '114 passed' }],
    ['tool/edit', { path: 'apps/web/src/store.ts', diff: { additions: 18, deletions: 4 } }],
    ['tool/edit', { path: 'apps/relayd/src/server.ts', diff: { additions: 42, deletions: 9 } }],
    ['worker/message', { text: 'Streaming projection is wired; verifying the SSE fan-out.' }],
  ]
  let seq = 3
  activity.forEach(([type, data], index) => {
    seq += 1
    insertEvent.run(
      `${run.id}:e${seq}`,
      run.id,
      stepId,
      workerId,
      seq,
      minutesAgo(Math.max(1, run.startedMinutesAgo - index * 2)),
      type,
      JSON.stringify(data),
    )
  })
  if (run.status === 'awaiting_host') {
    seq += 1
    insertEvent.run(`${run.id}:e${seq}`, run.id, stepId, workerId, seq, minutesAgo(2), 'worker/completed', JSON.stringify({ status: 'completed' }))
  }
}

type RelayEventType = 'tool/read' | 'tool/search' | 'tool/command' | 'test/result' | 'tool/edit' | 'worker/message'

seedRun({ id: 'run-inspector', sessionId: 'codex:inspector', profileId: 'deepseek-code', task: 'Wire the tray and the inspector to the daemon', cwd: sessions[0]!.cwd, status: 'running', startedMinutesAgo: 6, workerRuntime: 'runtime:deepseek' })
seedRun({ id: 'run-audit', sessionId: 'codex:inspector', profileId: 'deepseek-research', task: 'Audit the event store read path for lock contention', cwd: sessions[0]!.cwd, status: 'awaiting_host', startedMinutesAgo: 24, workerRuntime: 'runtime:deepseek' })
seedRun({ id: 'run-registry', sessionId: 'codex:adapters', profileId: 'deepseek-code', task: 'Make a new CLI adapter a one-file change', cwd: sessions[1]!.cwd, status: 'running', startedMinutesAgo: 9, workerRuntime: 'runtime:deepseek' })

database.close()
process.stdout.write(`Seeded demo log at ${target}\n`)
