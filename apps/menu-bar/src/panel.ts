import { join } from 'node:path'
import { BrowserWindow, screen, type Tray } from 'electron'

/**
 * Relay's control panel: a frameless window anchored under the menu bar icon.
 *
 * Configuration needs real controls (text fields, selects, sliders, switches),
 * which a native NSMenu cannot carry — so the menu keeps status and quick
 * actions and this panel carries the forms. It is served by relayd, which makes
 * the panel same-origin with the API: no file:// module restrictions and no
 * special case in the daemon's Origin guard (docs/menu-bar.md 6).
 */

let panel: BrowserWindow | undefined

export type PanelTab = 'agents' | 'policy' | 'codex' | 'runtime'
export type PanelIntent = 'new-agent' | 'edit-agent' | 'add-runtime' | 'codex-actions'

export interface PanelTarget {
  /** Daemon base URL, e.g. http://127.0.0.1:7352 */
  base: string
  token: string
  lang: string
  tab?: PanelTab
  intent?: PanelIntent
  profileId?: string
}

export function panelUrl(target: PanelTarget): string {
  const dev = process.env.RELAY_PANEL_DEV_URL
  const url = new URL(dev ?? `${target.base}/panel/`)
  if (dev) url.searchParams.set('base', target.base)
  url.searchParams.set('lang', target.lang)
  if (target.tab) url.searchParams.set('tab', target.tab)
  if (target.intent) url.searchParams.set('intent', target.intent)
  if (target.profileId) url.searchParams.set('profileId', target.profileId)
  // The token rides the fragment: it never reaches the server or a referrer.
  url.hash = `t=${target.token}`
  return url.toString()
}

function placeUnder(tray: Tray, window: BrowserWindow): void {
  const bounds = tray.getBounds()
  const { width, height } = window.getBounds()
  const display = screen.getDisplayNearestPoint({ x: bounds.x, y: bounds.y }).workArea
  const x = Math.min(
    Math.max(Math.round(bounds.x + bounds.width / 2 - width / 2), display.x + 8),
    display.x + display.width - width - 8,
  )
  const y = Math.min(bounds.y + bounds.height + 6, display.y + display.height - height - 8)
  window.setPosition(x, y, false)
}

export function openPanel(tray: Tray, target: PanelTarget): void {
  if (panel && !panel.isDestroyed()) {
    // An open panel navigates in place: same window, same state, no reload.
    panel.webContents.send('relay:panel', {
      tab: target.tab,
      intent: target.intent,
      profileId: target.profileId,
    })
    placeUnder(tray, panel)
    panel.show()
    panel.focus()
    return
  }
  const url = panelUrl(target)
  panel = new BrowserWindow({
    width: 420,
    height: 640,
    show: false,
    frame: false,
    resizable: true,
    maximizable: false,
    minimizable: false,
    fullscreenable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    backgroundColor: '#1c1d1f',
    webPreferences: {
      preload: join(import.meta.dirname, 'panel-preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })
  panel.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true })
  // The panel behaves like a popover: clicking anywhere else dismisses it.
  panel.on('blur', () => panel?.hide())
  panel.on('closed', () => {
    panel = undefined
  })
  placeUnder(tray, panel)
  panel.once('ready-to-show', () => {
    if (!panel || panel.isDestroyed()) return
    placeUnder(tray, panel)
    panel.show()
    panel.focus()
  })
  void panel.loadURL(url)
}

export function closePanel(): void {
  panel?.destroy()
  panel = undefined
}

export function panelIsOpen(): boolean {
  return Boolean(panel && !panel.isDestroyed() && panel.isVisible())
}
