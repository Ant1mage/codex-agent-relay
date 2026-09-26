import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { RelayConfigStore } from '../src/index.js'
import type { AgentProfile } from '@relay/protocol'

let directory: string
let store: RelayConfigStore

const profile: AgentProfile = {
  id: 'agent-1',
  name: 'Test Agent',
  runtimeId: 'runtime:deepseek-harness',
  description: 'created by a test',
  capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: false },
  enabled: true,
}

beforeEach(() => {
  directory = mkdtempSync(join(tmpdir(), 'relay-config-'))
  store = new RelayConfigStore({
    profiles: join(directory, 'profiles.json'),
    settings: join(directory, 'settings.json'),
  })
})

afterEach(() => rmSync(directory, { recursive: true, force: true }))

describe('RelayConfigStore', () => {
  it('falls back to the default policy until a settings file exists', () => {
    const config = store.read()
    expect(config.policy.maxConcurrentRuns).toBe(4)
    expect(config.policy.requireWorktreeForParallelWriters).toBe(true)
    expect(config.workspaceOverrides).toEqual({})
  })

  it('creates, updates and removes a profile on disk', () => {
    expect(store.upsertProfile(profile).profiles.map((item) => item.id)).toEqual(['agent-1'])
    const updated = store.upsertProfile({ ...profile, name: 'Renamed', enabled: false })
    expect(updated.profiles).toHaveLength(1)
    expect(updated.profiles[0]?.name).toBe('Renamed')
    expect(updated.profiles[0]?.enabled).toBe(false)
    expect(JSON.parse(readFileSync(store.paths.profiles, 'utf8'))).toHaveLength(1)
    expect(store.removeProfile('agent-1').profiles).toEqual([])
  })

  it('writes policy and workspace overrides that the MCP process can read', () => {
    const config = store.writeSettings({
      policy: { ...store.read().policy, allowNetwork: false, maxConcurrentWriters: 2 },
      workspaceOverrides: { '/tmp/project': { allowWrite: false } },
    })
    expect(config.policy.allowNetwork).toBe(false)
    expect(config.workspaceOverrides['/tmp/project']).toEqual({ allowWrite: false })
    const reread = new RelayConfigStore(store.paths).read()
    expect(reread.policy.maxConcurrentWriters).toBe(2)
    expect(reread.workspaceOverrides['/tmp/project']).toEqual({ allowWrite: false })
  })

  it('changes its revision when either file changes', () => {
    const before = store.read().revision
    store.upsertProfile(profile)
    const afterProfile = store.read().revision
    expect(afterProfile).not.toBe(before)
    store.writeSettings({ policy: store.read().policy, workspaceOverrides: {} })
    expect(store.read().revision).not.toBe(afterProfile)
  })

  it('survives a hand-edited file instead of taking the worker down', () => {
    writeFileSync(store.paths.profiles, '{ not json', 'utf8')
    expect(store.read().profiles).toEqual([])
    writeFileSync(store.paths.profiles, JSON.stringify([{ id: 'broken' }]), 'utf8')
    expect(store.read().profiles).toEqual([])
  })
})
