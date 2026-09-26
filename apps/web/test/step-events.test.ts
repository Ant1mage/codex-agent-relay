import { describe, expect, it } from 'vitest'
import type { RelayEvent } from '@relay/protocol'
import type { RunView } from '@relay/relay-api'
import { eventsForStep } from '../src/lib/step-events.js'

const view: RunView = {
  run: {
    id: 'run-1',
    hostSessionId: 'codex:one',
    profileId: 'deepseek-code',
    task: 'task',
    cwd: '/tmp',
    accessMode: 'write',
    isolation: 'shared',
    status: 'running',
    createdAt: '2026-01-01T10:00:00.000Z',
    updatedAt: '2026-01-01T10:00:00.000Z',
  },
  steps: [
    { id: 'step-1', runId: 'run-1', profileId: 'a', task: 'a', accessMode: 'write', isolation: 'shared', status: 'completed', iteration: 1, createdAt: '2026-01-01T10:00:00.000Z', updatedAt: '2026-01-01T10:00:00.000Z' },
    { id: 'step-2', runId: 'run-1', profileId: 'b', task: 'b', accessMode: 'write', isolation: 'shared', status: 'running', iteration: 2, createdAt: '2026-01-01T10:01:00.000Z', updatedAt: '2026-01-01T10:01:00.000Z' },
  ],
  workers: [
    { id: 'w1', runId: 'run-1', stepId: 'step-1', iteration: 1, runtimeId: 'r', status: 'completed', startedAt: '2026-01-01T10:00:00.000Z' },
    { id: 'w2', runId: 'run-1', stepId: 'step-2', iteration: 2, runtimeId: 'r', status: 'running', startedAt: '2026-01-01T10:01:00.000Z' },
  ],
}

function event(overrides: Partial<RelayEvent> & { seq: number }): RelayEvent {
  return {
    id: `e${overrides.seq}`,
    runId: 'run-1',
    timestamp: '2026-01-01T10:00:00.000Z',
    type: 'tool/read',
    data: {},
    ...overrides,
  }
}

describe('eventsForStep', () => {
  it('keeps the step own workers and every run-level event', () => {
    const events = [
      event({ seq: 1, type: 'run/created' }),
      event({ seq: 2, stepId: 'step-1', workerSessionId: 'w1' }),
      event({ seq: 3, stepId: 'step-2', workerSessionId: 'w2' }),
      event({ seq: 4, type: 'run/awaiting_host' }),
    ]
    expect(eventsForStep(view, 'step-2', events).map((item) => item.seq)).toEqual([1, 3, 4])
    expect(eventsForStep(view, 'step-1', events).map((item) => item.seq)).toEqual([1, 2, 4])
  })

  it('attributes a worker-only event to the step that owns the worker', () => {
    const events = [event({ seq: 5, workerSessionId: 'w2' })]
    expect(eventsForStep(view, 'step-2', events)).toHaveLength(1)
    expect(eventsForStep(view, 'step-1', events)).toHaveLength(0)
  })
})
