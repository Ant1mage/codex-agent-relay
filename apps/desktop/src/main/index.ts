import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { app, BrowserWindow, ipcMain, shell } from 'electron'
import { DesktopDataSource } from './data-source.js'
import type { DesktopSettings } from '../shared/api.js'
import type { AgentProfile } from '@relay/protocol'

let dataSource: DesktopDataSource | undefined

// Icons are generated from the SVG masters in assets/app-icon by
// tools/build-icons.sh. Vite copies that directory into the renderer build
// (see publicDir in electron.vite.config.ts), so the packaged path mirrors it.
function iconFile(name: string): string {
  return app.isPackaged
    ? join(import.meta.dirname, `../renderer/app-icon/build/${name}`)
    : join(import.meta.dirname, `../../../../assets/app-icon/build/${name}`)
}

function createWindow(): void {
  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 980,
    minHeight: 680,
    title: 'Relay',
    icon: iconFile('appicon-256.png'),
    backgroundColor: '#0b0d10',
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

  if (process.platform === 'darwin') {
    // Largest raster: the Dock renders the mark well above 256pt on Retina.
    app.dock?.setIcon(iconFile('appicon-1024.png'))
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
