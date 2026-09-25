import { describe, expect, it } from 'vitest'
import { HostSessionRegistry } from '../src/index.js'

describe('HostSessionRegistry', () => {
  it('keeps the Codex display name and updates it without changing identity', () => {
    const sessions = new HostSessionRegistry()
    const created = sessions.upsertCodex({
      nativeSessionId: 'thread-1',
      displayName: 'Original Codex title',
      cwd: '/workspace',
      status: 'active',
    })
    const renamed = sessions.renameFromCodex('thread-1', 'Renamed in Codex')

    expect(renamed.id).toBe(created.id)
    expect(renamed.displayName).toBe('Renamed in Codex')
    expect(renamed.nameSource).toBe('codex')
  })
})
