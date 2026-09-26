import { Menu, Tray, app, nativeImage, type MenuItemConstructorOptions } from 'electron'
import type { Locale } from '@relay/protocol'
import type {
  CodexIntegrationStatus,
  DesktopMenuBarPrefs,
  DesktopNavigateRequest,
  DesktopSnapshot,
} from '../shared/api.js'
import { appIconPath } from './app-icon.js'
import {
  buildMenuBarItems,
  menuBarStatusLabel,
  menuBarViewFromSnapshot,
  type MenuBarAction,
  type MenuBarItem,
  type MenuBarPlatform,
} from './menu-bar-model.js'

/** How often the menu re-reads the projection. Matches the window's poll. */
const REFRESH_INTERVAL_MS = 2_000
/** Probing Codex spawns a process, so the menu accepts a minute-old answer. */
const CODEX_CACHE_MS = 60_000

/**
 * Everything the menu bar needs from the app. Keeping this an interface means
 * the tray knows nothing about SQLite, adapters or IPC: it renders a model and
 * reports what the user picked.
 */
export interface MenuBarHost {
  /** One projection revision, shared with the window (short-lived cache). */
  snapshot(): DesktopSnapshot
  /** Cached Codex integration status; the menu must not spawn probes per poll. */
  codex(): Promise<CodexIntegrationStatus>
  locale(): Locale
  menuBarPrefs(): DesktopMenuBarPrefs
  launchAtLogin(): boolean
  openPanel(request: DesktopNavigateRequest): void
  cancelWorker(workerSessionId: string): void
  cancelSession(hostSessionId: string): void
  openWorkspace(cwd: string): void
  copySessionId(hostSessionId: string): void
  copyDiagnostics(): void
  installCodex(): void
  setHideDockIcon(hidden: boolean): void
  setLaunchAtLogin(enabled: boolean): void
  quit(): void
}

/**
 * macOS renders menu bar icons as templates: only the alpha channel is used, so
 * one asset follows both light and dark menu bars. The canonical light export is
 * already a dark mark on transparency, which is exactly a template's mask, so
 * there is no separate tray asset and no icon build step
 * (assets/appicon/README.md).
 */
function trayIcon() {
  const image = nativeImage.createFromPath(appIconPath(16))
  const retina = nativeImage.createFromPath(appIconPath(32))
  if (!retina.isEmpty()) {
    image.addRepresentation({ scaleFactor: 2, buffer: retina.toPNG() })
  }
  image.setTemplateImage(true)
  return image
}

function toTemplateItem(item: MenuBarItem, dispatch: (action: MenuBarAction) => void): MenuItemConstructorOptions {
  if (item.kind === 'separator') return { type: 'separator' }
  if (item.kind === 'header') return { label: item.label, enabled: false }
  const template: MenuItemConstructorOptions = {
    label: item.label,
    enabled: item.enabled ?? true,
  }
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

/**
 * Relay's menu bar presence (docs/menu-bar.md). It is a view over the same
 * snapshot the window renders, plus the two presentation switches macOS users
 * expect; it never starts, chooses or retries work itself.
 */
export class MenuBar {
  readonly #host: MenuBarHost
  #tray: Tray | undefined
  #timer: NodeJS.Timeout | undefined
  #refreshInFlight = false
  /** Serialized previous model, so an unchanged menu is not rebuilt. */
  #lastModel = ''

  constructor(host: MenuBarHost) {
    this.#host = host
  }

  /** False when the icon asset was missing; the app then stays window-only. */
  get isInstalled(): boolean {
    return this.#tray !== undefined
  }

  async start(): Promise<void> {
    const icon = trayIcon()
    if (icon.isEmpty()) {
      /*
       * A missing asset must not take the app down: Relay stays usable as a
       * window-only app, and the window's close behaviour follows isInstalled
       * so the user can never end up with a hidden window and no menu bar icon.
       */
      console.warn('[relay] menu bar icon is missing; running without a menu bar')
      return
    }
    this.#tray = new Tray(icon)
    // Double-click is the Clash-style shortcut back to the panel; a single
    // click belongs to the menu itself.
    this.#tray.on('double-click', () => this.#host.openPanel({}))
    await this.refresh(true)
    this.#timer = setInterval(() => void this.refresh(), REFRESH_INTERVAL_MS)
    this.#timer.unref?.()
    this.#previewIfRequested()
  }

  /**
   * Development-only hook used to capture the open menu in screenshots
   * (docs/menu-bar.md "验证"). Packaged builds never open a menu on their own.
   */
  #previewIfRequested(): void {
    if (app.isPackaged || process.env.RELAY_MENU_BAR_PREVIEW !== '1') return
    const delay = Number(process.env.RELAY_MENU_BAR_PREVIEW_DELAY_MS ?? 1_500)
    setTimeout(() => this.#tray?.popUpContextMenu(), Number.isFinite(delay) ? delay : 1_500)
  }

  /** Rebuilds the native menu when the projection changed. */
  async refresh(force = false): Promise<void> {
    if (!this.#tray || this.#refreshInFlight) return
    this.#refreshInFlight = true
    try {
      const view = menuBarViewFromSnapshot({
        snapshot: this.#host.snapshot(),
        codex: await this.#host.codex(),
        prefs: { ...this.#host.menuBarPrefs(), launchAtLogin: this.#host.launchAtLogin() },
        locale: this.#host.locale(),
        platform: process.platform as MenuBarPlatform,
      })
      const model = JSON.stringify(view)
      if (!force && model === this.#lastModel) return
      this.#lastModel = model
      const items = buildMenuBarItems(view)
      this.#tray.setToolTip(menuBarStatusLabel(view))
      this.#tray.setContextMenu(
        Menu.buildFromTemplate(items.map((item) => toTemplateItem(item, (action) => this.#dispatch(action)))),
      )
    } finally {
      this.#refreshInFlight = false
    }
  }

  #dispatch(action: MenuBarAction): void {
    switch (action.type) {
      case 'open-panel':
        this.#host.openPanel({
          ...(action.hostSessionId ? { hostSessionId: action.hostSessionId } : {}),
          ...(action.runId ? { runId: action.runId } : {}),
        })
        return
      case 'open-settings':
        this.#host.openPanel({ settingsSection: action.section ?? 'general' })
        return
      case 'cancel-worker':
        this.#host.cancelWorker(action.workerSessionId)
        return
      case 'cancel-session':
        this.#host.cancelSession(action.hostSessionId)
        return
      case 'open-workspace':
        this.#host.openWorkspace(action.cwd)
        return
      case 'copy-session-id':
        this.#host.copySessionId(action.hostSessionId)
        return
      case 'copy-diagnostics':
        this.#host.copyDiagnostics()
        return
      case 'install-codex':
        this.#host.installCodex()
        return
      case 'toggle-hide-dock':
        this.#host.setHideDockIcon(!this.#host.menuBarPrefs().hideDockIcon)
        void this.refresh(true)
        return
      case 'toggle-launch-at-login':
        this.#host.setLaunchAtLogin(!this.#host.launchAtLogin())
        void this.refresh(true)
        return
      case 'quit':
        this.#host.quit()
        return
    }
  }

  destroy(): void {
    if (this.#timer) clearInterval(this.#timer)
    this.#timer = undefined
    this.#tray?.destroy()
    this.#tray = undefined
  }
}
