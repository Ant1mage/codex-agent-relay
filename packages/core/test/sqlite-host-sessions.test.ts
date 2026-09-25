import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { SqliteHostSessionStore } from '../src/index.js'

describe('SqliteHostSessionStore', () => {
  it('shares Codex names with another process after reopening', () => {
    const directory = mkdtempSync(join(tmpdir(), 'relay-sessions-'))
    const path = join(directory, 'relay.sqlite')
    const writer = new SqliteHostSessionStore(path)
    writer.upsertCodex({
      nativeSessionId: 'thread-1',
      displayName: 'Exact Codex name',
      cwd: '/workspace',
      status: 'active',
    })
    writer.close()

    const reader = new SqliteHostSessionStore(path)
    expect(reader.get('codex:thread-1')?.displayName).toBe('Exact Codex name')
    reader.close()
    rmSync(directory, { recursive: true, force: true })
  })
})
