import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { app, shell } from 'electron'
import type { MenuView } from '@relay/relay-api'
import { RelayClient } from '@relay/relay-api'
import { inspectorUrl, readServerInfo, type ServerInfo } from '@relay/relay-api/server-info'

/**
 * Everything the tray knows about the daemon. The tray is a client: it reads
 * ~/.relay/server.json, asks the daemon for the projection, and can start the
 * daemon and open the inspector — it never touches the database itself.
 */

export type DaemonStatus = 'running' | 'stopped' | 'starting'

export interface DaemonProbe {
  status: DaemonStatus
  info?: ServerInfo
  menu?: MenuView
  error?: string
}

/** Browsers the inspector prefers, in the order the tray will try them. */
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
    spawn('open', ['-a', browser, url], { detached: true, stdio: 'ignore' }).unref()
    return
  }
  void shell.openExternal(url)
}

export async function probeDaemon(): Promise<DaemonProbe> {
  const info = readServerInfo()
  if (!info) return { status: 'stopped' }
  const client = new RelayClient({ baseUrl: info.url, token: info.token })
  try {
    const [health, menu] = await Promise.all([client.health(), client.menu()])
    if (health.pid !== info.pid) return { status: 'stopped', error: 'Stale server.json' }
    return { status: 'running', info, menu }
  } catch (error) {
    return { status: 'stopped', info, error: error instanceof Error ? error.message : String(error) }
  }
}

/**
 * Starts the daemon detached so it outlives the tray. In development that is the
 * workspace command; a packaged build ships the compiled daemon next to the app.
 */
export function startDaemon(): void {
  const command = process.env.RELAY_DAEMON_COMMAND
  const root = repoRoot()
  const child = command
    ? spawn(command, [], { detached: true, stdio: 'ignore', env: process.env })
    : app.isPackaged
      ? spawn(process.execPath, [join(import.meta.dirname, 'relayd.js')], {
          detached: true,
          stdio: 'ignore',
          env: process.env,
        })
      : root
        ? spawn('corepack', ['pnpm', '--dir', root, 'relayd'], {
            detached: true,
            stdio: 'ignore',
            cwd: root,
            env: process.env,
          })
        : undefined
  child?.unref()
}

export function stopDaemon(info: ServerInfo | undefined): void {
  if (!info) return
  try {
    process.kill(info.pid, 'SIGTERM')
  } catch {
    // Already gone; the next probe will clear the file.
  }
}

export { inspectorUrl }
