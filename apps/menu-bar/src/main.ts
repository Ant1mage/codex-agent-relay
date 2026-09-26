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
import { openPanel } from './panel.js'
import {
  buildMenuBarItems,
  menuBarStatusLabel,
  type MenuBarAction,
  type MenuBarItem,
  type MenuBarPlatform,
  type MenuBarView,
} from './menu-model.js'

/**
 * Relay's desktop surface: one menu bar item plus the control panel it opens.
 *
 * There is no application window and no inspector HTML here — the menu carries
 * status and quick actions, the panel carries the forms (Agent Profiles, policy,
 * Codex integration, runtimes), and clicking a session opens the log viewer in
 * Edge/Chrome. Everything the tray shows or changes goes through relayd.
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
let probe: DaemonProbe = { status: 'stopped', verified: false }
/** Last failure worth showing in the menu (spawn, panel, MCP action). */
let lastError: string | undefined

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
    ...(lastError ?? probe.error ? { error: lastError ?? probe.error } : {}),
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
    if (probe.status === 'running') {
      startingSince = 0
      lastError = undefined
    }
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

function openControlPanel(tab: 'agents' | 'policy' | 'codex' | 'runtime'): void {
  const info: ServerInfo | undefined = probe.info
  if (!info || !tray) {
    lastError = 'daemon 未运行，无法打开配置面板'
    void refresh(true)
    return
  }
  try {
    openPanel(tray, {
      base: info.url,
      token: info.token,
      lang: resolveLocale(app.getLocale()),
      tab,
    })
  } catch (error) {
    lastError = error instanceof Error ? error.message : String(error)
    void refresh(true)
  }
}

/** Runs an MCP-side action (Codex lifecycle, rescan) and surfaces the outcome. */
function runClientAction(
  action: (relay: RelayClient) => Promise<unknown>,
  label: string,
): void {
  const relay = client()
  if (!relay) {
    lastError = `${label}: daemon 未运行`
    void refresh(true)
    return
  }
  void action(relay)
    .then((result) => {
      const messages = (result as { messages?: string[] } | undefined)?.messages
      lastError = messages && messages.length > 0 ? messages[0] : undefined
      return refresh(true)
    })
    .catch((error: unknown) => {
      lastError = `${label}: ${error instanceof Error ? error.message : String(error)}`
      return refresh(true)
    })
}

function dispatch(action: MenuBarAction): void {
  switch (action.type) {
    case 'open-inspector':
      openInspector(action.hostSessionId, action.runId)
      return
    case 'open-panel':
      openControlPanel(action.tab)
      return
    case 'rescan':
      runClientAction((relay) => relay.refresh(), 'rescan')
      return
    case 'repair-codex':
      runClientAction((relay) => relay.codex('repair'), 'codex repair')
      return
    case 'start-daemon': {
      startingSince = Date.now()
      lastError = startDaemon()
      void refresh(true)
      return
    }
    case 'restart-daemon': {
      startingSince = Date.now()
      // Only a daemon whose identity was verified is ever signalled.
      if (probe.verified) stopDaemon(probe.info)
      setTimeout(() => {
        lastError = startDaemon()
        void refresh(true)
      }, 600)
      void refresh(true)
      return
    }
    case 'cancel-worker':
      runClientAction((relay) => relay.cancelWorker(action.workerSessionId), 'cancel')
      return
    case 'cancel-session':
      runClientAction((relay) => relay.cancelSession(action.hostSessionId), 'cancel session')
      return
    case 'open-workspace':
      void shell.openPath(action.cwd)
      return
    case 'copy-session-id':
      clipboard.writeText(action.hostSessionId)
      return
    case 'copy-diagnostics':
      runClientAction(async (relay) => {
        clipboard.writeText(await relay.diagnostics())
      }, 'diagnostics')
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
 * (docs/menu-bar.md 8). A packaged build never opens a menu on its own.
 */
function previewIfRequested(): void {
  if (app.isPackaged) return
  const delay = Number(process.env.RELAY_MENU_BAR_PREVIEW_DELAY_MS ?? 1_500)
  if (process.env.RELAY_MENU_BAR_PREVIEW === '1') {
    setTimeout(() => tray?.popUpContextMenu(), Number.isFinite(delay) ? delay : 1_500)
  }
  if (process.env.RELAY_PANEL_PREVIEW === '1') {
    const panelDelay = Number(process.env.RELAY_PANEL_PREVIEW_DELAY_MS ?? 1_200)
    setTimeout(
      () => openControlPanel((process.env.RELAY_PANEL_PREVIEW_TAB as 'agents') ?? 'agents'),
      Number.isFinite(panelDelay) ? panelDelay : 1_200,
    )
  }
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
  tray.on('double-click', () => openInspector(probe.menu?.sessions[0]?.id))
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
