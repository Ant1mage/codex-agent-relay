import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { databasePath, profilesPath, relayHome, settingsPath } from '@relay/config'

/**
 * Where the daemon records how to reach it. The tray and any script read this
 * file instead of assuming a port, which is what makes the automatic port
 * fallback safe: whoever wants the daemon asks this file where it is.
 *
 * The record also carries a per-process nonce. Liveness cannot be proven by a
 * PID (they get reused), so the startup guard and the tray's restart path both
 * make the daemon confirm its nonce before anything is trusted or killed.
 */

export const HOST = '127.0.0.1'
/** "RELA" on a phone keypad; IANA has no registration for it. */
export const DEFAULT_PORT = 7352
/** Consecutive ports the daemon tries before giving up. */
export const PORT_ATTEMPTS = 8

export { databasePath, profilesPath, relayHome, settingsPath }

export function serverInfoPath(): string {
  return `${relayHome()}/server.json`
}

export interface ServerInfo {
  pid: number
  port: number
  /** Loopback base URL, without the token. */
  url: string
  /** Per-run secret; required by /api/* and by the stream. */
  token: string
  /** Random per process start; /api/health echoes it so identity is provable. */
  nonce: string
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
      nonce: typeof parsed.nonce === 'string' ? parsed.nonce : '',
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
  // 0600: the token in this file is the only credential the daemon has.
  writeFileSync(path, `${JSON.stringify(info, null, 2)}\n`, { encoding: 'utf8', mode: 0o600 })
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
