import type { CodexThreadMetadataResolver } from '@relay/integration-codex'
import { RunController, type RunProjection } from '@relay/core'
import type { HostSessionStore } from '@relay/core'
import type { AccessMode, AgentProfile, Isolation } from './types.js'
import type { CodexInvocationContext } from './context.js'

export interface RunAgentInput {
  agentId: string
  task: string
  accessMode?: AccessMode
  isolation?: Isolation
}

export class RelayService {
  constructor(
    readonly controller: RunController,
    readonly sessions: HostSessionStore,
    readonly codexThreads: CodexThreadMetadataResolver,
    readonly beforeRun?: (workspace: string) => void | Promise<void>,
  ) {}

  async syncSession(context: CodexInvocationContext): Promise<ReturnType<HostSessionStore['upsertCodex']>> {
    const metadata = await this.codexThreads.resolve(context.threadId)
    return this.sessions.upsertCodex({
      nativeSessionId: metadata.id,
      displayName: metadata.displayName,
      cwd: metadata.cwd,
      ...(metadata.model ? { model: metadata.model } : {}),
      status: 'active',
    })
  }

  async endSession(context: CodexInvocationContext): Promise<void> {
    const session = await this.syncSession(context)
    this.sessions.upsertCodex({
      nativeSessionId: session.nativeSessionId,
      displayName: session.displayName,
      cwd: session.cwd,
      ...(session.model ? { model: session.model } : {}),
      status: 'ended',
    })
    this.controller.policies.clearSession(session.id)
  }

  async listAgents(context: CodexInvocationContext): Promise<AgentProfile[]> {
    await this.syncSession(context)
    return this.controller.profiles.list({ enabledOnly: true })
  }

  async runAgent(context: CodexInvocationContext, input: RunAgentInput): Promise<{
    runId: string
    workerSessionId: string
    hostSessionDisplayName: string
  }> {
    const session = await this.syncSession(context)
    await this.beforeRun?.(session.cwd)
    const active = await this.controller.start({
      hostSessionId: session.id,
      profileId: input.agentId,
      task: input.task,
      cwd: session.cwd,
      accessMode: input.accessMode ?? 'read_only',
      isolation: input.isolation ?? 'shared',
    })
    return {
      runId: active.run.id,
      workerSessionId: active.worker.id,
      hostSessionDisplayName: session.displayName,
    }
  }

  async status(workerSessionId: string): Promise<RunProjection> {
    return this.controller.getByWorker(workerSessionId)
  }

  async wait(workerSessionId: string): Promise<RunProjection> {
    return this.controller.waitForWorker(workerSessionId)
  }

  async send(workerSessionId: string, message: string): Promise<void> {
    await this.controller.send(workerSessionId, message)
  }

  async cancel(workerSessionId: string): Promise<void> {
    await this.controller.cancelWorker(workerSessionId)
  }

  async accept(workerSessionId: string): Promise<RunProjection> {
    return this.controller.acceptWorker(workerSessionId)
  }

  async resume(workerSessionId: string, feedback: string): Promise<{
    runId: string
    workerSessionId: string
  }> {
    const active = await this.controller.resume(workerSessionId, feedback)
    return { runId: active.run.id, workerSessionId: active.worker.id }
  }
}
