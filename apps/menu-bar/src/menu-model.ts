import { createTranslator, type TranslationKey, type Translator } from '@relay/i18n'
import type { Locale } from '@relay/protocol'
import type { MenuView } from '@relay/relay-api'

/**
 * The tray menu, as data. Nothing here imports Electron, so the whole menu can
 * be unit tested without a GUI session; main.ts turns the items into an NSMenu
 * and performs the actions.
 */

export type MenuBarPlatform = 'darwin' | 'win32' | 'linux'
export type DaemonStatus = 'running' | 'stopped' | 'starting'

export type PanelTab = 'agents' | 'policy' | 'codex' | 'runtime'
/** Menu picks that open the panel straight into a form, never a second window. */
export type PanelIntent = 'new-agent' | 'edit-agent' | 'add-runtime' | 'codex-actions'

export type MenuBarAction =
  | { type: 'open-inspector'; hostSessionId?: string; runId?: string }
  | { type: 'open-panel'; tab: PanelTab; intent?: PanelIntent; profileId?: string }
  | { type: 'rescan' }
  | { type: 'repair-codex' }
  | { type: 'start-daemon' }
  | { type: 'restart-daemon' }
  | { type: 'cancel-worker'; workerSessionId: string }
  | { type: 'cancel-session'; hostSessionId: string }
  | { type: 'open-workspace'; cwd: string }
  | { type: 'copy-session-id'; hostSessionId: string }
  | { type: 'copy-diagnostics' }
  | { type: 'install-codex' }
  | { type: 'refresh' }
  | { type: 'check-updates' }
  | { type: 'download-update' }
  | { type: 'install-update' }
  | { type: 'toggle-launch-at-login' }
  | { type: 'quit' }

export interface MenuBarItem {
  kind: 'normal' | 'header' | 'separator' | 'checkbox'
  label: string
  checked?: boolean
  enabled?: boolean
  accelerator?: string
  action?: MenuBarAction
  submenu?: MenuBarItem[]
}

export interface MenuBarView {
  locale: Locale
  platform: MenuBarPlatform
  daemon: DaemonStatus
  /** Absent while the daemon is not reachable; the menu then offers to start it. */
  menu?: MenuView
  /** Why the daemon is not reachable, shown as the disabled status line. */
  error?: string
  /** Tray↔daemon version skew, which an app update can create. */
  daemonVersionMismatch?: { running: string; app: string }
  update?: { status: string; version?: string; percent?: number; message?: string }
  launchAtLogin: boolean
  /** Browser the inspector opens in, for the menu's own labelling. */
  browser: string
  maxSessions?: number
  maxWorkers?: number
}

export const DEFAULT_MAX_MENU_SESSIONS = 8
export const DEFAULT_MAX_MENU_WORKERS = 12

/** The first line of the menu: what is happening, or why nothing can. */
export function menuBarStatusLabel(view: MenuBarView): string {
  const t = createTranslator(view.locale)
  if (view.daemon === 'starting') return t('menu.status.starting')
  if (view.daemon !== 'running' || !view.menu) return t('menu.status.daemonDown')
  const menu = view.menu
  if (menu.runningWorkers > 0) {
    return t('menu.status.running').replace('{count}', String(menu.runningWorkers))
  }
  if (menu.awaitingHost > 0) {
    return t('menu.status.awaiting').replace('{count}', String(menu.awaitingHost))
  }
  if (menu.status === 'noRuntime') return t('menu.status.noRuntime')
  if (menu.status === 'needsSetup') return t('menu.status.needsSetup')
  return t('menu.status.ready')
}

function separator(): MenuBarItem {
  return { kind: 'separator', label: '' }
}

function header(label: string): MenuBarItem {
  return { kind: 'header', label, enabled: false }
}

function withCount(label: string, count: number): string {
  return `${label} (${count})`
}

/**
 * Builds the whole menu. Everything is either a projection of the daemon's
 * snapshot or an OS-level switch; the tray never decides which agent should run
 * (docs/menu-bar.md 6).
 */
/**
 * The App update section. It is deliberately independent of the daemon: a new
 * Relay can be available while the log service is down, and the Codex
 * integration has its own lifecycle (docs/updates.md).
 */
function updateItems(t: Translator, view: MenuBarView): MenuBarItem[] {
  const items: MenuBarItem[] = []
  const update = view.update
  if (update?.status === 'available' || update?.status === 'downloaded') {
    items.push({ kind: 'normal', label: t('menu.updateAvailable', { version: update.version ?? '' }), enabled: false })
    items.push({
      kind: 'normal',
      label: update.status === 'available' ? t('menu.downloadUpdate') : t('menu.installUpdate'),
      action: { type: update.status === 'available' ? 'download-update' : 'install-update' },
    })
  } else if (update?.status === 'downloading') {
    items.push(header(t('menu.downloadUpdate') + ' ' + (update.percent ?? 0) + '%'))
  } else if (update?.status === 'checking') {
    items.push(header(t('menu.updateChecking')))
  } else if (update?.status === 'none') {
    items.push(header(t('menu.updateNone')))
  } else if (update?.status === 'error') {
    items.push(header('⚠︎ ' + (update.message ?? 'update error')))
  }
  items.push({
    kind: 'normal',
    label: t('menu.checkUpdates'),
    enabled: update?.status !== 'checking',
    action: { type: 'check-updates' },
  })
  return items
}

export function buildMenuBarItems(view: MenuBarView): MenuBarItem[] {
  const t = createTranslator(view.locale)
  const maxSessions = view.maxSessions ?? DEFAULT_MAX_MENU_SESSIONS
  const maxWorkers = view.maxWorkers ?? DEFAULT_MAX_MENU_WORKERS
  const menu = view.menu
  const items: MenuBarItem[] = [header(menuBarStatusLabel(view))]

  if (view.daemon !== 'running' || !menu) {
    items.push({
      kind: 'normal',
      label: view.daemon === 'starting' ? t('menu.starting') : t('menu.startDaemon'),
      enabled: view.daemon !== 'starting',
      action: { type: 'start-daemon' },
    })
    items.push({
      kind: 'normal',
      label: t('menu.restartDaemon'),
      enabled: view.daemon !== 'starting',
      action: { type: 'restart-daemon' },
    })
    items.push(separator())
    items.push({ kind: 'normal', label: t('menu.diagnostics'), action: { type: 'copy-diagnostics' } })
    items.push(separator())
    // App updates do not depend on the log service being up.
    items.push(...updateItems(t, view))
    items.push(separator())
    items.push({ kind: 'normal', label: t('menu.quit'), accelerator: 'CmdOrCtrl+Q', action: { type: 'quit' } })
    return items
  }

  items.push({
    kind: 'normal',
    label: t('menu.openInspector'),
    accelerator: 'CmdOrCtrl+O',
    action: { type: 'open-inspector' },
  })
  items.push({
    kind: 'normal',
    label: t('menu.openPanel'),
    accelerator: 'CmdOrCtrl+,',
    action: { type: 'open-panel', tab: 'agents' },
  })
  items.push(separator())

  const activeSessions = menu.sessions.filter((session) => session.activeWorkers.length > 0)
  if (activeSessions.length > 0) {
    const submenu: MenuBarItem[] = []
    let shown = 0
    let hidden = 0
    for (const session of activeSessions) {
      for (const worker of session.activeWorkers) {
        if (shown >= maxWorkers) {
          hidden += 1
          continue
        }
        shown += 1
        submenu.push({
          kind: 'normal',
          label: `${session.displayName} — ${worker.label}`,
          submenu: [
            {
              kind: 'normal',
              label: t('menu.showInInspector'),
              action: { type: 'open-inspector', hostSessionId: session.id, runId: worker.runId },
            },
            {
              kind: 'normal',
              label: t('menu.cancelWorker'),
              action: { type: 'cancel-worker', workerSessionId: worker.workerSessionId },
            },
          ],
        })
      }
      if (session.activeWorkers.length > 1) {
        submenu.push({
          kind: 'normal',
          label: `${t('menu.stopAll')} · ${session.displayName}`,
          action: { type: 'cancel-session', hostSessionId: session.id },
        })
      }
    }
    if (hidden > 0) submenu.push(header(`+${hidden}`))
    items.push({ kind: 'normal', label: withCount(t('menu.active'), shown + hidden), submenu })
    items.push(separator())
  }

  const sessions = menu.sessions.slice(0, maxSessions)
  items.push({
    kind: 'normal',
    label: t('menu.sessions'),
    submenu:
      sessions.length === 0
        ? [header(t('menu.noSessions'))]
        : sessions.map((session) => ({
            kind: 'normal',
            label: session.displayName,
            submenu: [
              {
                kind: 'normal',
                label: t('menu.showInInspector'),
                action: { type: 'open-inspector', hostSessionId: session.id },
              },
              {
                kind: 'normal',
                label: t('menu.stopAll'),
                enabled: session.activeWorkers.length > 0,
                action: { type: 'cancel-session', hostSessionId: session.id },
              },
              separator(),
              {
                kind: 'normal',
                label: t('menu.openWorkspace'),
                action: { type: 'open-workspace', cwd: session.cwd },
              },
              {
                kind: 'normal',
                label: t('menu.copySessionId'),
                action: { type: 'copy-session-id', hostSessionId: session.id },
              },
            ],
          })),
  })

  items.push({
    kind: 'normal',
    label: t('menu.agents'),
    submenu: [
      { kind: 'normal', label: t('panel.addAgent'), action: { type: 'open-panel', tab: 'agents', intent: 'new-agent' } },
      separator(),
      ...(menu.agents.length === 0
        ? [header(t('menu.noAgents'))]
        : menu.agents.map((agent) => {
            const reason =
              agent.blocked === 'auth'
                ? t('agents.authRequired')
                : agent.blocked === 'missing'
                  ? t('agents.notInstalled')
                  : agent.blocked === 'disabled'
                    ? t('menu.agentDisabled')
                    : undefined
            return {
              kind: 'normal' as const,
              // A name is a shortcut into the editor; the switches live in the
              // panel because a menu cannot carry a form.
              label: reason ? `${agent.name} · ${reason}` : agent.name,
              action: {
                type: 'open-panel' as const,
                tab: 'agents' as const,
                intent: 'edit-agent' as const,
                profileId: agent.id,
              },
            }
          })),
    ],
  })

  items.push({
    kind: 'normal',
    label: t('panel.runtime'),
    submenu: [
      ...(menu.runtimes.length === 0
        ? [header(t('panel.noRuntimes'))]
        : menu.runtimes.map((runtime) =>
            header(
              `${runtime.adapterId} · ${runtime.health}${runtime.version ? ` · ${runtime.version}` : ''}`,
            ),
          )),
      separator(),
      { kind: 'normal', label: t('panel.addRuntimeMenu'), action: { type: 'open-panel', tab: 'runtime', intent: 'add-runtime' } },
      { kind: 'normal', label: t('panel.rescan'), action: { type: 'rescan' } },
      { kind: 'normal', label: t('menu.openPanel'), action: { type: 'open-panel', tab: 'runtime' } },
    ],
  })

  const checkLabels: Record<string, TranslationKey> = {
    'codex-cli': 'onboarding.check.codex-cli',
    'relay-mcp': 'onboarding.check.relay-mcp',
    'relay-skill': 'onboarding.check.relay-skill',
    'relay-plugin': 'onboarding.check.relay-plugin',
    'relay-hooks': 'onboarding.check.relay-hooks',
  }
  const brokenChecks = menu.codex.checks.filter((check) => !check.ok)
  items.push({
    kind: 'normal',
    label: t('menu.codex'),
    submenu: [
      header(menu.codex.configured ? t('menu.codexConnected') : t('menu.codexMissing')),
      ...menu.codex.checks.map((check) => {
        const key = checkLabels[check.id]
        const label = key ? t(key) : check.id
        return header(`${check.ok ? '✓' : '✗'} ${label}${check.ok ? '' : ` — ${check.status}`}`)
      }),
      separator(),
      { kind: 'normal', label: t('menu.codexSettings'), action: { type: 'open-panel', tab: 'codex', intent: 'codex-actions' } },
      {
        kind: 'normal',
        label: brokenChecks.length > 0 ? t('menu.repairCodex') : t('menu.installCodex'),
        action: { type: 'repair-codex' },
      },
    ],
  })

  items.push(separator())
  if (view.error) items.push(header(`⚠︎ ${view.error.slice(0, 80)}`))
  if (view.daemon === 'running' && view.daemonVersionMismatch) {
    items.push(
      header(
        t('menu.daemonStale', {
          running: view.daemonVersionMismatch.running,
          app: view.daemonVersionMismatch.app,
        }),
      ),
    )
  }
  items.push({ kind: 'normal', label: t('menu.refresh'), action: { type: 'refresh' } })
  items.push({ kind: 'normal', label: t('menu.diagnostics'), action: { type: 'copy-diagnostics' } })
  if (view.platform === 'darwin' || view.platform === 'win32') {
    items.push({
      kind: 'checkbox',
      label: t('menu.launchAtLogin'),
      checked: view.launchAtLogin,
      action: { type: 'toggle-launch-at-login' },
    })
  }
  items.push(separator())
  // App updates remain actionable while relayd is healthy; they are not a
  // daemon-recovery-only feature.
  items.push(...updateItems(t, view))
  items.push(separator())
  items.push({ kind: 'normal', label: t('menu.quit'), accelerator: 'CmdOrCtrl+Q', action: { type: 'quit' } })
  return items
}

/** Menu bar entries are localized on the tray side; the wire stays neutral. */
export function menuBarTooltip(view: MenuBarView): string {
  return menuBarStatusLabel(view)
}
