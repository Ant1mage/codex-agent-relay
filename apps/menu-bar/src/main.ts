import { app, clipboard, Menu, nativeImage, shell, Tray, type MenuItemConstructorOptions } from 'electron'
import { createTranslator, resolveLocale } from '@relay/i18n'
import { RelayClient } from '@relay/relay-api'
import type { ServerInfo } from '@relay/relay-api/server-info'
import { trayIconPath } from './app-icon.js'
import {
  browserName,
  inspectorUrl,
  openInBrowser,
  probeDaemon,
  startDaemon,
  stopDaemon,
  type DaemonProbe,
} from './daemon.js'
import {
  buildMenuBarItems,
  menuBarStatusLabel,
  type MenuBarAction,
  type MenuBarItem,
  type MenuBarPlatform,
  type MenuBarView,
} from './menu-model.js'

/**
 * Relay's whole desktop presence: one menu bar item.
 *
 * There is no window, no renderer and no HTML here — clicking a session opens
 * the inspector in Edge/Chrome, which is the web app served by apps/relayd
 * (docs/inspector.md). The tray only reads the daemon's projection and reports
 * what the user picked.
 */

/** Matches the inspector's poll; both read the same cached projection server-side. */
const POLL_MS = 2_000
/** How long "starting" may last before the menu admits the daemon did not come up. */
const START_TIMEOUT_MS = 20_000

let tray: Tray | undefined
let timer: NodeJS.Timeout | undefined
/** Serialized previous model, so an unchanged menu is not rebuilt. */
let lastModel = ''
let busy = false
let startingSince = 0
let probe: DaemonProbe = { status: 'stopped' }

function trayIcon() {
  const image = nativeImage.createFromPath(trayIconPath(16))
  const retina = nativeImage.createFromPath(trayIconPath(32))
  if (!retina.isEmpty()) image.addRepresentation({ scaleFactor: 2, buffer: retina.toPNG() })
  image.setTemplateImage(true)
  return image
}

function toTemplateItem(item: MenuBarItem, dispatch: (action: MenuBarAction) => void): MenuItemConstructorOptions {
  if (item.kind === 'separator') return { type: 'separator' }
  if (item.kind === 'header') return { label: item.label, enabled: false }
  const template: MenuItemConstructorOptions = { label: item.label, enabled: item.enabled ?? true }
  if (item.accelerator) template.accelerator = item.accelerator
  if (item.submenu) {
    template.submenu = item.submenu.map((child) => toTemplateItem(child, dispatch))
    return template
  }
  if (item.kind === 'checkbox') {
    template.type = 'checkbox'
    template.checked = item.checked ?? false
  }
  if (item.action) {
    const action = item.action
    template.click = () => dispatch(action)
  }
  return template
}

function client(): RelayClient | undefined {
  const info = probe.info
  return info ? new RelayClient({ baseUrl: info.url, token: info.token }) : undefined
}

function currentStatus(): DaemonProbe['status'] {
  if (probe.status === 'running') return 'running'
  if (startingSince > 0 && Date.now() - startingSince < START_TIMEOUT_MS) return 'starting'
  return 'stopped'
}

function view(): MenuBarView {
  const status = currentStatus()
  return {
    locale: resolveLocale(app.getLocale()),
    platform: process.platform as MenuBarPlatform,
    daemon: status,
    ...(status === 'running' && probe.menu ? { menu: probe.menu } : {}),
    ...(probe.error ? { error: probe.error } : {}),
    launchAtLogin: app.getLoginItemSettings().openAtLogin,
    browser: browserName(),
  }
}

/** Rebuilds the native menu when anything it shows changed. */
async function refresh(force = false): Promise<void> {
  if (!tray || busy) return
  busy = true
  try {
    probe = await probeDaemon()
    if (probe.status === 'running') startingSince = 0
    const model = view()
    const serialized = JSON.stringify(model)
    if (!force && serialized === lastModel) return
    lastModel = serialized
    const items = buildMenuBarItems(model)
    tray.setToolTip(menuBarStatusLabel(model))
    tray.setContextMenu(
      Menu.buildFromTemplate(items.map((item) => toTemplateItem(item, (action) => dispatch(action)))),
    )
  } finally {
    busy = false
  }
}

function openInspector(hostSessionId?: string, runId?: string): void {
  const info: ServerInfo | undefined = probe.info
  if (!info) return
  openInBrowser(inspectorUrl(info, hostSessionId, runId))
}

function dispatch(action: MenuBarAction): void {
  switch (action.type) {
    case 'open-inspector':
      openInspector(action.hostSessionId, action.runId)
      return
    case 'start-daemon':
      startingSince = Date.now()
      startDaemon()
      void refresh(true)
      return
    case 'restart-daemon':
      startingSince = Date.now()
      stopDaemon(probe.info)
      setTimeout(() => {
        startDaemon()
        void refresh(true)
      }, 600)
      void refresh(true)
      return
    case 'cancel-worker':
      void client()?.cancelWorker(action.workerSessionId).then(() => refresh(true)).catch(() => undefined)
      return
    case 'cancel-session':
      void client()?.cancelSession(action.hostSessionId).then(() => refresh(true)).catch(() => undefined)
      return
    case 'open-workspace':
      void shell.openPath(action.cwd)
      return
    case 'copy-session-id':
      clipboard.writeText(action.hostSessionId)
      return
    case 'copy-diagnostics':
      void client()
        ?.diagnostics()
        .then((report) => clipboard.writeText(report))
        .catch((error: unknown) => {
          clipboard.writeText(`Relay diagnostics unavailable: ${error instanceof Error ? error.message : String(error)}`)
        })
      return
    case 'install-codex':
      void client()?.installCodex().then(() => refresh(true)).catch(() => undefined)
      return
    case 'refresh':
      void refresh(true)
      return
    case 'toggle-launch-at-login':
      app.setLoginItemSettings({ openAtLogin: !app.getLoginItemSettings().openAtLogin })
      void refresh(true)
      return
    case 'quit':
      app.quit()
      return
  }
}

/**
 * Development-only hook used to capture the open menu in screenshots
 * (docs/menu-bar.md "验证"). A packaged build never opens a menu on its own.
 */
function previewIfRequested(): void {
  if (app.isPackaged || process.env.RELAY_MENU_BAR_PREVIEW !== '1') return
  const delay = Number(process.env.RELAY_MENU_BAR_PREVIEW_DELAY_MS ?? 1_500)
  setTimeout(() => tray?.popUpContextMenu(), Number.isFinite(delay) ? delay : 1_500)
}

void app.whenReady().then(async () => {
  // No window means no Dock tile and no app menu: the menu bar is the app.
  if (process.platform === 'darwin') app.setActivationPolicy('accessory')
  const icon = trayIcon()
  if (icon.isEmpty()) {
    console.warn('[relay] menu bar icon is missing; Relay cannot show a menu bar item')
    app.quit()
    return
  }
  tray = new Tray(icon)
  tray.on('double-click', () => {
    const session = probe.menu?.sessions[0]?.id
    openInspector(session)
  })
  await refresh(true)
  timer = setInterval(() => void refresh(), POLL_MS)
  timer.unref?.()
  previewIfRequested()
})

app.on('before-quit', () => {
  if (timer) clearInterval(timer)
  tray?.destroy()
  tray = undefined
})

// The tray owns the process lifetime; there are no windows to close it.
app.on('window-all-closed', () => undefined)

const translator = () => createTranslator(resolveLocale(app.getLocale()))
export { translator }
