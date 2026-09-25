import { describe, expect, it } from 'vitest'
import type { CodexThreadMetadataResolver } from '@relay/integration-codex'
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
import { HostSessionRegistry, MemoryEventStore, RunController } from '@relay/core'
import { RelayService } from '../src/index.js'

class CompletedAdapter implements AgentAdapter {
  readonly id = 'completed'
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
  async start(_input: StartInput): Promise<WorkerSessionHandle> {
    async function* events(): AsyncGenerator<AdapterEvent> {
      yield { type: 'worker/completed', data: { summary: 'done' } }
    }
    return { nativeSessionId: 'native-worker', events: events() }
  }
  async cancel(): Promise<void> {}
  dispose(): void {}
}

describe('RelayService', () => {
  it('binds every run to the exact Codex session name and cwd', async () => {
    const controller = new RunController(new MemoryEventStore())
    const adapter = new CompletedAdapter()
    const runtime: Runtime = {
      id: 'runtime:completed',
      adapterId: adapter.id,
      executablePath: '/usr/bin/true',
      health: 'available',
      capabilities: adapter.capabilities(),
    }
    const profile: AgentProfile = {
      id: 'completed-agent',
      name: 'Completed agent',
      runtimeId: runtime.id,
      description: 'Completes immediately',
      capabilities: {
        readWorkspace: true,
        writeWorkspace: false,
        executeCommands: false,
        networkAccess: false,
      },
      enabled: true,
    }
    controller.registerAdapter(adapter)
    controller.registerRuntime(runtime)
    controller.registerProfile(profile)
    const resolver: CodexThreadMetadataResolver = {
      async resolve(threadId) {
        return {
          id: threadId,
          displayName: 'Name shown by Codex',
          cwd: '/codex/workspace',
          model: 'gpt-test',
        }
      },
    }
    const sessions = new HostSessionRegistry()
    const service = new RelayService(controller, sessions, resolver)

    const started = await service.runAgent(
      { threadId: 'thread-1' },
      { agentId: profile.id, task: 'bounded task' },
    )
    const projected = await service.wait(started.workerSessionId)

    expect(started.hostSessionDisplayName).toBe('Name shown by Codex')
    expect(projected.run.hostSessionId).toBe('codex:thread-1')
    expect(projected.run.cwd).toBe('/codex/workspace')
    expect(sessions.get('codex:thread-1')?.displayName).toBe('Name shown by Codex')
  })
})

