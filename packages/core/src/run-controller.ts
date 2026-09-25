import { randomUUID } from 'node:crypto'
import type { AgentAdapter } from '@relay/adapter-sdk'
import {
  RelayError,
  agentProfileSchema,
  runRequestSchema,
  runtimeSchema,
  type AgentProfile,
  type RelayEvent,
  type RelayEventType,
  type Run,
  type RunRequest,
  type Runtime,
  type WorkerSession,
} from '@relay/protocol'
import type { EventStore } from './memory-event-store.js'

export interface ActiveRun {
  run: Run
  worker: WorkerSession
  completion: Promise<void>
}

export class RunController {
  readonly #adapters = new Map<string, AgentAdapter>()
  readonly #runtimes = new Map<string, Runtime>()
  readonly #profiles = new Map<string, AgentProfile>()

  constructor(private readonly eventStore: EventStore) {}

  registerAdapter(adapter: AgentAdapter): void {
    this.#adapters.set(adapter.id, adapter)
  }

  registerRuntime(input: Runtime): void {
    const runtime = runtimeSchema.parse(input)
    this.#runtimes.set(runtime.id, runtime)
  }

  registerProfile(input: AgentProfile): void {
    const profile = agentProfileSchema.parse(input)
    this.#profiles.set(profile.id, profile)
  }

  async start(input: RunRequest): Promise<ActiveRun> {
    const request = runRequestSchema.parse(input)
    const profile = this.#profiles.get(request.profileId)
    if (!profile) throw new RelayError('PROFILE_NOT_FOUND', `Unknown profile ${request.profileId}`)
    if (!profile.enabled) throw new RelayError('PROFILE_DISABLED', `Profile ${profile.id} is disabled`)
    if (request.accessMode === 'write' && !profile.capabilities.writeWorkspace) {
      throw new RelayError('CAPABILITY_DENIED', `Profile ${profile.id} cannot write to the workspace`)
    }

    const runtime = this.#runtimes.get(profile.runtimeId)
    if (!runtime) throw new RelayError('RUNTIME_NOT_FOUND', `Unknown runtime ${profile.runtimeId}`)
    if (runtime.health !== 'available') {
      throw new RelayError('RUNTIME_UNAVAILABLE', `Runtime ${runtime.id} is not available`)
    }
    const adapter = this.#adapters.get(runtime.adapterId)
    if (!adapter) throw new RelayError('RUNTIME_UNAVAILABLE', `No adapter for ${runtime.adapterId}`)

    const now = new Date().toISOString()
    const run: Run = {
      id: randomUUID(),
      ...request,
      status: 'queued',
      createdAt: now,
    }
    const worker: WorkerSession = {
      id: randomUUID(),
      runId: run.id,
      runtimeId: runtime.id,
      status: 'starting',
      startedAt: now,
    }
    let seq = 0
    const append = async (
      type: RelayEventType,
      data: unknown,
      nativeEvent?: unknown,
      includeWorker = true,
    ): Promise<void> => {
      seq += 1
      const event: RelayEvent = {
        id: randomUUID(),
        runId: run.id,
        ...(includeWorker ? { workerSessionId: worker.id } : {}),
        seq,
        timestamp: new Date().toISOString(),
        type,
        data,
        ...(nativeEvent === undefined ? {} : { nativeEvent }),
      }
      await this.eventStore.append(event)
    }

    await append('run/created', { run }, undefined, false)
    run.status = 'starting'

    const completion = (async () => {
      try {
        const handle = await adapter.start({
          runId: run.id,
          workerSessionId: worker.id,
          task: run.task,
          cwd: run.cwd,
          accessMode: run.accessMode,
          ...(profile.instructions ? { instructions: profile.instructions } : {}),
        })
        if (handle.nativeSessionId) worker.nativeSessionId = handle.nativeSessionId
        if (handle.processId) worker.processId = handle.processId
        worker.status = 'running'
        run.status = 'running'
        await append('worker/started', { worker })

        for await (const event of handle.events) {
          await append(event.type, event.data, event.nativeEvent)
          if (event.type === 'worker/completed') {
            worker.status = 'completed'
            run.status = 'completed'
          } else if (event.type === 'worker/failed') {
            worker.status = 'failed'
            run.status = 'failed'
          } else if (event.type === 'worker/cancelled') {
            worker.status = 'cancelled'
            run.status = 'cancelled'
          }
        }
      } catch (error) {
        worker.status = 'failed'
        run.status = 'failed'
        await append('worker/failed', {
          message: error instanceof Error ? error.message : String(error),
        })
      }
    })()

    return { run, worker, completion }
  }
}

