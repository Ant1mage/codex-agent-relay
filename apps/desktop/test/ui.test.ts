import { describe, expect, it } from 'vitest'
import type { RelayEvent } from '@relay/protocol'
import { consoleRows, eventKind, statusGlyph } from '../src/renderer/src/ui'

function event(
  type: RelayEvent['type'],
  id: string,
  data: unknown = {},
  timestamp = '2026-01-01T10:00:00.000Z',
): RelayEvent {
  return { id, runId: 'run-1', seq: Number(id.replace(/\D/g, '')) || 1, timestamp, type, data }
}

describe('statusGlyph', () => {
  it('maps statuses to the glyphs in docs/ui.md 8', () => {
    expect(statusGlyph('completed')).toBe('✓')
    expect(statusGlyph('running')).toBe('●')
    expect(statusGlyph('starting')).toBe('●')
    expect(statusGlyph('queued')).toBe('○')
    expect(statusGlyph('failed')).toBe('✕')
  })
})

describe('eventKind', () => {
  it('maps observable tool activity to Console categories', () => {
    expect(eventKind(event('tool/read', 'e1'))).toBe('read')
    expect(eventKind(event('tool/search', 'e2'))).toBe('search')
    expect(eventKind(event('tool/edit', 'e3'))).toBe('edit')
    expect(eventKind(event('tool/command', 'e4'))).toBe('command')
    expect(eventKind(event('test/result', 'e5'))).toBe('test')
    expect(eventKind(event('tool/result', 'e6'))).toBe('result')
    expect(eventKind(event('worker/failed', 'e7'))).toBe('error')
  })

  it('never surfaces hidden reasoning or runtime-internal child agents', () => {
    // docs/ui.md 12.4 and 23: no hidden chain-of-thought, no child-agent tree.
    expect(eventKind(event('worker/reasoning', 'e8'))).toBeUndefined()
    expect(eventKind(event('child/started', 'e9'))).toBeUndefined()
    expect(eventKind(event('child/completed', 'e10'))).toBeUndefined()
  })

  it('hides relay lifecycle bookkeeping', () => {
    expect(eventKind(event('run/created', 'e11'))).toBeUndefined()
    expect(eventKind(event('step/created', 'e12'))).toBeUndefined()
    expect(eventKind(event('step/iteration_started', 'e13'))).toBeUndefined()
  })
})

describe('consoleRows', () => {
  it('keeps distinct categories as separate rows', () => {
    const rows = consoleRows([
      event('tool/read', 'e1', { path: 'src/a.ts' }),
      event('tool/edit', 'e2', { path: 'src/b.ts' }),
      event('tool/command', 'e3', { command: 'pnpm test' }),
    ])
    expect(rows.map((row) => row.kind)).toEqual(['read', 'edit', 'command'])
    expect(rows[1]?.label).toBe('src/b.ts')
  })

  it('skips events Console must not render', () => {
    const rows = consoleRows([
      event('run/created', 'e1'),
      event('worker/reasoning', 'e2'),
      event('child/started', 'e3'),
      event('tool/read', 'e4', { path: 'src/a.ts' }),
    ])
    expect(rows).toHaveLength(1)
    expect(rows[0]?.kind).toBe('read')
  })

  it('groups three or more consecutive low-value reads (docs/ui.md 12.3)', () => {
    const reads = [1, 2, 3, 4].map((n) => event('tool/read', `e${n}`, { path: `src/f${n}.ts` }))
    const rows = consoleRows(reads)
    expect(rows).toHaveLength(1)
    expect(rows[0]?.count).toBe(4)
    expect(rows[0]?.kind).toBe('read')
  })

  it('does not group fewer than three, and never groups across categories', () => {
    const two = [1, 2].map((n) => event('tool/read', `e${n}`, { path: `src/f${n}.ts` }))
    expect(consoleRows(two)).toHaveLength(2)

    const split = [
      event('tool/read', 'e1', { path: 'a' }),
      event('tool/read', 'e2', { path: 'b' }),
      event('tool/edit', 'e3', { path: 'c' }),
      event('tool/read', 'e4', { path: 'd' }),
      event('tool/read', 'e5', { path: 'e' }),
    ]
    const rows = consoleRows(split)
    expect(rows.map((row) => row.kind)).toEqual(['read', 'read', 'edit', 'read', 'read'])
    expect(rows.every((row) => row.count === undefined)).toBe(true)
  })

  it('carries diff stats through for edits (docs/ui.md 12.2 "+42 -12")', () => {
    const rows = consoleRows([
      event('tool/edit', 'e1', { path: 'src/kimi.ts', diff: { additions: 42, deletions: 12 } }),
    ])
    expect(rows[0]?.additions).toBe(42)
    expect(rows[0]?.deletions).toBe(12)
  })

  it('lists every file when a read event reports an array', () => {
    const rows = consoleRows([event('tool/read', 'e1', { files: ['a.ts', 'b.ts', 'c.ts'] })])
    expect(rows[0]?.label).toBe('3')
  })
})
