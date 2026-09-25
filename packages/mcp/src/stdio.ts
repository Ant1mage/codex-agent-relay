import { existsSync, mkdirSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { serveStdio } from '@modelcontextprotocol/server/stdio'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import {
  RunController,
  SqliteControlQueue,
  SqliteEventStore,
  SqliteHostSessionStore,
} from '@relay/core'
import {
  agentProfileSchema,
  relayPolicyOverrideSchema,
  relayPolicySchema,
  type AgentProfile,
} from '@relay/protocol'
import { CodexAppServerThreadResolver } from '@relay/integration-codex'
import { createRelayMcpServer } from './server.js'
import { RelayService } from './service.js'

interface RelayRuntime {
  service: RelayService
  commands: SqliteControlQueue
}

function applyPolicySettings(
  controller: RunController,
  settingsPath: string,
  workspace?: string,
): void {
  if (!existsSync(settingsPath)) return
  try {
    const parsed = JSON.parse(readFileSync(settingsPath, 'utf8')) as {
      policy?: unknown
      workspaceOverrides?: Record<string, unknown>
    }
    controller.policies.setGlobal(relayPolicySchema.parse(parsed.policy))
    if (workspace) {
      controller.policies.clearWorkspace(workspace)
      const override = parsed.workspaceOverrides?.[workspace]
      if (override) controller.policies.setWorkspace(workspace, relayPolicyOverrideSchema.parse(override))
    }
  } catch (error) {
    process.stderr.write(
      `[relay] Ignoring invalid settings: ${error instanceof Error ? error.message : String(error)}\n`,
    )
  }
}

async function createRuntime(): Promise<RelayRuntime> {
  const databasePath = process.env.RELAY_DB_PATH ?? join(homedir(), '.relay', 'relay.sqlite')
  const settingsPath = process.env.RELAY_SETTINGS_PATH ?? join(homedir(), '.relay', 'settings.json')
  const profilesPath = process.env.RELAY_PROFILES_PATH ?? join(homedir(), '.relay', 'profiles.json')
  mkdirSync(dirname(databasePath), { recursive: true })
  const controller = new RunController(new SqliteEventStore(databasePath))
  applyPolicySettings(controller, settingsPath)
  const adapter = new DeepSeekAdapter()
  controller.registerAdapter(adapter)
  const detection = await adapter.detect()
  for (const diagnostic of detection.diagnostics) process.stderr.write(`[relay] ${diagnostic}\n`)
  const runtime = detection.runtimes[0]
  if (runtime) {
    controller.registerRuntime(runtime)
    const defaults: AgentProfile[] = [
      {
        id: 'deepseek-code',
        name: 'DeepSeek Code',
        runtimeId: runtime.id,
        description: 'Read and write code, execute commands, and run tests with DeepSeek Harness.',
        capabilities: {
          readWorkspace: true,
          writeWorkspace: true,
          executeCommands: true,
          networkAccess: false,
        },
        enabled: true,
      },
      {
        id: 'deepseek-research',
        name: 'DeepSeek Research',
        runtimeId: runtime.id,
        description: 'Inspect the workspace and research without writing files.',
        capabilities: {
          readWorkspace: true,
          writeWorkspace: false,
          executeCommands: false,
          networkAccess: true,
        },
        enabled: true,
      },
    ]
    let profiles = defaults
    if (existsSync(profilesPath)) {
      try {
        profiles = agentProfileSchema
          .array()
          .parse(JSON.parse(readFileSync(profilesPath, 'utf8')))
          .map((profile) => ({ ...profile, runtimeId: runtime.id }))
      } catch (error) {
        process.stderr.write(
          `[relay] Ignoring invalid profiles: ${error instanceof Error ? error.message : String(error)}\n`,
        )
      }
    }
    for (const profile of profiles) controller.registerProfile(profile)
  }
  return {
    service: new RelayService(
      controller,
      new SqliteHostSessionStore(databasePath),
      new CodexAppServerThreadResolver(),
      (workspace) => applyPolicySettings(controller, settingsPath, workspace),
    ),
    commands: new SqliteControlQueue(databasePath),
  }
}

const runtime = await createRuntime()
let drainingCommands = false
const commandTimer = setInterval(() => {
  if (drainingCommands) return
  const command = runtime.commands.claimNext()
  if (!command) return
  drainingCommands = true
  void runtime.service
    .cancel(command.workerSessionId)
    .then(() => runtime.commands.complete(command.id))
    .catch((error: unknown) =>
      runtime.commands.fail(command.id, error instanceof Error ? error.message : String(error)),
    )
    .finally(() => {
      drainingCommands = false
    })
}, 250)
commandTimer.unref()

void serveStdio(() => createRelayMcpServer(runtime.service))
