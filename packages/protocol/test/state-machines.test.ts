import { describe, expect, it } from 'vitest'
import { canTransitionRun, canTransitionWorker } from '../src/index.js'

describe('state machines', () => {
  it('allows lifecycle progress and rejects terminal transitions', () => {
    expect(canTransitionRun('queued', 'starting')).toBe(true)
    expect(canTransitionRun('running', 'completed')).toBe(true)
    expect(canTransitionRun('completed', 'running')).toBe(false)
    expect(canTransitionWorker('starting', 'running')).toBe(true)
    expect(canTransitionWorker('failed', 'running')).toBe(false)
  })
})
