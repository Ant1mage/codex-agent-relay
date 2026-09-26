import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { app, BrowserWindow, ipcMain, nativeTheme, shell } from 'electron'
import { DesktopDataSource } from './data-source.js'
import type { DesktopSettings } from '../shared/api.js'
import type { AgentProfile } from '@relay/protocol'

let dataSource: DesktopDataSource | undefined

/**
 * Canonical Relay icon exports, in assets/appicon/png/light.
 *
 * These PNGs are the design deliverables and are used as-is; Relay never
 * re-rasterises the SVG to produce platform rasters. Vite copies assets/ into
 * the renderer output (see publicDir in electron.vite.config.ts), so the
 * packaged path mirrors the source tree.
 */
function iconPng(size: 256 | 1024): string {
  const relative = `appicon/png/light/relay-icon-${size}.png`
  return app.isPackaged
    ? join(import.meta.dirname, '../renderer', relative)
    : join(import.meta.dirname, '../../../../assets', relative)
}

function createWindow(): void {
  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 980,
    minHeight: 680,
    title: 'Relay',
    // Windows and Linux read the window icon. macOS does not: its application
    // icon belongs to the .app bundle, so a BrowserWindow icon would be wrong.
    ...(process.platform !== 'darwin' ? { icon: iconPng(256) } : {}),
    // Relay is light-theme-first; the window paints this before the renderer
    // loads, so a dark value would flash black on launch (docs/ui.md 20).
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#1c1d1f' : '#f4f4f3',
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    webPreferences: {
      preload: join(import.meta.dirname, '../preload/index.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })

  if (process.env.ELECTRON_RENDERER_URL) {
    void window.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    void window.loadFile(join(import.meta.dirname, '../renderer/index.html'))
  }
}

void app.whenReady().then(async () => {
  const databasePath = process.env.RELAY_DB_PATH ?? join(homedir(), '.relay', 'relay.sqlite')
  const settingsPath = process.env.RELAY_SETTINGS_PATH ?? join(homedir(), '.relay', 'settings.json')
  mkdirSync(dirname(databasePath), { recursive: true })
  mkdirSync(dirname(settingsPath), { recursive: true })
  dataSource = new DesktopDataSource(databasePath, settingsPath)
  await dataSource.initialize()

  // Development only. A packaged macOS build must take its icon from the .app
  // bundle (.icns via packaging config), not from a runtime dock call; Relay has
  // no packaging configuration yet, so this exists purely to brand `electron-vite
  // dev` runs. The 1024px export is used because the Dock draws well above 256pt
  // on Retina.
  if (process.platform === 'darwin' && !app.isPackaged) {
    app.dock?.setIcon(iconPng(1024))
  }

  ipcMain.handle('relay:snapshot', () => dataSource?.snapshot())
  ipcMain.handle('relay:settings:save', (_event, settings: DesktopSettings) =>
    dataSource?.saveSettings(settings),
  )
  ipcMain.handle('relay:profile:save', (_event, profile: AgentProfile) =>
    dataSource?.saveProfile(profile),
  )
  ipcMain.handle('relay:worker:cancel', (_event, workerSessionId: string) =>
    dataSource?.cancelWorker(workerSessionId),
  )
  ipcMain.handle('relay:session:cancel', (_event, hostSessionId: string) =>
    dataSource?.cancelSessionWorkers(hostSessionId),
  )
  ipcMain.handle('relay:codex:status', () => dataSource?.codexIntegration())
  ipcMain.handle('relay:runtime:options', (_event, runtimeId: string) =>
    dataSource?.runtimeOptions(runtimeId),
  )
  ipcMain.handle('relay:onboarding:complete', () => dataSource?.completeOnboarding())
  ipcMain.handle('relay:workspace:open', async (_event, path: string) => {
    // Only ever the cwd Relay recorded for a session, never renderer-supplied paths.
    const allowed = dataSource?.snapshot().sessions.some((session) => session.cwd === path)
    if (!allowed) return { ok: false, message: 'Unknown workspace' }
    const failure = await shell.openPath(path)
    return failure ? { ok: false, message: failure } : { ok: true }
  })

  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

app.on('before-quit', () => dataSource?.close())
