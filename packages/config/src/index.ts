import { existsSync, mkdirSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { z } from 'zod'
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

/**
 * A runtime the user registered by hand, for CLIs the scanner cannot find (an
 * unusual install prefix, a wrapper script, a side-by-side version). Detection
 * still owns its own results; this is an addition, never an override.
 */
export interface ManualRuntime {
  id: string
  adapterId: string
  executablePath: string
  /** Free-form label shown next to the adapter id. */
  label?: string | undefined
}

export const manualRuntimeSchema = z.object({
  id: z.string().trim().min(1).max(128),
  adapterId: z.string().trim().min(1).max(128),
  executablePath: z.string().trim().min(1).max(4_096),
  label: z.string().trim().min(1).max(128).optional(),
})

export interface RelayConfig {
  profiles: AgentProfile[]
  policy: RelayPolicy
  workspaceOverrides: Record<string, RelayPolicyOverride>
  /** Hand-registered runtimes, merged with what the scanner finds. */
  manualRuntimes: ManualRuntime[]
  /**
   * Problems found while reading the files (unparseable JSON, invalid schema).
   * Relay keeps running on defaults, but the control panel has to say so
   * instead of presenting defaults as if they were the user's settings.
   */
  warnings: string[]
  /** Cheap change stamp (mtime+size of every file) for hot reload. */
  revision: string
}

export interface RelayConfigPaths {
  profiles: string
  settings: string
  runtimes: string
}

export function runtimesPath(): string {
  return process.env.RELAY_RUNTIMES_PATH ?? join(relayHome(), 'runtimes.json')
}

declare const __RELAY_VERSION__: string | undefined

/**
 * The running Relay version. Bundles inline it at build time (tools/define-version.mjs);
 * a source checkout falls back to the environment or to a marker.
 */
export function relayVersion(): string {
  if (process.env.RELAY_VERSION) return process.env.RELAY_VERSION
  if (typeof __RELAY_VERSION__ !== 'undefined') return __RELAY_VERSION__
  return '0.0.0-dev'
}

/** True when the current process is Electron rather than plain Node. */
export function runsOnElectron(): boolean {
  return typeof process.versions.electron === 'string'
}

/**
 * Where a packaged Relay keeps its runtime files (relayd, the MCP server, the
 * Codex integration sources, the web and panel builds). electron-builder puts
 * them in Contents/Resources; the tray passes this through when it starts the
 * daemon, so nothing has to guess a path relative to a bundle file.
 */
export function resourcesDir(): string | undefined {
  return process.env.RELAY_RESOURCES_DIR
}

export function defaultConfigPaths(): RelayConfigPaths {
  return { profiles: profilesPath(), settings: settingsPath(), runtimes: runtimesPath() }
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
    const warnings: string[] = []
    const profiles = this.#readProfiles(options.runtimes ?? [], warnings)
    const settings = this.#readSettings(warnings)
    const manualRuntimes = this.#readManualRuntimes(warnings)
    return {
      profiles,
      policy: settings.policy,
      workspaceOverrides: settings.workspaceOverrides,
      manualRuntimes,
      warnings,
      revision: this.revision(),
    }
  }

  #readProfiles(runtimes: Runtime[], warnings: string[]): AgentProfile[] {
    if (!existsSync(this.#paths.profiles)) return defaultProfiles(runtimes)
    try {
      return agentProfileSchema.array().parse(JSON.parse(readFileSync(this.#paths.profiles, 'utf8')))
    } catch (error) {
      // A hand-edited file must not take the daemon or the worker down, but the
      // fallback is reported: silently reverting to defaults looks like data loss.
      warnings.push(
        `profiles.json 无法解析，正在使用内置 Profile：${error instanceof Error ? error.message : String(error)}`,
      )
      return defaultProfiles(runtimes)
    }
  }

  #readSettings(warnings: string[]): {
    policy: RelayPolicy
    workspaceOverrides: Record<string, RelayPolicyOverride>
  } {
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
    } catch (error) {
      warnings.push(
        `settings.json 无法解析，正在使用默认策略：${error instanceof Error ? error.message : String(error)}`,
      )
      return { policy: defaultPolicy, workspaceOverrides: {} }
    }
  }

  #readManualRuntimes(warnings: string[]): ManualRuntime[] {
    if (!existsSync(this.#paths.runtimes)) return []
    try {
      return manualRuntimeSchema.array().parse(JSON.parse(readFileSync(this.#paths.runtimes, 'utf8')))
    } catch (error) {
      warnings.push(
        `runtimes.json 无法解析：${error instanceof Error ? error.message : String(error)}`,
      )
      return []
    }
  }

  #assertReadable(kind: 'profiles' | 'settings' | 'runtimes'): void {
    const warnings: string[] = []
    if (kind === 'profiles') this.#readProfiles([], warnings)
    else if (kind === 'settings') this.#readSettings(warnings)
    else this.#readManualRuntimes(warnings)
    if (warnings.length > 0) {
      throw new Error(`拒绝覆盖损坏的 ${kind}.json；请先备份并修复或移走原文件。${warnings[0]}`)
    }
  }

  writeManualRuntimes(runtimes: ManualRuntime[]): RelayConfig {
    this.#assertReadable('runtimes')
    atomicWrite(this.#paths.runtimes, manualRuntimeSchema.array().parse(runtimes))
    return this.read()
  }

  upsertManualRuntime(runtime: ManualRuntime): RelayConfig {
    const parsed = manualRuntimeSchema.parse(runtime)
    const current = this.read().manualRuntimes
    const index = current.findIndex((candidate) => candidate.id === parsed.id)
    if (index === -1) current.push(parsed)
    else current[index] = parsed
    return this.writeManualRuntimes(current)
  }

  removeManualRuntime(id: string): RelayConfig {
    return this.writeManualRuntimes(this.read().manualRuntimes.filter((runtime) => runtime.id !== id))
  }

  writeProfiles(profiles: AgentProfile[]): RelayConfig {
    this.#assertReadable('profiles')
    atomicWrite(this.#paths.profiles, agentProfileSchema.array().parse(profiles))
    return this.read()
  }

  /**
   * Individual settings writes used by the control panel. They deliberately
   * refuse to overwrite a file that failed to parse: the panel edits what it
   * read, and a corrupt file has no readable content to merge with.
   */

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
    this.#assertReadable('settings')
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
    return `${stamp(this.#paths.profiles)}|${stamp(this.#paths.settings)}|${stamp(this.#paths.runtimes)}`
  }
}
