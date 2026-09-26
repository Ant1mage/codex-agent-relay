import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { app, shell } from 'electron'
import { RelayClient, type MenuView } from '@relay/relay-api'
import { inspectorUrl, readServerInfo, type ServerInfo } from '@relay/relay-api/server-info'

/**
 * Everything the tray knows about the daemon. The tray is a client: it reads
 * ~/.relay/server.json, asks the daemon for the projection, and can start the
 * daemon and open the inspector or the panel — it never touches the database.
 */

export type DaemonStatus = 'running' | 'stopped' | 'starting'

export interface DaemonProbe {
  status: DaemonStatus
  info?: ServerInfo
  menu?: MenuView
  error?: string
  /**
   * True only when the daemon answered with the nonce from server.json. Restart
   * and "already running" both key off this: a PID on its own proves nothing.
   */
  verified: boolean
}

const BROWSERS = ['Microsoft Edge', 'Google Chrome', 'Chromium', 'Brave Browser']

export function repoRoot(): string | undefined {
  let directory = import.meta.dirname
  for (let depth = 0; depth < 6; depth += 1) {
    if (existsSync(join(directory, 'pnpm-workspace.yaml'))) return directory
    const parent = dirname(directory)
    if (parent === directory) return undefined
    directory = parent
  }
  return undefined
}

/** The bundled daemon in a packaged app, the workspace command in development. */
function daemonCommand(): { command: string; args: string[]; cwd?: string } | undefined {
  const override = process.env.RELAY_DAEMON_COMMAND
  if (override) return { command: override, args: [] }
  const bundled = [
    join(import.meta.dirname, 'relayd', 'serve.js'),
    join(import.meta.dirname, '..', 'relayd', 'serve.js'),
  ].find((candidate) => existsSync(candidate))
  if (bundled) return { command: process.execPath, args: [bundled] }
  const root = repoRoot()
  return root ? { command: 'corepack', args: ['pnpm', '--dir', root, 'relayd'], cwd: root } : undefined
}

function installedBrowser(): string | undefined {
  const override = process.env.RELAY_BROWSER
  if (override) return override
  const roots = ['/Applications', join(app.getPath('home'), 'Applications')]
  for (const name of BROWSERS) {
    for (const root of roots) {
      if (existsSync(join(root, `${name}.app`))) return name
    }
  }
  return undefined
}

export function browserName(): string {
  return installedBrowser() ?? 'default browser'
}

/** Opens a new tab, preferring Chromium-family browsers over the default. */
export function openInBrowser(url: string): void {
  const browser = installedBrowser()
  if (browser) {
    spawnOpen(['-a', browser, url])
    return
  }
  void shell.openExternal(url)
}

/** `open` cannot fail in a way that should take the tray down; report it instead. */
function spawnOpen(args: string[]): void {
  try {
    const child = spawn('open', args, { detached: true, stdio: 'ignore' })
    child.on('error', (error) => process.stderr.write(`[relay] open failed: ${error.message}\n`))
    child.unref()
  } catch (error) {
    process.stderr.write(`[relay] open failed: ${error instanceof Error ? error.message : String(error)}\n`)
  }
}

export async function probeDaemon(): Promise<DaemonProbe> {
  const info = readServerInfo()
  if (!info) return { status: 'stopped', verified: false }
  const client = new RelayClient({ baseUrl: info.url, token: info.token })
  try {
    const [health, menu] = await Promise.all([client.health(), client.menu()])
    const verified = health.pid === info.pid && info.nonce.length > 0 && health.nonce === info.nonce
    if (!verified) {
      return { status: 'stopped', info, error: 'server.json 与运行的 daemon 不匹配（已过期）', verified: false }
    }
    return { status: 'running', info, menu, verified: true }
  } catch (error) {
    return {
      status: 'stopped',
      info,
      error: error instanceof Error ? error.message : String(error),
      verified: false,
    }
  }
}

/**
 * Starts the daemon detached so it outlives the tray. Failures are reported as a
 * string instead of an uncaught 'error' event, which would kill the menu bar —
 * the one surface the user has left.
 */
export function startDaemon(): string | undefined {
  const resolved = daemonCommand()
  if (!resolved) return '找不到 relayd 入口（既没有打包产物，也不在源码仓库里）'
  try {
    const child = spawn(resolved.command, resolved.args, {
      detached: true,
      stdio: 'ignore',
      env: process.env,
      ...(resolved.cwd ? { cwd: resolved.cwd } : {}),
    })
    child.on('error', (error) => {
      process.stderr.write(`[relay] relayd failed to start: ${error.message}\n`)
    })
    child.unref()
    return undefined
  } catch (error) {
    return error instanceof Error ? error.message : String(error)
  }
}

/** Only ever called for a daemon whose nonce was verified. */
export function stopDaemon(info: ServerInfo | undefined): void {
  if (!info) return
  try {
    process.kill(info.pid, 'SIGTERM')
  } catch {
    // Already gone; the next probe clears the record.
  }
}

export { inspectorUrl }
