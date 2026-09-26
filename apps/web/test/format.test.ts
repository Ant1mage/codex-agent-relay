import { describe, expect, it } from 'vitest'
import type { RelayEvent } from '@relay/protocol'
import { changedFiles, consoleRows, eventKind, relativeTime, statusGlyph, visibleEvents } from '../src/lib/format.js'

function event(type: RelayEvent['type'], id: string, data: unknown = {}, timestamp = '2026-01-01T10:00:00.000Z'): RelayEvent {
  return { id, runId: 'run-1', seq: Number(id.replace(/\D/g, '')) || 1, timestamp, type, data }
}

const t = ((key: string) => key) as never

describe('statusGlyph', () => {
  it('maps statuses to glyphs', () => {
    expect(statusGlyph('completed')).toBe('✓')
    expect(statusGlyph('running')).toBe('●')
    expect(statusGlyph('awaiting_host')).toBe('◆')
    expect(statusGlyph('failed')).toBe('✕')
    expect(statusGlyph('queued')).toBe('○')
  })
})

describe('eventKind', () => {
  it('maps observable activity to console categories', () => {
    expect(eventKind(event('tool/read', 'e1'))).toBe('read')
    expect(eventKind(event('tool/edit', 'e2'))).toBe('edit')
    expect(eventKind(event('tool/command', 'e3'))).toBe('command')
    expect(eventKind(event('test/result', 'e4'))).toBe('test')
    expect(eventKind(event('worker/failed', 'e5'))).toBe('error')
  })

  it('hides reasoning, child agents and lifecycle bookkeeping', () => {
    expect(eventKind(event('worker/reasoning', 'e6'))).toBeUndefined()
    expect(eventKind(event('child/started', 'e7'))).toBeUndefined()
    expect(eventKind(event('run/created', 'e8'))).toBeUndefined()
    expect(eventKind(event('step/created', 'e9'))).toBeUndefined()
  })
})

describe('consoleRows', () => {
  it('groups three or more consecutive reads', () => {
    const rows = consoleRows([1, 2, 3, 4].map((n) => event('tool/read', `e${n}`, { path: `src/f${n}.ts` })))
    expect(rows).toHaveLength(1)
    expect(rows[0]?.count).toBe(4)
  })

  it('never groups fewer than three, nor across categories', () => {
    const two = [1, 2].map((n) => event('tool/read', `e${n}`))
    expect(consoleRows(two)).toHaveLength(2)
    const split = [
      event('tool/read', 'e1'),
      event('tool/read', 'e2'),
      event('tool/edit', 'e3'),
      event('tool/read', 'e4'),
      event('tool/read', 'e5'),
    ]
    expect(consoleRows(split).map((row) => row.kind)).toEqual(['read', 'read', 'edit', 'read', 'read'])
  })

  it('keeps diff stats on edits', () => {
    const rows = consoleRows([event('tool/edit', 'e1', { path: 'src/kimi.ts', diff: { additions: 42, deletions: 12 } })])
    expect(rows[0]).toMatchObject({ additions: 42, deletions: 12, label: 'src/kimi.ts' })
  })
})

describe('inspectors', () => {
  it('lists only observable events for the raw view', () => {
    const events = [event('run/created', 'e1'), event('worker/reasoning', 'e2'), event('tool/read', 'e3')]
    expect(visibleEvents(events).map((item) => item.id)).toEqual(['e3'])
  })

  it('aggregates changed files across edits', () => {
    const files = changedFiles([
      event('tool/edit', 'e1', { path: 'src/a.ts', diff: { additions: 2, deletions: 1 } }),
      event('tool/edit', 'e2', { path: 'src/a.ts', diff: { additions: 3, deletions: 0 } }),
      event('tool/edit', 'e3', { file: 'src/b.ts' }),
    ])
    expect(files).toEqual([
      { path: 'src/a.ts', additions: 5, deletions: 1 },
      { path: 'src/b.ts', additions: 0, deletions: 0 },
    ])
  })
})

describe('relativeTime', () => {
  it('says active while something is still running', () => {
    expect(relativeTime('2026-01-01T10:00:00.000Z', 'running', t)).toBe('common.active')
  })

  it('falls back to a date for old timestamps', () => {
    expect(relativeTime('2020-01-01T10:00:00.000Z', 'completed', t)).toMatch(/2020|Jan/)
  })
})
