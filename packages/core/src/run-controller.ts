import { randomUUID } from 'node:crypto'
import { resolve } from 'node:path'
import type { AgentAdapter, Disposable, WorkerSessionHandle } from '@relay/adapter-sdk'
import {
  RelayError,
  runRequestSchema,
  type AgentProfile,
  type RelayEvent,
  type RelayEventType,
  type Run,
  type RunRequest,
  type Runtime,
  type Step,
  type WorkerSession,
} from '@relay/protocol'
import type { EventStore } from './memory-event-store.js'
import { assertPolicyAllows, PolicyResolver } from './policy.js'
import { projectRun, type RunProjection } from './projection.js'
import { AdapterRegistry, ProfileRegistry, RuntimeRegistry } from './registry.js'

export interface ActiveRun {
  run: Run
  step: Step
  worker: WorkerSession
  completion: Promise<void>
}

interface InternalRun {
  run: Run
  step: Step
  worker: WorkerSession
  adapter: AgentAdapter
  handle?: WorkerSessionHandle
  cancelRequested: boolean
  terminal: boolean
  append(
    type: RelayEventType,
    data: unknown,
    nativeEvent?: unknown,
    includeWorker?: boolean,
    includeStep?: boolean,
  ): Promise<void>
}

export interface RunControllerOptions {
  adapters?: AdapterRegistry
  runtimes?: RuntimeRegistry
  profiles?: ProfileRegistry
  policies?: PolicyResolver
}

export class RunController {
  readonly adapters: AdapterRegistry
  readonly runtimes: RuntimeRegistry
  readonly profiles: ProfileRegistry
  readonly policies: PolicyResolver
  readonly #active = new Map<string, InternalRun>()

  constructor(
    private readonly eventStore: EventStore,
    options: RunControllerOptions = {},
  ) {
    this.adapters = options.adapters ?? new AdapterRegistry()
    this.runtimes = options.runtimes ?? new RuntimeRegistry()
    this.profiles = options.profiles ?? new ProfileRegistry()
    this.policies = options.policies ?? new PolicyResolver()
  }

  registerAdapter(adapter: AgentAdapter): Disposable {
    return this.adapters.register(adapter)
  }

  registerRuntime(runtime: Runtime): Disposable {
    return this.runtimes.register(runtime)
  }

  registerProfile(profile: AgentProfile): Disposable {
    return this.profiles.register(profile)
  }

  listActive(): Array<{ run: Run; worker: WorkerSession }> {
    return [...this.#active.values()].map(({ run, worker }) => ({ run, worker }))
  }

  async get(runId: string): Promise<RunProjection> {
    const events = await this.eventStore.list(runId)
    if (events.length === 0) throw new RelayError('RUN_NOT_FOUND', `Unknown run ${runId}`)
    return projectRun(events)
  }

  async wait(runId: string): Promise<RunProjection> {
    const active = this.#active.get(runId)
    if (active) {
      while (this.#active.has(runId)) await new Promise((resolve) => setTimeout(resolve, 10))
    }
    return this.get(runId)
  }

  async getByWorker(workerSessionId: string): Promise<RunProjection> {
    const runId = await this.eventStore.findRunIdByWorker(workerSessionId)
    if (!runId) throw new RelayError('WORKER_NOT_FOUND', `Unknown worker ${workerSessionId}`)
    return this.get(runId)
  }

  async waitForWorker(workerSessionId: string): Promise<RunProjection> {
    const runId = await this.eventStore.findRunIdByWorker(workerSessionId)
    if (!runId) throw new RelayError('WORKER_NOT_FOUND', `Unknown worker ${workerSessionId}`)
    return this.wait(runId)
  }

  async send(workerSessionId: string, message: string): Promise<void> {
    const active = [...this.#active.values()].find(
      (candidate) => candidate.worker.id === workerSessionId,
    )
    if (!active) throw new RelayError('WORKER_NOT_FOUND', `Worker ${workerSessionId} is not active`)
    if (!active.adapter.send) {
      throw new RelayError('OPERATION_UNSUPPORTED', `Adapter ${active.adapter.id} does not support send`)
    }
    const nativeSessionId = active.handle?.nativeSessionId
    if (!nativeSessionId) throw new RelayError('WORKER_NOT_FOUND', `Worker ${workerSessionId} is starting`)
    await active.adapter.send(nativeSessionId, message)
  }

  async cancelWorker(workerSessionId: string): Promise<void> {
    const active = [...this.#active.values()].find(
      (candidate) => candidate.worker.id === workerSessionId,
    )
    if (!active) throw new RelayError('WORKER_NOT_FOUND', `Worker ${workerSessionId} is not active`)
    await this.cancel(active.run.id)
  }

  async accept(runId: string): Promise<RunProjection> {
    const events = await this.eventStore.list(runId)
    if (events.length === 0) throw new RelayError('RUN_NOT_FOUND', `Unknown run ${runId}`)
    const projection = projectRun(events)
    if (projection.run.status !== 'awaiting_host') {
      throw new RelayError(
        'INVALID_STATE',
        `Run ${runId} cannot be accepted from ${projection.run.status}`,
      )
    }
    const timestamp = new Date().toISOString()
    await this.eventStore.append({
      id: randomUUID(),
      runId,
      seq: events.length + 1,
      timestamp,
      type: 'run/accepted',
      data: { acceptedBy: 'host' },
    })
    return this.get(runId)
  }

  async acceptWorker(workerSessionId: string): Promise<RunProjection> {
    const runId = await this.eventStore.findRunIdByWorker(workerSessionId)
    if (!runId) throw new RelayError('WORKER_NOT_FOUND', `Unknown worker ${workerSessionId}`)
    return this.accept(runId)
  }

  async start(input: RunRequest): Promise<ActiveRun> {
    const request = runRequestSchema.parse(input)
    const profile = this.profiles.require(request.profileId)
    if (!profile.enabled) throw new RelayError('PROFILE_DISABLED', `Profile ${profile.id} is disabled`)
    if (request.accessMode === 'write' && !profile.capabilities.writeWorkspace) {
      throw new RelayError('CAPABILITY_DENIED', `Profile ${profile.id} cannot write to the workspace`)
    }

    const runtime = this.runtimes.require(profile.runtimeId)
    if (runtime.health !== 'available') {
      throw new RelayError('RUNTIME_UNAVAILABLE', `Runtime ${runtime.id} is not available`)
    }
    const adapter = this.adapters.get(runtime.adapterId)
    if (!adapter) throw new RelayError('RUNTIME_UNAVAILABLE', `No adapter for ${runtime.adapterId}`)

    const policy = this.policies.resolve({
      workspace: request.cwd,
      hostSessionId: request.hostSessionId,
    })
    assertPolicyAllows(policy, request, profile)
    this.#assertConcurrency(request, policy.maxConcurrentRuns, policy.maxConcurrentWriters)
    this.#assertWorkspaceIsolation(request, policy.requireWorktreeForParallelWriters)

    const now = new Date().toISOString()
    const run: Run = {
      id: randomUUID(),
      ...request,
      status: 'queued',
      createdAt: now,
      updatedAt: now,
    }
    const step: Step = {
      id: randomUUID(),
      runId: run.id,
      profileId: request.profileId,
      task: request.task,
      accessMode: request.accessMode,
      isolation: request.isolation,
      status: 'queued',
      iteration: 1,
      createdAt: now,
      updatedAt: now,
    }
    const worker: WorkerSession = {
      id: randomUUID(),
      runId: run.id,
      stepId: step.id,
      iteration: step.iteration,
      runtimeId: runtime.id,
      status: 'starting',
      startedAt: now,
    }
    let seq = 0
    let writeChain = Promise.resolve()
    const append: InternalRun['append'] = (
      type,
      data,
      nativeEvent,
      includeWorker = true,
      includeStep = true,
    ) => {
      seq += 1
      const event: RelayEvent = {
        id: randomUUID(),
        runId: run.id,
        ...(includeStep ? { stepId: step.id } : {}),
        ...(includeWorker ? { workerSessionId: worker.id } : {}),
        seq,
        timestamp: new Date().toISOString(),
        type,
        data,
        ...(nativeEvent === undefined ? {} : { nativeEvent: boundNativeEvent(nativeEvent) }),
      }
      writeChain = writeChain.then(async () => this.eventStore.append(event))
      return writeChain
    }

    await append('run/created', { run }, undefined, false, false)
    await append('step/created', { step }, undefined, false)
    run.status = 'starting'
    run.updatedAt = new Date().toISOString()
    step.status = 'starting'
    step.updatedAt = run.updatedAt
    const internal: InternalRun = {
      run,
      step,
      worker,
      adapter,
      cancelRequested: false,
      terminal: false,
      append,
    }
    this.#active.set(run.id, internal)

    const completion = this.#execute(internal, profile).finally(() => {
      this.#active.delete(run.id)
    })
    return { run, step, worker, completion }
  }

  async cancel(runId: string): Promise<void> {
    const active = this.#active.get(runId)
    if (!active) throw new RelayError('RUN_NOT_FOUND', `Run ${runId} is not active`)
    if (active.terminal || active.cancelRequested) return
    active.cancelRequested = true

    try {
      const nativeSessionId = active.handle?.nativeSessionId ?? active.worker.id
      await active.adapter.cancel(nativeSessionId)
      await this.#finish(active, 'worker/cancelled', { requestedBy: 'host' })
    } catch (error) {
      await this.#finish(active, 'worker/failed', {
        message: error instanceof Error ? error.message : String(error),
        operation: 'cancel',
      })
      throw new RelayError('ADAPTER_FAILURE', `Adapter failed to cancel run ${runId}`, error)
    }
  }

  async #execute(active: InternalRun, profile: AgentProfile): Promise<void> {
    const { adapter, run, step, worker } = active
    try {
      const handle = await adapter.start({
        runId: run.id,
        workerSessionId: worker.id,
        task: run.task,
        cwd: run.cwd,
        accessMode: run.accessMode,
        ...(profile.instructions ? { instructions: profile.instructions } : {}),
      })
      active.handle = handle
      if (handle.nativeSessionId) worker.nativeSessionId = handle.nativeSessionId
      if (handle.processId) worker.processId = handle.processId

      if (active.cancelRequested) {
        await adapter.cancel(handle.nativeSessionId ?? worker.id)
        await this.#finish(active, 'worker/cancelled', { requestedBy: 'host' })
        return
      }

      worker.status = 'running'
      run.status = 'running'
      run.updatedAt = new Date().toISOString()
      step.status = 'running'
      step.updatedAt = run.updatedAt
      await active.append('worker/started', { worker })

      for await (const event of handle.events) {
        if (active.terminal) break
        if (
          event.type === 'worker/completed' ||
          event.type === 'worker/failed' ||
          event.type === 'worker/cancelled'
        ) {
          await this.#finish(active, event.type, event.data, event.nativeEvent)
        } else {
          await active.append(event.type, event.data, event.nativeEvent)
        }
      }

      if (!active.terminal) {
        await this.#finish(active, 'worker/failed', {
          message: `Adapter ${adapter.id} ended without a terminal event`,
        })
      }
    } catch (error) {
      if (!active.terminal) {
        await this.#finish(active, 'worker/failed', {
          message: error instanceof Error ? error.message : String(error),
        })
      }
    }
  }

  async #finish(
    active: InternalRun,
    type: 'worker/completed' | 'worker/failed' | 'worker/cancelled',
    data: unknown,
    nativeEvent?: unknown,
  ): Promise<void> {
    if (active.terminal) return
    active.terminal = true
    const status = type.slice('worker/'.length) as 'completed' | 'failed' | 'cancelled'
    const now = new Date().toISOString()
    active.worker.status = status
    active.step.status = status === 'completed' ? 'awaiting_host' : status
    active.step.updatedAt = now
    active.run.status = status === 'completed' ? 'awaiting_host' : status
    active.run.updatedAt = now
    active.worker.endedAt = now
    await active.append(type, data, nativeEvent)
    if (status === 'completed') {
      await active.append('run/awaiting_host', { stepId: active.step.id }, undefined, false, false)
    }
  }

  #assertConcurrency(request: RunRequest, maxRuns: number, maxWriters: number): void {
    if (this.#active.size >= maxRuns) {
      throw new RelayError('CONCURRENCY_LIMIT', `Concurrent run limit of ${maxRuns} reached`)
    }
    const writers = [...this.#active.values()].filter(
      (active) => active.run.accessMode === 'write',
    ).length
    if (request.accessMode === 'write' && writers >= maxWriters) {
      throw new RelayError('CONCURRENCY_LIMIT', `Concurrent writer limit of ${maxWriters} reached`)
    }
  }

  #assertWorkspaceIsolation(request: RunRequest, requireWorktree: boolean): void {
    if (!requireWorktree || request.accessMode !== 'write' || request.isolation === 'worktree') return
    const workspace = resolve(request.cwd)
    const conflicting = [...this.#active.values()].find(
      (active) =>
        active.run.accessMode === 'write' &&
        active.run.isolation === 'shared' &&
        resolve(active.run.cwd) === workspace,
    )
    if (conflicting) {
      throw new RelayError(
        'WORKSPACE_CONFLICT',
        `Run ${conflicting.run.id} is already writing to ${workspace}`,
      )
    }
  }
}

const MAX_NATIVE_EVENT_BYTES = 128 * 1024

function boundNativeEvent(nativeEvent: unknown): unknown {
  try {
    const serialized = JSON.stringify(nativeEvent)
    if (serialized.length <= MAX_NATIVE_EVENT_BYTES) return nativeEvent
    return {
      truncated: true,
      originalBytes: serialized.length,
      preview: serialized.slice(0, MAX_NATIVE_EVENT_BYTES),
    }
  } catch {
    return { truncated: true, preview: String(nativeEvent) }
  }
}
