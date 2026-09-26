import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'
import * as electronUpdater from 'electron-updater'
import type { AppUpdater, ProgressInfo, UpdateInfo } from 'electron-updater'

/**
 * Relay App updates, and only App updates.
 *
 * electron-updater drives download/verify/replace/relaunch from the signed
 * macOS build electron-builder produces; Relay does not implement any of it
 * (docs/updates.md). The Codex integration has its own lifecycle and is never
 * touched here: installing a new Relay does not mean the Codex MCP server, its
 * skill or its hooks were updated, and the integration checks say so on their
 * own (docs/codex-integration.md 2).
 */

/**
 * electron-updater is CJS without a default export, and touching autoUpdater
 * constructs an updater that reads app.getVersion() immediately. Resolve it
 * lazily, after the app is ready, so importing this module can never crash a
 * process that is not (yet) a full Electron main process.
 */
let resolved: AppUpdater | undefined
function autoUpdaterInstance(): AppUpdater {
  resolved ??= (electronUpdater as unknown as { autoUpdater: AppUpdater }).autoUpdater
  return resolved
}

export type UpdateStatus =
  | 'unsupported'
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'downloaded'
  | 'none'
  | 'error'

export interface UpdateState {
  status: UpdateStatus
  /** Version offered by the feed, when one is known. */
  version?: string
  /** Recent progress 0-100 while downloading. */
  percent?: number
  message?: string
}

let state: UpdateState = { status: 'idle' }
let wired = false

function publish(next: UpdateState): void {
  state = next
  listeners.forEach((listener) => listener(state))
}

const listeners = new Set<(state: UpdateState) => void>()

export function onUpdateState(listener: (state: UpdateState) => void): () => void {
  listeners.add(listener)
  listener(state)
  return () => listeners.delete(listener)
}

export function updateState(): UpdateState {
  return state
}

/**
 * A packaged, signed build reads app-update.yml. Development and unpacked builds
 * have no feed, so they report `unsupported` instead of pretending to check —
 * unless RELAY_UPDATE_FEED points at one, which is how the update path is tested
 * locally.
 */
export function updatesSupported(): boolean {
  if (process.env.RELAY_UPDATE_FEED) return true
  if (!app.isPackaged) return false
  // electron-builder only writes app-update.yml for builds with a publish
  // target. Without it there is no feed, and saying so beats reporting an error.
  return existsSync(join(process.resourcesPath, 'app-update.yml'))
}

function wire(): void {
  if (wired) return
  wired = true
  const autoUpdater = autoUpdaterInstance()
  autoUpdater.autoDownload = false
  autoUpdater.autoInstallOnAppQuit = true
  autoUpdater.logger = null as never
  if (process.env.RELAY_UPDATE_FEED) {
    autoUpdaterInstance().setFeedURL({ provider: 'generic', url: process.env.RELAY_UPDATE_FEED })
  }
  autoUpdater.on('checking-for-update', () => publish({ status: 'checking' }))
  autoUpdater.on('update-available', (info: UpdateInfo) => publish({ status: 'available', version: info.version }))
  autoUpdater.on('update-not-available', () => publish({ status: 'none' }))
  autoUpdater.on('download-progress', (progress: ProgressInfo) =>
    publish({ status: 'downloading', percent: Math.round(progress.percent) }),
  )
  autoUpdater.on('update-downloaded', (info: UpdateInfo) => publish({ status: 'downloaded', version: info.version }))
  autoUpdater.on('error', (error: Error) =>
    publish({ status: 'error', message: (error?.message ?? String(error)).slice(0, 200) }),
  )
}

export async function checkForUpdates(): Promise<UpdateState> {
  if (!updatesSupported()) {
    publish({ status: 'unsupported' })
    return state
  }
  wire()
  try {
    await autoUpdaterInstance().checkForUpdates()
  } catch (error) {
    publish({ status: 'error', message: (error instanceof Error ? error.message : String(error)).slice(0, 200) })
  }
  return state
}

export async function downloadUpdate(): Promise<UpdateState> {
  if (!updatesSupported()) return state
  wire()
  publish({ status: 'downloading', percent: 0 })
  try {
    await autoUpdaterInstance().downloadUpdate()
  } catch (error) {
    publish({ status: 'error', message: (error instanceof Error ? error.message : String(error)).slice(0, 200) })
  }
  return state
}

/** Quits, lets the updater replace the bundle, and relaunches. */
export function installUpdate(): void {
  if (state.status !== 'downloaded') return
  autoUpdaterInstance().quitAndInstall()
}
