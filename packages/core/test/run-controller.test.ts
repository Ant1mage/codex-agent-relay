import { describe, expect, it } from 'vitest'
import { exerciseAdapter } from '@relay/adapter-sdk'
import type { AgentProfile, Runtime } from '@relay/protocol'
import { MemoryEventStore, RunController } from '../src/index.js'
import { FakeAdapter } from './fake-adapter.js'

const runtime: Runtime = {
  id: 'runtime:fake',
  adapterId: 'fake',
  executablePath: '/usr/bin/true',
  version: '1.0.0',
  health: 'available',
  capabilities: {
    nonInteractive: true,
    structuredEvents: true,
    cwd: true,
    resume: false,
    send: false,
    cancel: true,
    childSessions: false,
  },
}

const profile: AgentProfile = {
  id: 'profile:fake-code',
  name: 'Fake Code',
  runtimeId: runtime.id,
  description: 'Exercises the provider-independent run path.',
  capabilities: {
    readWorkspace: true,
    writeWorkspace: true,
    executeCommands: true,
    networkAccess: false,
  },
  enabled: true,
}

describe('phase 0 contracts', () => {
  it('runs an in-memory task through a fake adapter', async () => {
    const store = new MemoryEventStore()
    const controller = new RunController(store)
    controller.registerAdapter(new FakeAdapter())
    controller.registerRuntime(runtime)
    controller.registerProfile(profile)

    const active = await controller.start({
      hostSessionId: 'codex:test-session',
      profileId: profile.id,
      task: 'Prove the vertical slice',
      cwd: process.cwd(),
      accessMode: 'write',
      isolation: 'shared',
    })
    await active.completion

    expect(active.run.status).toBe('awaiting_host')
    expect(active.step.status).toBe('awaiting_host')
    expect(active.worker.status).toBe('completed')
    const events = store.list(active.run.id)
    expect(events.map((event) => event.type)).toEqual([
      'run/created',
      'step/created',
      'worker/started',
      'worker/message',
      'tool/read',
      'worker/completed',
      'run/awaiting_host',
    ])
    expect(events.map((event) => event.seq)).toEqual([1, 2, 3, 4, 5, 6, 7])

    const accepted = await controller.accept(active.run.id)
    expect(accepted.run.status).toBe('completed')
    expect(accepted.steps[0]?.status).toBe('completed')
    expect(store.list(active.run.id).at(-1)?.type).toBe('run/accepted')
  })

  it('provides a reusable adapter conformance exercise', async () => {
    const report = await exerciseAdapter(new FakeAdapter(), {
      runId: 'run:test',
      workerSessionId: 'worker:test',
      task: 'Conformance check',
      cwd: process.cwd(),
      accessMode: 'read_only',
    })

    expect(report).toMatchObject({
      adapterId: 'fake',
      eventCount: 3,
      terminalEvent: 'worker/completed',
    })
  })
})
