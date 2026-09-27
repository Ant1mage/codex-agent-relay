import { mkdirSync } from 'node:fs'
import { dirname } from 'node:path'
import { serveStdio } from '@modelcontextprotocol/server/stdio'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { AntigravityAdapter } from '@relay/adapter-antigravity'
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
import { RuntimeConfigReloader } from './config-reloader.js'

interface RelayRuntime {
  service: RelayService
  commands: SqliteControlQueue
}

function findSessionId(value: unknown): string | undefined {
  if (!value || typeof value !== 'object') return undefined
  for (const key of ['session_id', 'thread_id', 'threadId']) {
    const candidate = (value as Record<string, unknown>)[key]
    if (typeof candidate === 'string' && candidate.length > 0) return candidate
  }
  for (const nested of Object.values(value as Record<string, unknown>)) {
    const candidate = findSessionId(nested)
    if (candidate) return candidate
  }
  return undefined
}

async function runSessionEndHook(): Promise<void> {
  let input = ''
  process.stdin.setEncoding('utf8')
  for await (const chunk of process.stdin) input += chunk
  const payload = input.trim() ? JSON.parse(input) as unknown : undefined
  const nativeSessionId = findSessionId(payload)
  if (!nativeSessionId) throw new Error('SessionEnd hook did not include a session id')
  const sessions = new SqliteHostSessionStore(resolveDatabasePath())
  try {
    const existing = sessions.get(`codex:${nativeSessionId}`)
    if (!existing) return
    sessions.upsertCodex({
      nativeSessionId,
      displayName: existing.displayName,
      cwd: existing.cwd,
      ...(existing.model ? { model: existing.model } : {}),
      status: 'ended',
    })
  } finally {
    sessions.close()
  }
}

async function createRuntime(): Promise<RelayRuntime> {
  const databasePath = resolveDatabasePath()
  mkdirSync(dirname(databasePath), { recursive: true })
  const controller = new RunController(new SqliteEventStore(databasePath))
  const config = new RelayConfigStore()
  const adapters: AgentAdapter[] = [
    new DeepSeekAdapter(), new AntigravityAdapter(), new KimiAdapter(), new ZaiAdapter(),
  ]
  for (const adapter of adapters) controller.registerAdapter(adapter)
  const reloader = new RuntimeConfigReloader(
    controller,
    config,
    adapters,
    (message) => process.stderr.write(`[relay] ${message}\n`),
  )
  await reloader.refresh()
  return {
    service: new RelayService(
      controller,
      new SqliteHostSessionStore(databasePath),
      new CodexAppServerThreadResolver(),
      (workspace) => reloader.applyWorkspace(workspace),
      () => reloader.refresh(),
    ),
    commands: new SqliteControlQueue(databasePath),
  }
}

if (process.argv.includes('--session-end-hook')) {
  await runSessionEndHook()
} else {
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
}
