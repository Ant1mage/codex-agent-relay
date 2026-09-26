import type {
  AdapterEvent,
  AgentAdapter,
  DetectionResult,
  WorkerSessionHandle,
} from '@relay/adapter-sdk'
import type { AdapterCapabilities, StartInput } from '@relay/protocol'

export class FakeAdapter implements AgentAdapter {
  readonly id = 'fake'
  disposed = false
  lastStartInput: StartInput | undefined

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
    this.lastStartInput = input
    async function* events(): AsyncGenerator<AdapterEvent> {
      yield { type: 'worker/message', data: { text: `Working on: ${input.task}` } }
      yield { type: 'tool/read', data: { path: 'README.md' } }
      yield { type: 'worker/completed', data: { summary: 'Fake task completed' } }
    }
    return { nativeSessionId: `fake:${input.workerSessionId}`, events: events() }
  }

  async cancel(): Promise<void> {}

  dispose(): void {
    this.disposed = true
  }
}
