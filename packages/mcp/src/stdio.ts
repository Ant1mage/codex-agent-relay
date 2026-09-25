import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { serveStdio } from '@modelcontextprotocol/server/stdio'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { HostSessionRegistry, RunController, SqliteEventStore } from '@relay/core'
import { CodexAppServerThreadResolver } from '@relay/integration-codex'
import { createRelayMcpServer } from './server.js'
import { RelayService } from './service.js'

async function createService(): Promise<RelayService> {
  const databasePath = process.env.RELAY_DB_PATH ?? join(homedir(), '.relay', 'relay.sqlite')
  mkdirSync(dirname(databasePath), { recursive: true })
  const controller = new RunController(new SqliteEventStore(databasePath))
  const adapter = new DeepSeekAdapter()
  controller.registerAdapter(adapter)
  const detection = await adapter.detect()
  for (const diagnostic of detection.diagnostics) process.stderr.write(`[relay] ${diagnostic}\n`)
  const runtime = detection.runtimes[0]
  if (runtime) {
    controller.registerRuntime(runtime)
    controller.registerProfile({
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
    })
    controller.registerProfile({
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
    })
  }
  return new RelayService(
    controller,
    new HostSessionRegistry(),
    new CodexAppServerThreadResolver(),
  )
}

const service = await createService()
void serveStdio(() => createRelayMcpServer(service))

