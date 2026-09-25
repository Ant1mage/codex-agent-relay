import { resolve } from 'node:path'
import {
  relayPolicyOverrideSchema,
  relayPolicySchema,
  type AgentProfile,
  type RelayPolicy,
  type RelayPolicyOverride,
  type RunRequest,
} from '@relay/protocol'
import { RelayError } from '@relay/protocol'

export const defaultPolicy: RelayPolicy = {
  maxConcurrentRuns: 4,
  maxConcurrentWriters: 1,
  requireWorktreeForParallelWriters: true,
  allowWrite: true,
  allowCommands: true,
  allowNetwork: true,
}

export interface PolicyScope {
  workspace?: string
  hostSessionId?: string
}

export class PolicyResolver {
  #global: RelayPolicy
  readonly #workspace = new Map<string, RelayPolicyOverride>()
  readonly #session = new Map<string, RelayPolicyOverride>()

  constructor(global: RelayPolicy = defaultPolicy) {
    this.#global = relayPolicySchema.parse(global)
  }

  setGlobal(policy: RelayPolicy): void {
    this.#global = relayPolicySchema.parse(policy)
  }

  setWorkspace(workspace: string, override: RelayPolicyOverride): void {
    this.#workspace.set(resolve(workspace), relayPolicyOverrideSchema.parse(override))
  }

  clearWorkspace(workspace: string): void {
    this.#workspace.delete(resolve(workspace))
  }

  setSession(hostSessionId: string, override: RelayPolicyOverride): void {
    this.#session.set(hostSessionId, relayPolicyOverrideSchema.parse(override))
  }

  clearSession(hostSessionId: string): void {
    this.#session.delete(hostSessionId)
  }

  resolve(scope: PolicyScope): RelayPolicy {
    return relayPolicySchema.parse({
      ...this.#global,
      ...(scope.workspace ? this.#workspace.get(resolve(scope.workspace)) : undefined),
      ...(scope.hostSessionId ? this.#session.get(scope.hostSessionId) : undefined),
    })
  }
}

export function assertPolicyAllows(
  policy: RelayPolicy,
  request: RunRequest,
  profile: AgentProfile,
): void {
  if (request.accessMode === 'write' && !policy.allowWrite) {
    throw new RelayError('CAPABILITY_DENIED', 'Workspace writes are disabled by policy')
  }
  if (profile.capabilities.executeCommands && !policy.allowCommands) {
    throw new RelayError('CAPABILITY_DENIED', 'Command execution is disabled by policy')
  }
  if (profile.capabilities.networkAccess && !policy.allowNetwork) {
    throw new RelayError('CAPABILITY_DENIED', 'Network access is disabled by policy')
  }
}
