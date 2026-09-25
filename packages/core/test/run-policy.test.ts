import { describe, expect, it } from 'vitest'
import type {
  AdapterCapabilities,
  AgentProfile,
  Runtime,
  StartInput,
} from '@relay/protocol'
import type {
  AdapterEvent,
  AgentAdapter,
  DetectionResult,
  WorkerSessionHandle,
} from '@relay/adapter-sdk'
import { MemoryEventStore, PolicyResolver, RunController } from '../src/index.js'

class ControlledAdapter implements AgentAdapter {
  readonly id = 'controlled'
  readonly #releases: Array<() => void> = []

  capabilities(): AdapterCapabilities {
    return {
      nonInteractive: true,
      structuredEvents: true,
      cwd: true,
      resume: false,
      send: false,
      cancel: true,
      childSessions: false,
    }
  }

  async detect(): Promise<DetectionResult> {
    return { runtimes: [], diagnostics: [] }
  }

  async start(input: StartInput): Promise<WorkerSessionHandle> {
    let release!: () => void
    const stopped = new Promise<void>((resolve) => {
      release = resolve
    })
    this.#releases.push(release)
    async function* events(): AsyncGenerator<AdapterEvent> {
      yield { type: 'worker/message', data: { task: input.task } }
      await stopped
    }
    return { nativeSessionId: input.workerSessionId, events: events() }
  }

  async cancel(): Promise<void> {
    this.#releases.shift()?.()
  }

  dispose(): void {}
}

const runtime: Runtime = {
  id: 'runtime:controlled',
  adapterId: 'controlled',
  executablePath: '/usr/bin/true',
  health: 'available',
  capabilities: new ControlledAdapter().capabilities(),
}

const profile: AgentProfile = {
  id: 'profile:controlled',
  name: 'Controlled writer',
  runtimeId: runtime.id,
  description: 'Waits until cancelled',
  capabilities: {
    readWorkspace: true,
    writeWorkspace: true,
    executeCommands: false,
    networkAccess: false,
  },
  enabled: true,
}

function setup(): { controller: RunController; store: MemoryEventStore } {
  const store = new MemoryEventStore()
  const policies = new PolicyResolver({
    maxConcurrentRuns: 4,
    maxConcurrentWriters: 2,
    requireWorktreeForParallelWriters: true,
    allowWrite: true,
    allowCommands: true,
    allowNetwork: true,
  })
  const controller = new RunController(store, { policies })
  controller.registerAdapter(new ControlledAdapter())
  controller.registerRuntime(runtime)
  controller.registerProfile(profile)
  return { controller, store }
}

describe('run policy and cancellation', () => {
  it('rejects two shared writers in the same workspace', async () => {
    const { controller } = setup()
    const first = await controller.start({
      hostSessionId: 'codex:one',
      profileId: profile.id,
      task: 'First writer',
      cwd: '/tmp/relay-project',
      accessMode: 'write',
      isolation: 'shared',
    })

    await expect(
      controller.start({
        hostSessionId: 'codex:two',
        profileId: profile.id,
        task: 'Second writer',
        cwd: '/tmp/relay-project',
        accessMode: 'write',
        isolation: 'shared',
      }),
    ).rejects.toMatchObject({ code: 'WORKSPACE_CONFLICT' })

    await controller.cancel(first.run.id)
    await first.completion
  })

  it('records cancellation as the terminal event', async () => {
    const { controller, store } = setup()
    const active = await controller.start({
      hostSessionId: 'codex:cancel',
      profileId: profile.id,
      task: 'Wait for cancellation',
      cwd: '/tmp/relay-cancel',
      accessMode: 'write',
      isolation: 'shared',
    })
    await controller.cancel(active.run.id)
    await active.completion

    expect(active.run.status).toBe('cancelled')
    expect(store.list(active.run.id).at(-1)?.type).toBe('worker/cancelled')
    expect(controller.listActive()).toEqual([])
  })
})
