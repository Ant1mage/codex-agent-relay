import { randomBytes } from 'node:crypto'
import { mkdirSync, readFileSync } from 'node:fs'
import type { Server } from 'node:http'
import { dirname, join } from 'node:path'
import {
  DEFAULT_PORT,
  HOST,
  PORT_ATTEMPTS,
  clearServerInfo,
  databasePath,
  inspectorUrl,
  isProcessAlive,
  readServerInfo,
  writeServerInfo,
} from '@relay/relay-api/server-info'
import type { CodexStatus } from '@relay/relay-api'
import {
  codexStatus,
  detectEnvironment,
  installCodexIntegration,
  type Environment,
} from './environment.js'
import { createRelayServer } from './server.js'
import { RelayStore } from './store.js'

/**
 * Entry point: pnpm relayd (or tsx apps/relayd/src/serve.ts).
 *
 * The daemon reads the event store the MCP process writes and serves the
 * inspector over loopback. It never starts or owns a worker, so it can be
 * restarted at any time without touching a running delegation.
 */

const CODEX_CACHE_MS = 60_000

function packageVersion(): string {
  try {
    const parsed = JSON.parse(readFileSync(join(import.meta.dirname, '../package.json'), 'utf8')) as {
      version?: string
    }
    return parsed.version ?? '0.0.0'
  } catch {
    return '0.0.0'
  }
}

function webRoot(): string {
  return process.env.RELAY_WEB_ROOT ?? join(import.meta.dirname, '../../web/out')
}

function parsePort(): number {
  const value = Number(process.env.RELAY_PORT)
  return Number.isInteger(value) && value > 0 && value < 65_536 ? value : DEFAULT_PORT
}

/** Binds the first free port in the block, so a second daemon never fails hard. */
function listen(server: Server, port: number): Promise<number> {
  return new Promise((resolve, reject) => {
    let candidate = port
    let remaining = PORT_ATTEMPTS
    const onError = (error: NodeJS.ErrnoException) => {
      if (error.code === 'EADDRINUSE' && remaining > 0) {
        remaining -= 1
        candidate += 1
        server.listen(candidate, HOST)
        return
      }
      reject(error)
    }
    server.on('error', onError)
    server.once('listening', () => {
      server.off('error', onError)
      resolve(candidate)
    })
    server.listen(candidate, HOST)
  })
}

/*
 * One daemon per user. Starting a second one would silently steal
 * ~/.relay/server.json from the first, so the tray's "Start log service" has to
 * be idempotent: if a live daemon is already recorded, this process exits.
 */
const running = readServerInfo()
if (running && isProcessAlive(running.pid)) {
  process.stdout.write(`Relay daemon is already running at ${running.url} (pid ${running.pid})\n`)
  process.exit(0)
}

const version = packageVersion()
const database = databasePath()
mkdirSync(dirname(database), { recursive: true })

const store = new RelayStore(database)
let environment: Environment = await detectEnvironment()
store.setEnvironment(environment)

let codexCache: { at: number; value: CodexStatus } | undefined
async function codex(): Promise<CodexStatus> {
  if (codexCache && Date.now() - codexCache.at < CODEX_CACHE_MS) return codexCache.value
  const value = await codexStatus()
  codexCache = { at: Date.now(), value }
  return value
}

const token = process.env.RELAY_TOKEN ?? randomBytes(24).toString('base64url')
const startedAt = new Date().toISOString()

// The port is only known after binding, so the server reads it through a ref.
const portRef = { current: parsePort() }
const server = createRelayServer({
  store,
  environment: () => environment,
  codex,
  refreshEnvironment: async () => {
    environment = await detectEnvironment()
    store.setEnvironment(environment)
    codexCache = undefined
    return environment
  },
  installCodex: async () => {
    const result = await installCodexIntegration()
    codexCache = { at: Date.now(), value: result.status }
    return result
  },
  webRoot: webRoot(),
  token,
  port: () => portRef.current,
  version,
  databasePath: database,
  startedAt,
})

portRef.current = await listen(server, portRef.current)
const port = portRef.current

const info = {
  pid: process.pid,
  port,
  url: `http://${HOST}:${port}`,
  token,
  startedAt,
  version,
  database,
}
writeServerInfo(info)

process.stdout.write(
  [
    `Relay daemon ${version} listening on ${info.url}`,
    `  inspector  ${inspectorUrl(info)}`,
    `  database   ${database}`,
    `  runtimes   ${environment.runtimes.length} detected, ${environment.profiles.length} profiles`,
    `  store      ${store.isInitialised() ? 'ready' : 'empty — waiting for the first Codex delegation'}`,
    '',
  ].join('\n'),
)

let closing = false
function shutdown(signal: string): void {
  if (closing) return
  closing = true
  process.stdout.write(`\nRelay daemon stopping (${signal})\n`)
  clearServerInfo(process.pid)
  server.close(() => {
    store.close()
    process.exit(0)
  })
  // A stuck SSE client must not keep the process alive forever.
  setTimeout(() => {
    store.close()
    process.exit(0)
  }, 1_500).unref()
}

process.on('SIGINT', () => shutdown('SIGINT'))
process.on('SIGTERM', () => shutdown('SIGTERM'))
