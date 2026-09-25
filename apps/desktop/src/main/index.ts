import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { app, BrowserWindow, ipcMain } from 'electron'
import { DesktopDataSource } from './data-source.js'
import type { DesktopSettings } from '../shared/api.js'
import type { AgentProfile } from '@relay/protocol'

let dataSource: DesktopDataSource | undefined

function createWindow(): void {
  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 980,
    minHeight: 680,
    title: 'Relay',
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

  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

app.on('before-quit', () => dataSource?.close())
