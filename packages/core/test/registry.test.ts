import { describe, expect, it } from 'vitest'
import type { AgentProfile } from '@relay/protocol'
import { AdapterRegistry, ProfileRegistry } from '../src/index.js'
import { FakeAdapter } from './fake-adapter.js'

describe('AdapterRegistry', () => {
  it('unregisters and disposes an adapter through one reversible handle', async () => {
    const registry = new AdapterRegistry()
    const adapter = new FakeAdapter()
    const registration = registry.register(adapter)
    expect(registry.get(adapter.id)).toBe(adapter)

    await registration.dispose()
    expect(registry.get(adapter.id)).toBeUndefined()
    expect(adapter.disposed).toBe(true)
  })

  it('does not allow a registration to shadow an existing provider', () => {
    const registry = new AdapterRegistry()
    registry.register(new FakeAdapter())
    expect(() => registry.register(new FakeAdapter())).toThrow(/already registered/)
  })
})

describe('ProfileRegistry.sync', () => {
  const profile = (id: string, name: string, enabled = true): AgentProfile => ({
    id,
    name,
    runtimeId: 'runtime:test',
    description: 'from the configuration file',
    capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: false },
    enabled,
  })

  it('adds, updates and removes in one pass, so edits apply without a restart', () => {
    const registry = new ProfileRegistry()
    registry.register(profile('a', 'A'))
    registry.register(profile('b', 'B'))

    const result = registry.sync([profile('a', 'A renamed'), profile('c', 'C'), profile('d', 'D', false)])
    expect(result).toEqual({ added: ['c', 'd'], updated: ['a'], removed: ['b'] })
    expect(registry.list().map((item) => item.id)).toEqual(['a', 'c', 'd'])
    expect(registry.list({ enabledOnly: true }).map((item) => item.id)).toEqual(['a', 'c'])
  })

  it('is a no-op when nothing changed', () => {
    const registry = new ProfileRegistry()
    registry.register(profile('a', 'A'))
    expect(registry.sync([profile('a', 'A')])).toEqual({ added: [], updated: [], removed: [] })
  })
})
