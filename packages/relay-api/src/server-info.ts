import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'

/**
 * Where the daemon records how to reach it. The tray and any script read this
 * file instead of assuming a port, which is what makes the automatic port
 * fallback safe: whoever wants the daemon asks this file where it is.
 */

export const HOST = '127.0.0.1'
/** "RELA" on a phone keypad; IANA has no registration for it. */
export const DEFAULT_PORT = 7352
/** Consecutive ports the daemon tries before giving up. */
export const PORT_ATTEMPTS = 8
export const DEFAULT_TOKEN_TTL_MS = 0

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

export function serverInfoPath(): string {
  return join(relayHome(), 'server.json')
}

export interface ServerInfo {
  pid: number
  port: number
  /** Loopback base URL, without the token. */
  url: string
  /** Per-run secret; required by /api/* and by the stream. */
  token: string
  startedAt: string
  version: string
  database: string
}

export function readServerInfo(path = serverInfoPath()): ServerInfo | undefined {
  try {
    if (!existsSync(path)) return undefined
    const parsed = JSON.parse(readFileSync(path, 'utf8')) as Partial<ServerInfo>
    if (
      typeof parsed.pid !== 'number' ||
      typeof parsed.port !== 'number' ||
      typeof parsed.token !== 'string' ||
      typeof parsed.url !== 'string'
    ) {
      return undefined
    }
    return {
      pid: parsed.pid,
      port: parsed.port,
      url: parsed.url,
      token: parsed.token,
      startedAt: typeof parsed.startedAt === 'string' ? parsed.startedAt : '',
      version: typeof parsed.version === 'string' ? parsed.version : '0.0.0',
      database: typeof parsed.database === 'string' ? parsed.database : databasePath(),
    }
  } catch {
    // A half-written or hand-edited file is treated as "no daemon".
    return undefined
  }
}

export function writeServerInfo(info: ServerInfo, path = serverInfoPath()): void {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(info, null, 2)}\n`, 'utf8')
}

/** Removes the file only when it still describes this process. */
export function clearServerInfo(pid: number, path = serverInfoPath()): void {
  try {
    const current = readServerInfo(path)
    if (current && current.pid !== pid) return
    rmSync(path, { force: true })
  } catch {
    // Shutdown must not fail on cleanup.
  }
}

export function isProcessAlive(pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/**
 * Inspector route for a session or a run. The tray deep-links through this, so
 * the shape lives next to the URL builder instead of in each caller.
 */
export function inspectorPath(sessionId?: string, runId?: string): string {
  if (!sessionId) return '/'
  const base = `/s/${encodeURIComponent(sessionId)}`
  return runId ? `${base}/r/${encodeURIComponent(runId)}` : base
}

/** Loopback URL plus the token in the fragment, which no server ever receives. */
export function inspectorUrl(info: ServerInfo, sessionId?: string, runId?: string): string {
  return `${info.url}${inspectorPath(sessionId, runId)}#t=${info.token}`
}

export function apiUrl(info: ServerInfo, path: string): string {
  return `${info.url}${path}`
}
