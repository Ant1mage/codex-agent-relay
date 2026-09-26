import { mkdirSync } from 'node:fs'
import { dirname } from 'node:path'
import { serveStdio } from '@modelcontextprotocol/server/stdio'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { AntigravityAdapter } from '@relay/adapter-antigravity'
import { GeminiAdapter } from '@relay/adapter-gemini'
import { KimiAdapter } from '@relay/adapter-kimi'
import { ZaiAdapter } from '@relay/adapter-zai'
import type { AgentAdapter } from '@relay/adapter-sdk'
import { RelayConfigStore, databasePath as resolveDatabasePath } from '@relay/config'
import {
  RunController,
  SqliteControlQueue,
  SqliteEventStore,
  SqliteHostSessionStore,
} from '@relay/core'
import { CodexAppServerThreadResolver } from '@relay/integration-codex'
import { createRelayMcpServer } from './server.js'
import { RelayService } from './service.js'

interface RelayRuntime {
  service: RelayService
  commands: SqliteControlQueue
}

/**
 * Reloads Agent Profiles and policy from disk. Called at startup and again
 * before every listing or run, because the daemon (the menu bar's control
 * plane) writes those files while this process keeps running: configuration
 * edits must take effect without restarting Codex (docs/inspector.md 9).
 */
function applyConfig(
  controller: RunController,
  config: RelayConfigStore,
  workspace?: string,
): void {
  try {
    const loaded = config.read({ runtimes: controller.runtimes.list() })
    controller.profiles.sync(loaded.profiles)
    controller.policies.setGlobal(loaded.policy)
    if (workspace) {
      controller.policies.clearWorkspace(workspace)
      const override = loaded.workspaceOverrides[workspace]
      if (override) controller.policies.setWorkspace(workspace, override)
    }
  } catch (error) {
    process.stderr.write(
      `[relay] Ignoring invalid configuration: ${error instanceof Error ? error.message : String(error)}\n`,
    )
  }
}

async function createRuntime(): Promise<RelayRuntime> {
  const databasePath = resolveDatabasePath()
  mkdirSync(dirname(databasePath), { recursive: true })
  const controller = new RunController(new SqliteEventStore(databasePath))
  const config = new RelayConfigStore()
  const adapters: AgentAdapter[] = [
    new DeepSeekAdapter(), new AntigravityAdapter(), new KimiAdapter(), new GeminiAdapter(), new ZaiAdapter(),
  ]
  for (const adapter of adapters) controller.registerAdapter(adapter)
  const detections = await Promise.all(adapters.map((adapter) => adapter.detect()))
  for (const detection of detections) for (const diagnostic of detection.diagnostics) process.stderr.write(`[relay] ${diagnostic}\n`)
  const runtimes = detections.flatMap((detection) => detection.runtimes)
  for (const runtime of runtimes) controller.registerRuntime(runtime)
  applyConfig(controller, config)
  return {
    service: new RelayService(
      controller,
      new SqliteHostSessionStore(databasePath),
      new CodexAppServerThreadResolver(),
      (workspace) => applyConfig(controller, config, workspace),
      () => applyConfig(controller, config),
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
