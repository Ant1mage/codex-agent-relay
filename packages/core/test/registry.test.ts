import { describe, expect, it } from 'vitest'
import { AdapterRegistry } from '../src/index.js'
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
