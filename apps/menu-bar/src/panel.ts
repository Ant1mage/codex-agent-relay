import { join } from 'node:path'
import { BrowserWindow, nativeTheme, screen, type Tray } from 'electron'
import {
  panelConnectionChanged,
  panelUrl,
  type PanelIntent,
  type PanelTab,
  type PanelTarget,
} from './panel-target.js'
import { panelBackgroundColor } from './panel-theme.js'

export { panelUrl, type PanelIntent, type PanelTab, type PanelTarget } from './panel-target.js'

/**
 * Relay's control panel: a frameless window anchored under the menu bar icon.
 *
 * Configuration needs real controls (text fields, selects, sliders, switches),
 * which a native NSMenu cannot carry — so the menu keeps status and quick
 * actions and this panel carries the forms. It is served by relayd, which makes
 * the panel same-origin with the API: no file:// module restrictions and no
 * special case in the daemon's Origin guard.
 */

let panel: BrowserWindow | undefined
let panelTarget: PanelTarget | undefined

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
    const connectionChanged = panelConnectionChanged(panelTarget, target)
    panelTarget = target
    if (connectionChanged) {
      // relayd rotates its token on every restart. A hidden BrowserWindow keeps
      // its renderer alive, so reload it with the new fragment before showing.
      panel.hide()
      void panel
        .loadURL(panelUrl(target))
        .then(() => {
          if (!panel || panel.isDestroyed()) return
          placeUnder(tray, panel)
          panel.show()
          panel.focus()
        })
        .catch((error: unknown) => {
          console.error('[relay] failed to reload panel after daemon restart', error)
        })
      return
    }
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
  panelTarget = target
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
    backgroundColor: panelBackgroundColor(nativeTheme.shouldUseDarkColors),
    webPreferences: {
      preload: join(import.meta.dirname, 'panel-preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })
  panel.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true })
  // The panel never owns browser tabs. External navigation is dispatched by
  // the main process so it cannot accidentally create another BrowserWindow.
  panel.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  // The panel behaves like a popover: clicking anywhere else dismisses it.
  panel.on('blur', () => panel?.hide())
  const syncBackgroundColor = () => {
    if (panel && !panel.isDestroyed()) panel.setBackgroundColor(panelBackgroundColor(nativeTheme.shouldUseDarkColors))
  }
  nativeTheme.on('updated', syncBackgroundColor)
  panel.on('closed', () => {
    nativeTheme.off('updated', syncBackgroundColor)
    panel = undefined
    panelTarget = undefined
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
  panelTarget = undefined
}

export function panelIsOpen(): boolean {
  return Boolean(panel && !panel.isDestroyed() && panel.isVisible())
}
