import { describe, expect, it } from 'vitest'
import { PolicyResolver } from '../src/index.js'

describe('PolicyResolver', () => {
  it('uses session overrides before workspace and global settings', () => {
    const resolver = new PolicyResolver({
      maxConcurrentRuns: 2,
      maxConcurrentWriters: 1,
      requireWorktreeForParallelWriters: true,
      allowWrite: false,
      allowCommands: false,
      allowNetwork: false,
    })
    resolver.setWorkspace('/tmp/project', { allowWrite: true, maxConcurrentRuns: 3 })
    resolver.setSession('codex:session', { maxConcurrentRuns: 5, allowNetwork: true })

    expect(
      resolver.resolve({ workspace: '/tmp/project', hostSessionId: 'codex:session' }),
    ).toMatchObject({
      maxConcurrentRuns: 5,
      allowWrite: true,
      allowCommands: false,
      allowNetwork: true,
    })
  })

  it('removes temporary session policy at session end', () => {
    const resolver = new PolicyResolver()
    resolver.setSession('codex:session', { allowWrite: false })
    resolver.clearSession('codex:session')
    expect(resolver.resolve({ hostSessionId: 'codex:session' }).allowWrite).toBe(true)
  })
})

