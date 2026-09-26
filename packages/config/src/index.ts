import { existsSync, mkdirSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { defaultProfiles } from '@relay/core/default-profiles'
import { defaultPolicy } from '@relay/core/policy'
import {
  agentProfileSchema,
  relayPolicyOverrideSchema,
  relayPolicySchema,
  type AgentProfile,
  type RelayPolicy,
  type RelayPolicyOverride,
  type Runtime,
} from '@relay/protocol'

/**
 * Relay's own configuration, on disk, with one owner for the paths.
 *
 * profiles.json is the Agent Profile set exposed to Codex; settings.json is the
 * routing policy plus per-workspace overrides. Both are read by relay-mcp (the
 * execution side) and written by relayd (the control plane), so the file format
 * lives here rather than in either process.
 */

export function relayHome(): string {
  return process.env.RELAY_HOME ?? join(homedir(), '.relay')
}

export function databasePath(): string {
  return process.env.RELAY_DB_PATH ?? join(relayHome(), 'relay.sqlite')
}

export function settingsPath(): string {
  return process.env.RELAY_SETTINGS_PATH ?? join(relayHome(), 'settings.json')
}

export function profilesPath(): string {
  return process.env.RELAY_PROFILES_PATH ?? join(relayHome(), 'profiles.json')
}

export interface RelayConfig {
  profiles: AgentProfile[]
  policy: RelayPolicy
  workspaceOverrides: Record<string, RelayPolicyOverride>
  /** Cheap change stamp (mtime+size of both files) for hot reload. */
  revision: string
}

export interface RelayConfigPaths {
  profiles: string
  settings: string
}

export function defaultConfigPaths(): RelayConfigPaths {
  return { profiles: profilesPath(), settings: settingsPath() }
}

function atomicWrite(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true })
  const temporary = `${path}.tmp`
  writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, 'utf8')
  renameSync(temporary, path)
}

function stamp(path: string): string {
  try {
    const stats = statSync(path)
    return `${stats.mtimeMs}:${stats.size}`
  } catch {
    return 'absent'
  }
}

export class RelayConfigStore {
  readonly #paths: RelayConfigPaths

  constructor(paths: RelayConfigPaths = defaultConfigPaths()) {
    this.#paths = paths
  }

  get paths(): RelayConfigPaths {
    return this.#paths
  }

  /**
   * Profiles come from the file when it exists, otherwise from the runtimes that
   * were actually detected. A profile whose runtime is missing is kept: hiding
   * user configuration would make the loss invisible.
   */
  read(options: { runtimes?: Runtime[] } = {}): RelayConfig {
    const profiles = this.#readProfiles(options.runtimes ?? [])
    const settings = this.#readSettings()
    return {
      profiles,
      policy: settings.policy,
      workspaceOverrides: settings.workspaceOverrides,
      revision: this.revision(),
    }
  }

  #readProfiles(runtimes: Runtime[]): AgentProfile[] {
    if (!existsSync(this.#paths.profiles)) return defaultProfiles(runtimes)
    try {
      return agentProfileSchema.array().parse(JSON.parse(readFileSync(this.#paths.profiles, 'utf8')))
    } catch {
      // A hand-edited file must not take the daemon or the worker down.
      return defaultProfiles(runtimes)
    }
  }

  #readSettings(): { policy: RelayPolicy; workspaceOverrides: Record<string, RelayPolicyOverride> } {
    if (!existsSync(this.#paths.settings)) return { policy: defaultPolicy, workspaceOverrides: {} }
    try {
      const parsed = JSON.parse(readFileSync(this.#paths.settings, 'utf8')) as {
        policy?: unknown
        workspaceOverrides?: Record<string, unknown>
      }
      const workspaceOverrides: Record<string, RelayPolicyOverride> = {}
      for (const [workspace, override] of Object.entries(parsed.workspaceOverrides ?? {})) {
        workspaceOverrides[workspace] = relayPolicyOverrideSchema.parse(override)
      }
      return { policy: relayPolicySchema.parse(parsed.policy), workspaceOverrides }
    } catch {
      return { policy: defaultPolicy, workspaceOverrides: {} }
    }
  }

  writeProfiles(profiles: AgentProfile[]): RelayConfig {
    atomicWrite(this.#paths.profiles, agentProfileSchema.array().parse(profiles))
    return this.read()
  }

  upsertProfile(profile: AgentProfile, options: { runtimes?: Runtime[] } = {}): RelayConfig {
    const parsed = agentProfileSchema.parse(profile)
    const current = this.read(options).profiles
    const index = current.findIndex((candidate) => candidate.id === parsed.id)
    if (index === -1) current.push(parsed)
    else current[index] = parsed
    return this.writeProfiles(current)
  }

  removeProfile(id: string): RelayConfig {
    const current = this.read().profiles.filter((profile) => profile.id !== id)
    return this.writeProfiles(current)
  }

  writeSettings(input: {
    policy: RelayPolicy
    workspaceOverrides: Record<string, RelayPolicyOverride>
  }): RelayConfig {
    const workspaceOverrides: Record<string, RelayPolicyOverride> = {}
    for (const [workspace, override] of Object.entries(input.workspaceOverrides)) {
      workspaceOverrides[workspace] = relayPolicyOverrideSchema.parse(override)
    }
    atomicWrite(this.#paths.settings, {
      policy: relayPolicySchema.parse(input.policy),
      workspaceOverrides,
    })
    return this.read()
  }

  revision(): string {
    return `${stamp(this.#paths.profiles)}|${stamp(this.#paths.settings)}`
  }
}
