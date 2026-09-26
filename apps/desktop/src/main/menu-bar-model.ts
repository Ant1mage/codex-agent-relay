import { createTranslator, type TranslationKey } from '@relay/i18n'
import type { Locale } from '@relay/protocol'
import type { CodexIntegrationStatus, DesktopSnapshot } from '../shared/api.js'

/**
 * The menu bar is a projection of the same snapshot the window renders, and the
 * model below is deliberately free of Electron: it is a plain description of the
 * menu that `menu-bar.ts` turns into a native NSMenu. Keeping the description
 * separate is what makes the menu testable without a GUI session.
 */

export type MenuBarPlatform = 'darwin' | 'win32' | 'linux'

/** Settings sections the menu may open, mirroring settings.tsx Selection. */
export type MenuBarSettingsSection = 'general' | 'agents' | 'workspace' | 'advanced'

export type MenuBarAction =
  | { type: 'open-panel'; hostSessionId?: string; runId?: string }
  | { type: 'open-settings'; section?: MenuBarSettingsSection }
  | { type: 'cancel-worker'; workerSessionId: string }
  | { type: 'cancel-session'; hostSessionId: string }
  | { type: 'open-workspace'; cwd: string }
  | { type: 'copy-session-id'; hostSessionId: string }
  | { type: 'copy-diagnostics' }
  | { type: 'install-codex' }
  | { type: 'toggle-hide-dock' }
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

export interface MenuBarActiveWorker {
  workerSessionId: string
  runId: string
  /** `profile · task preview`, the two facts that identify a running worker. */
  label: string
}

export interface MenuBarSession {
  id: string
  displayName: string
  cwd: string
  activeWorkers: MenuBarActiveWorker[]
}

/** Why an agent cannot be used right now; absent means it can be. */
export type MenuBarAgentBlock = 'auth' | 'missing' | 'disabled'

/**
 * The switches the menu offers. `hideDockIcon` is Relay's own preference;
 * `launchAtLogin` is read back from the OS login items, which is why they are
 * not the same object even though they render identically.
 */
export interface MenuBarSwitches {
  hideDockIcon: boolean
  launchAtLogin: boolean
}

export interface MenuBarAgent {
  id: string
  name: string
  blocked?: MenuBarAgentBlock
}

export interface MenuBarView {
  locale: Locale
  platform: MenuBarPlatform
  status: 'ready' | 'needsSetup' | 'noRuntime'
  /** Workers in starting/running state across every session. */
  runningWorkers: number
  /** Runs whose worker finished and now wait for Codex to review. */
  awaitingHost: number
  sessions: MenuBarSession[]
  agents: MenuBarAgent[]
  codex: { configured: boolean; checks: Array<{ id: string; ok: boolean; detail: string }> }
  prefs: MenuBarSwitches
  /** Guard rails so a busy machine cannot grow the menu without bound. */
  maxSessions?: number
  maxWorkers?: number
}

export const DEFAULT_MAX_MENU_SESSIONS = 8
export const DEFAULT_MAX_MENU_WORKERS = 12

const ACTIVE_WORKER_STATUSES = new Set(['starting', 'running'])

/**
 * Relay's environment chip and this menu must agree, so both read the same two
 * facts: a detected runtime and a configured Codex integration (docs/ui.md 22.2).
 */
export function menuBarStatus(
  snapshot: Pick<DesktopSnapshot, 'runtimes' | 'profiles'>,
  codex: Pick<CodexIntegrationStatus, 'configured'>,
): MenuBarView['status'] {
  if (snapshot.runtimes.length === 0) return 'noRuntime'
  const usable = snapshot.profiles.some(
    (profile) =>
      profile.enabled &&
      snapshot.runtimes.some((runtime) => runtime.id === profile.runtimeId && runtime.health === 'available'),
  )
  return codex.configured && usable ? 'ready' : 'needsSetup'
}

/** First line of a delegated task, cut to a length a menu can actually show. */
export function taskPreview(task: string, max = 48): string {
  const line = task.split('\n').find((candidate) => candidate.trim().length > 0)?.trim() ?? ''
  const collapsed = line.replace(/\s+/g, ' ')
  return collapsed.length > max ? `${collapsed.slice(0, max - 1).trimEnd()}…` : collapsed
}

/**
 * Projects a snapshot into the menu's input. Cancel/report actions and the
 * window share one snapshot revision, so a menu never shows a worker the
 * snapshot has already retired.
 */
export function menuBarViewFromSnapshot(input: {
  snapshot: DesktopSnapshot
  codex: CodexIntegrationStatus
  prefs: MenuBarSwitches
  locale: Locale
  platform: MenuBarPlatform
}): MenuBarView {
  const { snapshot, codex, prefs, locale, platform } = input
  const profileName = (id: string) =>
    snapshot.profiles.find((profile) => profile.id === id)?.name ?? id
  const sessions: MenuBarSession[] = snapshot.sessions.map((session) => {
    const runs = snapshot.runs.filter((view) => view.run.hostSessionId === session.id)
    const activeWorkers: MenuBarActiveWorker[] = []
    for (const view of runs) {
      for (const worker of view.workers) {
        if (!ACTIVE_WORKER_STATUSES.has(worker.status)) continue
        const step = view.steps.find((candidate) => candidate.id === worker.stepId)
        const profile = profileName(step?.profileId ?? view.run.profileId)
        const task = taskPreview(step?.task ?? view.run.task)
        activeWorkers.push({
          workerSessionId: worker.id,
          runId: view.run.id,
          label: task ? `${profile} · ${task}` : profile,
        })
      }
    }
    return { id: session.id, displayName: session.displayName, cwd: session.cwd, activeWorkers }
  })

  const agents: MenuBarAgent[] = snapshot.profiles.map((profile) => {
    const runtime = snapshot.runtimes.find((candidate) => candidate.id === profile.runtimeId)
    const blocked: MenuBarAgentBlock | undefined = !runtime || runtime.health === 'unavailable'
      ? 'missing'
      : runtime.health === 'authentication_required'
        ? 'auth'
        : profile.enabled
          ? undefined
          : 'disabled'
    return { id: profile.id, name: profile.name, ...(blocked ? { blocked } : {}) }
  })

  const runningWorkers = sessions.reduce((total, session) => total + session.activeWorkers.length, 0)
  const awaitingHost = snapshot.runs.filter((view) => view.run.status === 'awaiting_host').length

  return {
    locale,
    platform,
    status: menuBarStatus(snapshot, codex),
    runningWorkers,
    awaitingHost,
    sessions,
    agents,
    codex: {
      configured: codex.configured,
      checks: codex.checks.map((check) => ({ id: check.id, ok: check.ok, detail: check.detail })),
    },
    prefs,
  }
}

/**
 * The first line of the menu. Clash leads with the proxy state; Relay leads with
 * worker state, because that is the only thing a menu bar can usefully report
 * (docs/architecture.md 1.1: the renderer — and so the menu — never derives new
 * domain facts, it only reads the projection).
 */
export function menuBarStatusLabel(view: MenuBarView): string {
  const t = createTranslator(view.locale)
  if (view.runningWorkers > 0) {
    return t('menu.status.running').replace('{count}', String(view.runningWorkers))
  }
  if (view.awaitingHost > 0) {
    return t('menu.status.awaiting').replace('{count}', String(view.awaitingHost))
  }
  if (view.status === 'noRuntime') return t('menu.status.noRuntime')
  if (view.status === 'needsSetup') return t('menu.status.needsSetup')
  return t('menu.status.ready')
}

/** A section label with a trailing count, e.g. "Active delegations (2)". */
function withCount(label: string, count: number): string {
  return `${label} (${count})`
}

function separator(): MenuBarItem {
  return { kind: 'separator', label: '' }
}

function header(label: string): MenuBarItem {
  return { kind: 'header', label, enabled: false }
}

/**
 * Builds the whole menu. Everything in it is either a projection of the
 * snapshot or an OS-level switch; nothing here decides which agent should run,
 * because that decision belongs to Codex (docs/architecture.md 1).
 */
export function buildMenuBarItems(view: MenuBarView): MenuBarItem[] {
  const t = createTranslator(view.locale)
  const maxSessions = view.maxSessions ?? DEFAULT_MAX_MENU_SESSIONS
  const maxWorkers = view.maxWorkers ?? DEFAULT_MAX_MENU_WORKERS
  const items: MenuBarItem[] = [
    header(menuBarStatusLabel(view)),
    { kind: 'normal', label: t('menu.open'), accelerator: 'CmdOrCtrl+O', action: { type: 'open-panel' } },
    separator(),
  ]

  const activeSessions = view.sessions.filter((session) => session.activeWorkers.length > 0)
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
              label: t('menu.showInPanel'),
              action: { type: 'open-panel', hostSessionId: session.id, runId: worker.runId },
            },
            {
              kind: 'normal',
              label: t('menu.cancelWorker'),
              action: { type: 'cancel-worker', workerSessionId: worker.workerSessionId },
            },
          ],
        })
      }
      // A single worker already has its own cancel item; only a session with
      // several needs a bulk action.
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

  const sessions = view.sessions.slice(0, maxSessions)
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
                label: t('menu.showInPanel'),
                action: { type: 'open-panel', hostSessionId: session.id },
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
    submenu:
      view.agents.length === 0
        ? [header(t('menu.noAgents'))]
        : view.agents.map((agent) => {
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
              label: reason ? `${agent.name} · ${reason}` : agent.name,
              enabled: !agent.blocked,
              action: { type: 'open-settings' as const, section: 'agents' as const },
            }
          }),
  })

  const checkLabels: Record<string, TranslationKey> = {
    'codex-cli': 'onboarding.check.codex-cli',
    'relay-mcp': 'onboarding.check.relay-mcp',
    'relay-skill': 'onboarding.check.relay-skill',
  }
  items.push({
    kind: 'normal',
    label: t('menu.codex'),
    submenu: [
      header(view.codex.configured ? t('menu.codexConnected') : t('menu.codexMissing')),
      ...view.codex.checks.map((check) => {
        const key = checkLabels[check.id]
        const label = key ? t(key) : check.id
        return header(`${check.ok ? '✓' : '✗'} ${label}${check.ok ? '' : ` — ${check.detail}`}`)
      }),
      separator(),
      { kind: 'normal', label: t('menu.installCodex'), action: { type: 'install-codex' } },
    ],
  })

  items.push(separator())
  // The two Clash-style switches that are honestly Relay's to own: how the app
  // presents itself, never how work is routed (that is Codex's call).
  if (view.platform === 'darwin') {
    items.push({
      kind: 'checkbox',
      label: t('menu.hideDock'),
      checked: view.prefs.hideDockIcon,
      action: { type: 'toggle-hide-dock' },
    })
  }
  if (view.platform === 'darwin' || view.platform === 'win32') {
    items.push({
      kind: 'checkbox',
      label: t('menu.launchAtLogin'),
      checked: view.prefs.launchAtLogin,
      action: { type: 'toggle-launch-at-login' },
    })
  }
  items.push(separator())
  items.push({
    kind: 'normal',
    label: t('menu.settings'),
    accelerator: 'CmdOrCtrl+,',
    action: { type: 'open-settings' },
  })
  items.push({ kind: 'normal', label: t('menu.copyDiagnostics'), action: { type: 'copy-diagnostics' } })
  items.push(separator())
  items.push({ kind: 'normal', label: t('menu.quit'), accelerator: 'CmdOrCtrl+Q', action: { type: 'quit' } })
  return items
}
