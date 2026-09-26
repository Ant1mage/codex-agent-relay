import { accessSync, constants } from 'node:fs'
import type { AgentAdapter } from '@relay/adapter-sdk'
import type { RelayConfigStore } from '@relay/config'
import type { RunController } from '@relay/core'
import type { Runtime } from '@relay/protocol'

const FALLBACK_CAPABILITIES = {
  nonInteractive: true,
  structuredEvents: false,
  cwd: true,
  resume: false,
  send: false,
  cancel: false,
  childSessions: false,
} as const

function canExecute(path: string): boolean {
  try {
    accessSync(path, constants.X_OK)
    return true
  } catch {
    return false
  }
}

/** Keeps relay-mcp aligned with daemon-owned files and changing local CLIs. */
export class RuntimeConfigReloader {
  readonly #reported = new Set<string>()
  #tail: Promise<void> = Promise.resolve()

  constructor(
    readonly controller: RunController,
    readonly config: RelayConfigStore,
    readonly adapters: AgentAdapter[],
    readonly report: (message: string) => void = () => undefined,
  ) {}

  /** Serialises concurrent MCP calls so nobody sees a half-applied scan. */
  refresh(): Promise<void> {
    const next = this.#tail.then(() => this.#refresh())
    this.#tail = next.catch(() => undefined)
    return next
  }

  /** Applies the workspace override after runAgent's global refresh. */
  applyWorkspace(workspace: string): void {
    const loaded = this.config.read({ runtimes: this.controller.runtimes.list() })
    this.controller.policies.clearWorkspace(workspace)
    const override = loaded.workspaceOverrides[workspace]
    if (override) this.controller.policies.setWorkspace(workspace, override)
  }

  async #refresh(): Promise<void> {
    const detections = await Promise.all(
      this.adapters.map(async (adapter) => {
        try {
          return await adapter.detect()
        } catch (error) {
          return {
            runtimes: [],
            diagnostics: [
              `${adapter.id} detection failed: ${error instanceof Error ? error.message : String(error)}`,
            ],
          }
        }
      }),
    )
    const runtimes: Runtime[] = detections.flatMap((detection) => detection.runtimes)
    const loaded = this.config.read({ runtimes })

    for (const entry of loaded.manualRuntimes) {
      if (runtimes.some((runtime) => runtime.id === entry.id)) continue
      if (!canExecute(entry.executablePath)) {
        this.#reportOnce(`Manual runtime ${entry.id} is not executable: ${entry.executablePath}`)
        continue
      }
      const adapter = this.adapters.find((candidate) => candidate.id === entry.adapterId)
      runtimes.push({
        id: entry.id,
        adapterId: entry.adapterId,
        executablePath: entry.executablePath,
        health: 'available',
        capabilities: adapter?.capabilities() ?? FALLBACK_CAPABILITIES,
      })
    }

    for (const diagnostic of detections.flatMap((detection) => detection.diagnostics)) {
      this.#reportOnce(diagnostic)
    }
    for (const warning of loaded.warnings) this.#reportOnce(warning)
    this.controller.runtimes.sync(runtimes)
    this.controller.profiles.sync(loaded.profiles)
    this.controller.policies.setGlobal(loaded.policy)
  }

  #reportOnce(message: string): void {
    if (this.#reported.has(message)) return
    this.#reported.add(message)
    this.report(message)
  }
}
