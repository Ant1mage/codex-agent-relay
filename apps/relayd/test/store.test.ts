import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { RelayStore, menuStatus, taskPreview } from '../src/store.js'
import { profiles, runtimes, seedDatabase } from './fixtures.js'

let directory: string
let store: RelayStore

beforeEach(() => {
  directory = mkdtempSync(join(tmpdir(), 'relay-store-'))
  const database = join(directory, 'relay.sqlite')
  seedDatabase(database)
  store = new RelayStore(database)
  store.setEnvironment({ runtimes, profiles })
})

afterEach(() => {
  store.close()
  rmSync(directory, { recursive: true, force: true })
})

const codex = { checks: [], configured: true }

describe('RelayStore', () => {
  it('projects runs and counts the workers that are still active', () => {
    const sessions = store.sessionsWithRuns()
    expect(sessions).toHaveLength(1)
    expect(sessions[0]?.session.displayName).toBe('Refine the inspector')
    expect(sessions[0]?.activeWorkers).toBe(1)
    expect(sessions[0]?.awaitingHost).toBe(0)
    expect(sessions[0]?.runs[0]?.run.status).toBe('running')
    expect(sessions[0]?.runs[0]?.steps).toHaveLength(1)
  })

  it('serves events after a cursor', () => {
    expect(store.eventsFor('run-1').events.map((event) => event.seq)).toEqual([1, 2, 3, 4, 5])
    expect(store.eventsFor('run-1', 3).events.map((event) => event.seq)).toEqual([4, 5])
    expect(store.cursors().get('run-1')).toBe(5)
  })

  it('stops re-projecting until the store actually changes', () => {
    const first = store.revision()
    expect(store.revision()).toBe(first)
    const database = new DatabaseSync(join(directory, 'relay.sqlite'))
    database
      .prepare(
        'INSERT INTO relay_events (id, run_id, step_id, worker_session_id, seq, timestamp, type, data_json, native_event_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)',
      )
      .run('e6', 'run-1', 'step-1', 'worker-1', 6, '2026-01-01T10:05:00.000Z', 'tool/command', JSON.stringify({ command: 'pnpm test' }))
    database.close()
    expect(store.revision()).not.toBe(first)
    expect(store.eventsFor('run-1').events).toHaveLength(6)
  })

  it('builds the tray projection from the same facts as the snapshot', () => {
    const menu = store.menu({ codex })
    expect(menu.status).toBe('ready')
    expect(menu.runningWorkers).toBe(1)
    expect(menu.awaitingHost).toBe(0)
    expect(menu.sessions[0]?.activeWorkers[0]).toEqual({
      workerSessionId: 'worker-1',
      runId: 'run-1',
      label: 'DeepSeek Code · Wire the tray to the daemon',
    })
    // A runtime that needs a sign-in blocks exactly its own agent.
    expect(menu.agents).toEqual([
      { id: 'deepseek-code', name: 'DeepSeek Code' },
      { id: 'kimi-code', name: 'Kimi Code', blocked: 'auth' },
    ])
  })

  it('queues cancellation for the database the MCP process reads', () => {
    expect(store.cancelSession('codex:one')).toEqual({ accepted: true, count: 1 })
    const database = new DatabaseSync(join(directory, 'relay.sqlite'))
    const rows = database
      .prepare('SELECT worker_session_id, status FROM relay_control_commands')
      .all() as Array<{ worker_session_id: string; status: string }>
    database.close()
    expect(rows).toEqual([{ worker_session_id: 'worker-1', status: 'pending' }])
  })

  it('reports an empty store instead of failing', () => {
    const empty = new RelayStore(join(directory, 'empty.sqlite'))
    empty.setEnvironment({ runtimes: [], profiles: [] })
    expect(empty.sessionsWithRuns()).toEqual([])
    expect(empty.menu({ codex }).status).toBe('noRuntime')
    empty.close()
  })
})

describe('taskPreview', () => {
  it('keeps the first line and truncates long tasks', () => {
    expect(taskPreview('Wire the tray\nsecond line')).toBe('Wire the tray')
    expect(taskPreview('x'.repeat(80))).toHaveLength(48)
  })
})

describe('menuStatus', () => {
  it('needs a usable runtime and a configured Codex', () => {
    expect(menuStatus([], profiles, codex)).toBe('noRuntime')
    expect(menuStatus(runtimes, profiles, { checks: [], configured: false })).toBe('needsSetup')
    expect(menuStatus(runtimes, profiles, codex)).toBe('ready')
  })
})
