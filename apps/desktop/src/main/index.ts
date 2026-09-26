import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { app, BrowserWindow, clipboard, ipcMain, nativeTheme, shell } from 'electron'
import { createTranslator, resolveLocale } from '@relay/i18n'
import type { AgentProfile, Locale } from '@relay/protocol'
import type { DesktopNavigateRequest, DesktopSettings } from '../shared/api.js'
import { appIconPath } from './app-icon.js'
import { DesktopDataSource } from './data-source.js'
import { buildDiagnosticsReport } from './diagnostics.js'
import { MenuBar, type MenuBarHost } from './menu-bar.js'

const relayHome = join(homedir(), '.relay')
const databasePath = process.env.RELAY_DB_PATH ?? join(relayHome, 'relay.sqlite')
const settingsPath = process.env.RELAY_SETTINGS_PATH ?? join(relayHome, 'settings.json')

/** The window polls every two seconds; a shared projection serves both readers. */
const SNAPSHOT_TTL_MS = 500
/** Probing Codex spawns a process, so the menu bar accepts a minute-old answer. */
const CODEX_MENU_TTL_MS = 60_000

let dataSource: DesktopDataSource | undefined
let menuBar: MenuBar | undefined
let mainWindow: BrowserWindow | undefined
let pendingNavigation: DesktopNavigateRequest | undefined
/** Set while quitting so the window's close handler stops hiding the app. */
let isQuitting = false
/**
 * The menu bar follows the language chosen in the window and the system language
 * until then, so the native menu and the app never disagree about which language
 * the user reads.
 */
let menuLocale: Locale = resolveLocale(app.getLocale())

function createWindow(): BrowserWindow {
  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 980,
    minHeight: 680,
    title: 'Relay',
    // Windows and Linux read the window icon. macOS does not: its application
    // icon belongs to the .app bundle, so a BrowserWindow icon would be wrong.
    ...(process.platform !== 'darwin' ? { icon: appIconPath(256) } : {}),
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

  /*
   * Relay is a menu bar app (docs/menu-bar.md): closing the window keeps the
   * process, the worker supervision and the menu bar alive, which is what makes
   * "close the window" different from "quit" on macOS. Quit stays available from
   * the menu bar and from Cmd+Q while the window is focused.
   */
  window.on('close', (event) => {
    // Without a menu bar icon there would be no way back to a hidden window, so
    // a failed tray setup degrades to the ordinary "close ends the app" rule.
    if (process.platform === 'darwin' && !isQuitting && menuBar?.isInstalled) {
      event.preventDefault()
      window.hide()
    }
  })
  window.on('closed', () => {
    if (mainWindow === window) mainWindow = undefined
  })
  window.webContents.on('did-finish-load', () => {
    if (!pendingNavigation) return
    const request = pendingNavigation
    pendingNavigation = undefined
    window.webContents.send('relay:navigate', request)
  })

  if (process.env.ELECTRON_RENDERER_URL) {
    void window.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    void window.loadFile(join(import.meta.dirname, '../renderer/index.html'))
  }
  return window
}

/** A single navigation request, queued when the renderer is not listening yet. */
function sendToWindow(request: DesktopNavigateRequest): void {
  const window = mainWindow
  if (!window || window.isDestroyed() || window.webContents.isLoading()) {
    pendingNavigation = { ...pendingNavigation, ...request }
    return
  }
  window.webContents.send('relay:navigate', request)
}

/** Shows the window, optionally pointing it at a session, run or settings page. */
function showPanel(request: DesktopNavigateRequest = {}): void {
  if (mainWindow && !mainWindow.isDestroyed()) {
    if (mainWindow.isMinimized()) mainWindow.restore()
    if (!mainWindow.isVisible()) mainWindow.show()
  } else {
    mainWindow = createWindow()
  }
  if (Object.keys(request).length > 0) sendToWindow(request)
  mainWindow.focus()
  // With no Dock icon there is no app to activate through, so take focus
  // directly (accessory activation policy, docs/menu-bar.md).
  if (process.platform === 'darwin') app.focus({ steal: true })
}

/**
 * macOS accessory mode is the Clash-style "menu bar only" presentation: the
 * icon stays, the Dock icon and the app menu go away. It is presentation only —
 * the same window, the same data, the same projection.
 */
function applyDockPolicy(hideDockIcon: boolean): void {
  if (process.platform !== 'darwin') return
  app.setActivationPolicy(hideDockIcon ? 'accessory' : 'regular')
  // A development build brands itself at runtime; switching back to a regular
  // app recreates the Dock tile, so re-apply the Relay icon (see below).
  if (!hideDockIcon && !app.isPackaged) app.dock?.setIcon(appIconPath(1024))
}

/** Launch at login is the OS's state, never a copy Relay keeps in a file. */
function launchAtLogin(): boolean {
  return app.getLoginItemSettings().openAtLogin
}

function setLaunchAtLogin(enabled: boolean): void {
  app.setLoginItemSettings({ openAtLogin: enabled })
}

void app.whenReady().then(async () => {
  mkdirSync(dirname(databasePath), { recursive: true })
  mkdirSync(dirname(settingsPath), { recursive: true })
  const source = new DesktopDataSource(databasePath, settingsPath)
  dataSource = source
  await source.initialize()

  // The menu bar's presentation switch decides whether Relay is a regular app
  // (Dock icon plus app menu) or a menu bar accessory (docs/menu-bar.md).
  const menuBarPrefs = source.settings().menuBar
  applyDockPolicy(menuBarPrefs.hideDockIcon)
  // Development only. A packaged macOS build must take its icon from the .app
  // bundle (.icns via packaging config), not from a runtime dock call; Relay has
  // no packaging configuration yet, so this exists purely to brand `electron-vite
  // dev` runs. The 1024px export is used because the Dock draws well above 256pt
  // on Retina.
  if (process.platform === 'darwin' && !app.isPackaged && !menuBarPrefs.hideDockIcon) {
    app.dock?.setIcon(appIconPath(1024))
  }

  ipcMain.handle('relay:snapshot', () => source.snapshot(SNAPSHOT_TTL_MS))
  ipcMain.handle('relay:settings:save', (_event, settings: DesktopSettings) =>
    source.saveSettings(settings),
  )
  ipcMain.handle('relay:profile:save', (_event, profile: AgentProfile) => source.saveProfile(profile))
  ipcMain.handle('relay:worker:cancel', (_event, workerSessionId: string) => {
    const result = source.cancelWorker(workerSessionId)
    void menuBar?.refresh(true)
    return result
  })
  ipcMain.handle('relay:session:cancel', (_event, hostSessionId: string) => {
    const result = source.cancelSessionWorkers(hostSessionId)
    void menuBar?.refresh(true)
    return result
  })
  ipcMain.handle('relay:codex:status', () => source.codexIntegration())
  ipcMain.handle('relay:codex:install', async () => {
    const result = await source.installCodexIntegration()
    void menuBar?.refresh(true)
    return result
  })
  ipcMain.handle('relay:runtime:options', (_event, runtimeId: string) =>
    source.runtimeOptions(runtimeId),
  )
  ipcMain.handle('relay:onboarding:complete', () => source.completeOnboarding())
  ipcMain.handle('relay:workspace:open', async (_event, path: string) => {
    // Only ever the cwd Relay recorded for a session, never renderer-supplied paths.
    const allowed = source.snapshot(0).sessions.some((session) => session.cwd === path)
    if (!allowed) return { ok: false, message: 'Unknown workspace' }
    const failure = await shell.openPath(path)
    return failure ? { ok: false, message: failure } : { ok: true }
  })
  // The menu bar owns the language of its own labels; the window reports changes
  // so a user who switched to Chinese does not get an English tray (menu-bar.md).
  ipcMain.on('relay:locale:set', (_event, locale: Locale) => {
    menuLocale = locale === 'zh-CN' ? 'zh-CN' : 'en'
    void menuBar?.refresh(true)
  })

  async function openWorkspace(cwd: string): Promise<void> {
    const allowed = source.snapshot(0).sessions.some((session) => session.cwd === cwd)
    if (allowed) await shell.openPath(cwd)
  }

  async function copyDiagnostics(): Promise<void> {
    const codex = await source.codexIntegration(CODEX_MENU_TTL_MS)
    const report = buildDiagnosticsReport({
      appVersion: app.getVersion(),
      electronVersion: process.versions.electron ?? 'unknown',
      chromeVersion: process.versions.chrome ?? 'unknown',
      nodeVersion: process.versions.node,
      platform: process.platform,
      arch: process.arch,
      databasePath,
      settingsPath,
      snapshot: source.snapshot(0),
      codex,
      menuBar: {
        hideDockIcon: source.settings().menuBar.hideDockIcon,
        launchAtLogin: launchAtLogin(),
      },
    })
    clipboard.writeText(report)
    sendToWindow({ notice: createTranslator(menuLocale)('menu.diagnosticsCopied') })
  }

  const host: MenuBarHost = {
    snapshot: () => source.snapshot(SNAPSHOT_TTL_MS),
    codex: () => source.codexIntegration(CODEX_MENU_TTL_MS),
    locale: () => menuLocale,
    menuBarPrefs: () => source.settings().menuBar,
    launchAtLogin,
    openPanel: showPanel,
    cancelWorker: (workerSessionId) => {
      source.cancelWorker(workerSessionId)
      void menuBar?.refresh(true)
    },
    cancelSession: (hostSessionId) => {
      source.cancelSessionWorkers(hostSessionId)
      void menuBar?.refresh(true)
    },
    openWorkspace: (cwd) => void openWorkspace(cwd),
    copySessionId: (hostSessionId) => {
      clipboard.writeText(hostSessionId)
      sendToWindow({ notice: createTranslator(menuLocale)('session.idCopied') })
    },
    copyDiagnostics: () => void copyDiagnostics(),
    installCodex: () => {
      void source.installCodexIntegration().then(() => menuBar?.refresh(true))
    },
    setHideDockIcon: (hidden) => {
      source.saveMenuBarPrefs({ hideDockIcon: hidden })
      applyDockPolicy(hidden)
    },
    setLaunchAtLogin,
    quit: () => {
      isQuitting = true
      app.quit()
    },
  }
  menuBar = new MenuBar(host)
  await menuBar.start()

  mainWindow = createWindow()
  app.on('activate', () => showPanel())
})

app.on('before-quit', () => {
  isQuitting = true
  menuBar?.destroy()
  dataSource?.close()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
