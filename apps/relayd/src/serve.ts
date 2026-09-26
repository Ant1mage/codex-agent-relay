import { randomBytes } from 'node:crypto'
import { mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import type { Server } from 'node:http'
import { dirname, join } from 'node:path'
import { RelayConfigStore, relayHome, type RelayConfig } from '@relay/config'
import type { CodexStatus } from '@relay/relay-api'
import { RelayClient } from '@relay/relay-api'
import {
  DEFAULT_PORT,
  HOST,
  PORT_ATTEMPTS,
  clearServerInfo,
  databasePath,
  inspectorUrl,
  readServerInfo,
  serverInfoPath,
  writeServerInfo,
} from '@relay/relay-api/server-info'
import {
  codexStatus,
  detectEnvironment,
  runCodexAction,
  runtimeOptions,
  type Environment,
} from './environment.js'
import { createRelayServer } from './server.js'
import { RelayStore } from './store.js'

/**
 * Entry point: pnpm relayd (or tsx apps/relayd/src/serve.ts).
 *
 * The daemon is Relay's local control plane: it reads the event store the MCP
 * process writes, owns Relay's configuration (Agent Profiles and policy), runs
 * the Codex integration lifecycle, and serves all of that to the tray and the
 * inspector over loopback. It never starts or owns a worker, so it can be
 * restarted at any time without touching a running delegation.
 */

const CODEX_CACHE_MS = 60_000
const STARTUP_LOCK_MS = 3_000
const STALE_LOCK_MS = 10_000

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

/**
 * The web inspector ships next to the daemon; source checkouts keep it in
 * apps/web/out. RELAY_WEB_ROOT overrides both.
 */
function webRoot(): string {
  const candidates = [
    process.env.RELAY_WEB_ROOT,
    join(import.meta.dirname, 'web'),
    join(import.meta.dirname, '../../web/out'),
  ]
  for (const candidate of candidates) {
    if (candidate && readFileSync.length >= 0) {
      try {
        statSync(join(candidate, 'index.html'))
        return candidate
      } catch {
        // Try the next location.
      }
    }
  }
  return candidates[1] ?? join(import.meta.dirname, 'web')
}

/** The control panel ships beside the daemon; source checkouts keep it in the tray's out/. */
function panelRoot(): string {
  const candidates = [
    process.env.RELAY_PANEL_ROOT,
    join(import.meta.dirname, 'panel'),
    join(import.meta.dirname, '../../menu-bar/out/panel'),
  ]
  for (const candidate of candidates) {
    if (!candidate) continue
    try {
      statSync(join(candidate, 'index.html'))
      return candidate
    } catch {
      // Try the next location.
    }
  }
  return candidates[1] ?? join(import.meta.dirname, 'panel')
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

/**
 * Only a daemon that answers with the recorded nonce counts as running. A PID is
 * not identity: they get reused, and a stale server.json must never block a
 * fresh start (docs/inspector.md 2).
 */
async function confirmedRunningDaemon(): Promise<{ url: string; pid: number } | undefined> {
  const info = readServerInfo()
  if (!info) return undefined
  try {
    const health = await new RelayClient({ baseUrl: info.url, token: info.token }).health()
    if (health.pid === info.pid && info.nonce.length > 0 && health.nonce === info.nonce) {
      return { url: info.url, pid: info.pid }
    }
  } catch {
    // Nothing answered: the record is stale.
  }
  return undefined
}

/**
 * Serialises concurrent starts. Two daemons launched at the same moment would
 * otherwise both pass the check and the later one would steal server.json.
 */
async function acquireStartupLock(): Promise<boolean> {
  const lock = join(relayHome(), 'daemon.lock')
  mkdirSync(dirname(lock), { recursive: true })
  const deadline = Date.now() + STARTUP_LOCK_MS
  while (Date.now() < deadline) {
    try {
      writeFileSync(lock, String(process.pid), { encoding: 'utf8', flag: 'wx', mode: 0o600 })
      return true
    } catch {
      try {
        const age = Date.now() - statSync(lock).mtimeMs
        if (age > STALE_LOCK_MS) {
          rmSync(lock, { force: true })
          continue
        }
      } catch {
        continue
      }
      await new Promise((resolve) => setTimeout(resolve, 150))
    }
  }
  return false
}

const alreadyRunning = await confirmedRunningDaemon()
if (alreadyRunning) {
  process.stdout.write(`Relay daemon is already running at ${alreadyRunning.url} (pid ${alreadyRunning.pid})\n`)
  process.exit(0)
}
if (!(await acquireStartupLock())) {
  process.stdout.write('Another Relay daemon is starting; giving up this attempt.\n')
  process.exit(0)
}

const version = packageVersion()
const nonce = randomBytes(12).toString('base64url')
const token = process.env.RELAY_TOKEN ?? randomBytes(24).toString('base64url')
const database = databasePath()
mkdirSync(dirname(database), { recursive: true })

const store = new RelayStore(database)
const config = new RelayConfigStore()
let environment: Environment = await detectEnvironment(config)
store.setEnvironment(environment)

let codexCache: { at: number; value: CodexStatus } | undefined
async function codex(): Promise<CodexStatus> {
  if (codexCache && Date.now() - codexCache.at < CODEX_CACHE_MS) return codexCache.value
  const value = await codexStatus()
  codexCache = { at: Date.now(), value }
  return value
}

/** Every configuration write invalidates the Codex view and reloads the store. */
function applyConfig(next: RelayConfig): RelayConfig {
  environment = { ...environment, profiles: next.profiles }
  store.setEnvironment(environment)
  return next
}

const startedAt = new Date().toISOString()
const portRef = { current: parsePort() }

const server = createRelayServer({
  store,
  environment: () => environment,
  config: () => config.read({ runtimes: environment.runtimes }),
  codex,
  refresh: async () => {
    environment = await detectEnvironment(config)
    store.setEnvironment(environment)
    codexCache = undefined
    return { environment, config: config.read({ runtimes: environment.runtimes }) }
  },
  runCodex: async (action) => {
    const result = await runCodexAction(action)
    codexCache = { at: Date.now(), value: result.status }
    return result
  },
  runtimeOptions: (runtimeId) => runtimeOptions(runtimeId, environment.runtimes),
  saveProfile: (profile) => applyConfig(config.upsertProfile(profile, { runtimes: environment.runtimes })),
  deleteProfile: (profileId) => applyConfig(config.removeProfile(profileId)),
  savePolicy: (input) =>
    applyConfig(config.writeSettings({ policy: input.policy, workspaceOverrides: input.workspaceOverrides })),
  webRoot: webRoot(),
  panelRoot: panelRoot(),
  token,
  port: () => portRef.current,
  version,
  databasePath: database,
  startedAt,
  nonce,
})

portRef.current = await listen(server, portRef.current)
const port = portRef.current

const info = {
  pid: process.pid,
  port,
  url: `http://${HOST}:${port}`,
  token,
  nonce,
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
  rmSync(join(relayHome(), 'daemon.lock'), { force: true })
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
