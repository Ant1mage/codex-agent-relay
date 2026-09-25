import type { AgentAdapter, Disposable } from '@relay/adapter-sdk'
import {
  RelayError,
  agentProfileSchema,
  runtimeSchema,
  type AgentProfile,
  type Runtime,
} from '@relay/protocol'

function registration(release: () => void | Promise<void>): Disposable {
  let active = true
  return {
    async dispose() {
      if (!active) return
      active = false
      await release()
    },
  }
}

export class AdapterRegistry {
  readonly #items = new Map<string, AgentAdapter>()

  register(adapter: AgentAdapter): Disposable {
    if (this.#items.has(adapter.id)) throw new Error(`Adapter ${adapter.id} is already registered`)
    this.#items.set(adapter.id, adapter)
    return registration(async () => {
      this.#items.delete(adapter.id)
      await adapter.dispose()
    })
  }

  get(id: string): AgentAdapter | undefined {
    return this.#items.get(id)
  }

  list(): AgentAdapter[] {
    return [...this.#items.values()]
  }
}

export class RuntimeRegistry {
  readonly #items = new Map<string, Runtime>()

  register(input: Runtime): Disposable {
    const runtime = runtimeSchema.parse(input)
    if (this.#items.has(runtime.id)) throw new Error(`Runtime ${runtime.id} is already registered`)
    this.#items.set(runtime.id, runtime)
    return registration(() => {
      this.#items.delete(runtime.id)
    })
  }

  require(id: string): Runtime {
    const runtime = this.#items.get(id)
    if (!runtime) throw new RelayError('RUNTIME_NOT_FOUND', `Unknown runtime ${id}`)
    return runtime
  }

  list(): Runtime[] {
    return [...this.#items.values()]
  }
}

export class ProfileRegistry {
  readonly #items = new Map<string, AgentProfile>()

  register(input: AgentProfile): Disposable {
    const profile = agentProfileSchema.parse(input)
    if (this.#items.has(profile.id)) throw new Error(`Profile ${profile.id} is already registered`)
    this.#items.set(profile.id, profile)
    return registration(() => {
      this.#items.delete(profile.id)
    })
  }

  require(id: string): AgentProfile {
    const profile = this.#items.get(id)
    if (!profile) throw new RelayError('PROFILE_NOT_FOUND', `Unknown profile ${id}`)
    return profile
  }

  list(options: { enabledOnly?: boolean } = {}): AgentProfile[] {
    return [...this.#items.values()].filter((profile) => !options.enabledOnly || profile.enabled)
  }
}
