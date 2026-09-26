import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Check,
  CircleAlert,
  FileDiff,
  MoreHorizontal,
  PanelLeft,
  PanelRight,
  RefreshCw,
  Settings as SettingsIcon,
  Square,
  X,
} from 'lucide-react'
import type { HostSession, Step } from '@relay/protocol'
import type { DesktopRunView } from '../../shared/api.js'
import { useAppStore } from './store.js'
import { createTranslator } from '@relay/i18n'
import { Onboarding } from './onboarding.js'
import { SettingsSheet } from './settings.js'
import {
  ConsoleRows,
  ProviderIcon,
  StatusLabel,
  consoleRows,
  elapsed,
  eventSummary,
  relativeTime,
  statusGlyph,
  storedFontSize,
  useRuntimeOptions,
  type Inspector,
  type Theme,
  type Translator,
} from './ui.js'

/* ------------------------------------------------------------------ */
/* Sidebar (docs/ui.md 4-5)                                            */
/* ------------------------------------------------------------------ */

function Sidebar({ t }: { t: Translator }) {
  const { snapshot, selectedSessionId, selectSession, setSettingsOpen } = useAppStore()
  const sessions = snapshot?.sessions ?? []
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <span className="sidebar-mark mark-tinted" role="img" aria-label="Relay" />
        <strong>{t('app.name')}</strong>
      </div>
      <div className="sidebar-section-title">{t('sessions.title')}</div>
      <div className="session-list">
        {/* Text-first rows, no per-session icons (docs/ui.md 4.3) */}
        {sessions.map((session) => (
          <button
            className={session.id === selectedSessionId ? 'session-name selected' : 'session-name'}
            key={session.id}
            onClick={() => selectSession(session.id)}
            title={session.displayName}
          >
            <strong>{session.displayName}</strong>
            <span>{relativeTime(session.updatedAt, session.status, t)}</span>
          </button>
        ))}
        {!sessions.length && <p className="sidebar-empty">{t('sessions.empty')}</p>}
      </div>
      <button className="sidebar-settings" onClick={() => setSettingsOpen(true)}>
        <SettingsIcon size={15} />{t('settings.title')}
      </button>
    </aside>
  )
}

/**
 * Session contextual menu (docs/ui.md 6/19). Session-level controls live here so
 * the toolbar never carries a global Stop button.
 */
function SessionMenu({ session, view, t }: { session: HostSession; view: DesktopRunView | undefined; t: Translator }) {
  const { setNotice, refresh } = useAppStore()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false)
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [open])

  const stopAll = async () => {
    setOpen(false)
    const result = await window.relay.cancelSessionWorkers(session.id)
    setNotice(result.count ? t('session.stopped').replace('{count}', String(result.count)) : t('session.stoppedNone'))
    await refresh()
  }
  const openWorkspace = async () => {
    setOpen(false)
    const result = await window.relay.openWorkspace(session.cwd)
    if (!result.ok) setNotice(result.message ?? t('error.generic'))
  }
  const copyId = async () => {
    setOpen(false)
    await navigator.clipboard.writeText(session.nativeSessionId)
    setNotice(t('session.idCopied'))
  }

  return (
    <div className="session-menu" ref={ref}>
      <button
        className="plain-icon"
        aria-label={t('session.menu')}
        aria-expanded={open}
        title={t('session.menu')}
        onClick={() => setOpen((current) => !current)}
      >
        <MoreHorizontal size={16} />
      </button>
      {open && (
        <div className="menu-popover" role="menu">
          <button onClick={() => void stopAll()}>{t('session.stopAll')}</button>
          <button onClick={() => void openWorkspace()}>{t('session.openWorkspace')}</button>
          <button onClick={() => void copyId()}>{t('session.copyId')}</button>
        </div>
      )}
      {view && <span className="session-run-status"><StatusLabel status={view.run.status} t={t} /></span>}
    </div>
  )
}

/* ------------------------------------------------------------------ */
/* Step (docs/ui.md 7-11)                                              */
/* ------------------------------------------------------------------ */

interface StepItem { view: DesktopRunView; step: Step }

function StepNode({
  item,
  index,
  selected,
  t,
  registerRef,
}: {
  item: StepItem
  index: number
  selected: boolean
  t: Translator
  registerRef(element: HTMLButtonElement | null): void
}) {
  const { snapshot, selectStep } = useAppStore()
  const { view, step } = item
  const profile = snapshot?.profiles.find((candidate) => candidate.id === view.run.profileId)
  const runtime = snapshot?.runtimes.find((candidate) => candidate.id === profile?.runtimeId)
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const name = profile?.name ?? view.run.profileId

  return (
    <div className="step-link">
      <button
        ref={registerRef}
        className={selected ? 'step-node selected' : `step-node ${step.status}`}
        onClick={() => selectStep(view.run.id, step.id)}
        title={`${t('steps.step')} ${index + 1} · ${name}`}
      >
        <div className="step-profile">
          <i className={`step-glyph ${step.status}`} aria-hidden="true">{statusGlyph(step.status)}</i>
          <ProviderIcon name={profile?.name ?? view.run.profileId} adapterId={runtime?.adapterId} />
          <strong>{name}</strong>
        </div>
        <em>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</em>
      </button>
    </div>
  )
}

function StepNavigator({ items, t }: { items: StepItem[]; t: Translator }) {
  const { selectedRunId, selectedStepId } = useAppStore()
  const currentRef = useRef<HTMLButtonElement>(null)
  const ordered = useMemo(
    () => [...items].sort((left, right) => left.step.createdAt.localeCompare(right.step.createdAt)),
    [items],
  )

  useEffect(() => {
    currentRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'center' })
  }, [selectedRunId, selectedStepId])

  return (
    <section className="step-strip" aria-label={t('steps.title')}>
      <div className="step-strip-label">{t('steps.title')}</div>
      <div className="step-scroll">
        {ordered.map((item, index) => {
          const selected = item.view.run.id === selectedRunId && item.step.id === selectedStepId
          return (
            <StepNode
              key={item.step.id}
              item={item}
              index={index}
              selected={selected}
              t={t}
              registerRef={(element) => { if (selected) currentRef.current = element }}
            />
          )
        })}
      </div>
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* Console (docs/ui.md 12-14)                                          */
/* ------------------------------------------------------------------ */

function Console({
  view,
  step,
  t,
  openInspector,
  cliInfoOpen,
  toggleCliInfo,
}: {
  view: DesktopRunView
  step: Step
  t: Translator
  openInspector(inspector: Exclude<Inspector, undefined>): void
  cliInfoOpen: boolean
  toggleCliInfo(): void
}) {
  const { setNotice, snapshot } = useAppStore()
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const events = view.events.filter((event) => !event.stepId || event.stepId === step.id)
  const rows = useMemo(() => consoleRows(events), [events])
  const changeCount = events.filter((event) => event.type === 'tool/edit').length
  const rawCount = events.filter((event) => event.nativeEvent !== undefined).length
  const active = step.status === 'running' || step.status === 'starting'
  const profile = snapshot?.profiles.find((item) => item.id === view.run.profileId)

  return (
    <section className="console-panel">
      <header className="console-header">
        <div>
          <strong>{t('console.title')} · {profile?.name ?? view.run.profileId}</strong>
          <span>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</span>
        </div>
        <div className="console-actions">
          {changeCount > 0 && (
            <button onClick={() => openInspector('changes')}><FileDiff size={13} />{t('console.changes')} <b>{changeCount}</b></button>
          )}
          <button onClick={() => openInspector('raw')}>{t('console.rawOutput')} <b>{rawCount}</b></button>
          {/* Worker-level Stop sits beside the selected CLI (docs/ui.md 19) */}
          {active && worker && (
            <button
              className="stop-button"
              onClick={async () => {
                const response = await window.relay.cancelWorker(worker.id)
                setNotice(response.accepted ? t('runs.cancelQueued') : response.message)
              }}
            >
              <Square size={11} />{t('action.stop')}
            </button>
          )}
          <button className={cliInfoOpen ? 'selected' : ''} onClick={toggleCliInfo}>
            <PanelRight size={13} />{t('cliInfo.title')}
          </button>
        </div>
      </header>
      <div className="console-body">
        {!rows.length && <div className="console-empty">{t('console.empty')}</div>}
        <ConsoleRows rows={rows} t={t} />
      </div>
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* CLI Info (docs/ui.md 15-16, 18)                                     */
/* ------------------------------------------------------------------ */

type Override = { model?: string; reasoning?: string }

function overrideKey(runId: string): string {
  return `relay.override.${runId}`
}

function CliInfo({ view, step, t, close, openInspector }: {
  view: DesktopRunView
  step: Step
  t: Translator
  close(): void
  openInspector(inspector: Exclude<Inspector, undefined>): void
}) {
  const { snapshot, setNotice } = useAppStore()
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const profile = snapshot?.profiles.find((candidate) => candidate.id === view.run.profileId)
  const runtime = snapshot?.runtimes.find((candidate) => candidate.id === worker?.runtimeId)
  const { options } = useRuntimeOptions(runtime?.id)
  const [override, setOverride] = useState<Override>(() => {
    try {
      return JSON.parse(localStorage.getItem(overrideKey(view.run.id)) ?? '{}') as Override
    } catch {
      return {}
    }
  })
  const active = step.status === 'running' || step.status === 'starting'
  const changeCount = view.events.filter((event) => (!event.stepId || event.stepId === step.id) && event.type === 'tool/edit').length
  const childAgents = new Set(
    view.events
      .filter((event) => event.type === 'child/started')
      .map((event) => String((event.data as { agentId?: string } | undefined)?.agentId ?? event.id)),
  ).size
  /**
   * Reasoning values are CLI tokens, so label them using the CLI's own names and
   * only fall back to a generic label for a value the CLI no longer reports.
   */
  const reasoningLabel = (value: string | undefined) => {
    if (!value) return t('agents.default')
    return options?.levels.find((level) => level.value === value)?.label ?? value
  }

  const applyOverride = (key: keyof Override, value: string) => {
    const merged: Override = { ...override }
    if (value) merged[key] = value
    else delete merged[key]
    setOverride(merged)
    localStorage.setItem(overrideKey(view.run.id), JSON.stringify(merged))
    // Never mutate a running worker; the override applies to the next run.
    if (active) setNotice(t('cliInfo.overrideHint'))
  }

  return (
    <aside className="cli-info">
      <header>
        <div>
          <span className="cli-profile">
            <ProviderIcon name={profile?.name ?? view.run.profileId} adapterId={runtime?.adapterId} />
            {profile?.name ?? view.run.profileId}
          </span>
          <h2>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</h2>
        </div>
        <button className="plain-icon" aria-label={t('cliInfo.hide')} title={t('cliInfo.hide')} onClick={close}><X size={15} /></button>
      </header>
      <dl>
        <div><dt>{t('cliInfo.runtime')}</dt><dd>{runtime?.adapterId ?? worker?.runtimeId ?? t('common.notAvailable')}</dd></div>
        <div>
          <dt>{t('agents.model')}</dt>
          <dd>
            {/* Session-level override; applies to the next run (docs/ui.md 16.2) */}
            {options?.models.length ? (
              <select
                aria-label={t('agents.model')}
                value={override.model ?? ''}
                onChange={(event) => applyOverride('model', event.target.value)}
              >
                <option value="">{profile?.model ?? t('agents.runtimeDefault')}</option>
                {options.models.map((model) => (
                  <option value={model.value} key={model.value}>{model.label ?? model.value}</option>
                ))}
              </select>
            ) : (
              <span className="field-note">{t('agents.noModelList')}</span>
            )}
          </dd>
        </div>
        <div>
          <dt>{t('agents.reasoning')}</dt>
          <dd>
            {options?.levels.length ? (
              <select
                aria-label={t('agents.reasoning')}
                value={override.reasoning ?? profile?.reasoning ?? ''}
                onChange={(event) => applyOverride('reasoning', event.target.value)}
              >
                <option value="">{reasoningLabel(profile?.reasoning)}</option>
                {options.levels.map((level) => (
                  <option value={level.value} key={level.value}>{level.label}</option>
                ))}
              </select>
            ) : (
              <span className="field-note">{t('agents.noReasoningLevels')}</span>
            )}
          </dd>
        </div>
        <div><dt>{t('cliInfo.workingDirectory')}</dt><dd title={view.run.cwd}>{view.run.cwd}</dd></div>
        <div><dt>{t('cliInfo.started')}</dt><dd>{new Date(step.createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</dd></div>
        {childAgents > 0 && <div><dt>{t('cliInfo.agents')}</dt><dd>{childAgents}</dd></div>}
        <div>
          <dt>{t('cliInfo.changes')}</dt>
          <dd>
            {changeCount
              ? <button className="link-button" onClick={() => openInspector('changes')}>{changeCount} {t('console.files')}</button>
              : 0}
          </dd>
        </div>
      </dl>
    </aside>
  )
}

/* ------------------------------------------------------------------ */
/* Drill-down sheets (docs/ui.md 14, 18)                               */
/* ------------------------------------------------------------------ */

function InspectorSheet({ inspector, view, step, t, close }: {
  inspector: Exclude<Inspector, undefined>
  view: DesktopRunView
  step: Step
  t: Translator
  close(): void
}) {
  const events = view.events.filter((event) => !event.stepId || event.stepId === step.id)
  const changes = events.filter((event) => event.type === 'tool/edit')
  const raw = events.filter((event) => event.nativeEvent !== undefined)
  return (
    <div className="sheet-backdrop" onMouseDown={(event) => event.target === event.currentTarget && close()}>
      <aside className="inspector-sheet">
        <header>
          <div>
            <span>{view.run.profileId}</span>
            <h2>{inspector === 'changes' ? t('console.changes') : t('console.rawOutput')}</h2>
          </div>
          <button className="plain-icon" onClick={close}><X size={16} /></button>
        </header>
        {inspector === 'changes'
          ? (
            <div className="change-list">
              {!changes.length && <p>{t('console.noChanges')}</p>}
              {changes.map((event) => (
                <article key={event.id}>
                  <strong className="change-path">{eventSummary(event)}</strong>
                  <pre>{JSON.stringify(event.data, null, 2)}</pre>
                </article>
              ))}
            </div>
          )
          : (
            <div className="raw-output">
              {!raw.length && <p>{t('console.noRawOutput')}</p>}
              {raw.map((event) => (
                <article key={event.id}>
                  <time>{event.timestamp}</time>
                  <strong>{event.type}</strong>
                  <pre>{JSON.stringify(event.nativeEvent, null, 2)}</pre>
                </article>
              ))}
            </div>
          )}
      </aside>
    </div>
  )
}

/* ------------------------------------------------------------------ */
/* Workspace, empty state, app shell                                   */
/* ------------------------------------------------------------------ */

/** Minimal empty state content (docs/ui.md 22.3). */
function EmptyStateBody({ t, hint }: { t: Translator; hint?: boolean }) {
  return (
    <>
      <span className="empty-mark mark-tinted" role="img" aria-hidden="true" />
      <h1>{hint ? t('runs.empty') : t('empty.noSessions')}</h1>
      <p>{t('empty.useRelay')}</p>
    </>
  )
}

function SessionWorkspace({ t }: { t: Translator }) {
  const { snapshot, selectedSessionId, selectedRunId, selectedStepId, loading, refresh, onboardingOpen } = useAppStore()
  const [inspector, setInspector] = useState<Inspector>()
  const [cliInfoOpen, setCliInfoOpen] = useState(() => localStorage.getItem('relay.cli-info-open') !== 'false')

  if (onboardingOpen) return <Onboarding t={t} />

  const session = snapshot?.sessions.find((item) => item.id === selectedSessionId)
  if (!session) {
    return (
      <main className="workspace empty-workspace">
        <EmptyStateBody t={t} />
      </main>
    )
  }

  const views = snapshot?.runs.filter((item) => item.run.hostSessionId === selectedSessionId) ?? []
  const stepItems: StepItem[] = views.flatMap((view) => view.steps.map((step) => ({ view, step })))
  const selectedView = views.find((view) => view.run.id === selectedRunId) ?? views[0]
  const selectedStep = selectedView?.steps.find((step) => step.id === selectedStepId) ?? selectedView?.steps[0]
  const toggleCliInfo = () =>
    setCliInfoOpen((current) => {
      const next = !current
      localStorage.setItem('relay.cli-info-open', String(next))
      return next
    })

  return (
    <main className="workspace">
      <header className="workspace-heading">
        <div>
          <h1>{selectedView?.run.task ?? session.displayName}</h1>
          <p>
            {selectedView && <StatusLabel status={selectedView.run.status} t={t} />}
            {selectedView && <span className="heading-sep">·</span>}
            <span>{t('cliInfo.started')} {new Date(session.startedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
          </p>
        </div>
        <div className="heading-actions">
          <button className="plain-icon" title={t('common.refresh')} onClick={() => void refresh()}>
            <RefreshCw className={loading ? 'spin' : ''} size={15} />
          </button>
          <SessionMenu session={session} view={selectedView} t={t} />
        </div>
      </header>

      {stepItems.length ? (
        <>
          <StepNavigator items={stepItems} t={t} />
          {selectedView && selectedStep && (
            <div className={cliInfoOpen ? 'workspace-lower cli-info-open' : 'workspace-lower'}>
              <Console
                view={selectedView}
                step={selectedStep}
                t={t}
                openInspector={setInspector}
                cliInfoOpen={cliInfoOpen}
                toggleCliInfo={toggleCliInfo}
              />
              {cliInfoOpen && (
                <CliInfo
                  view={selectedView}
                  step={selectedStep}
                  t={t}
                  close={toggleCliInfo}
                  openInspector={setInspector}
                />
              )}
            </div>
          )}
        </>
      ) : (
        <div className="workspace-placeholder">
          <EmptyStateBody t={t} hint />
        </div>
      )}

      {inspector && selectedView && selectedStep && (
        <InspectorSheet
          inspector={inspector}
          view={selectedView}
          step={selectedStep}
          t={t}
          close={() => setInspector(undefined)}
        />
      )}
    </main>
  )
}

export function App() {
  const { locale, refresh, error, notice, setNotice } = useAppStore()
  const [theme, setThemeState] = useState<Theme>(() => (localStorage.getItem('relay.theme') as Theme | null) ?? 'system')
  const [fontSize, setFontSizeState] = useState(storedFontSize)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem('relay.sidebar-collapsed') === 'true')
  const t = createTranslator(locale)

  const setTheme = (next: Theme) => { localStorage.setItem('relay.theme', next); setThemeState(next) }
  const toggleSidebar = () =>
    setSidebarCollapsed((current) => {
      const next = !current
      localStorage.setItem('relay.sidebar-collapsed', String(next))
      return next
    })
  const setFontSize = (next: number) => {
    const value = Math.min(20, Math.max(14, Math.round(next)))
    localStorage.setItem('relay.font-size', String(value))
    setFontSizeState(value)
  }

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    if (theme === 'system') document.documentElement.removeAttribute('data-theme')
  }, [theme])
  useEffect(() => {
    document.documentElement.style.setProperty('--relay-font-scale', String(fontSize / 14))
  }, [fontSize])
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'b') {
        event.preventDefault()
        toggleSidebar()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  })
  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2_000)
    return () => window.clearInterval(timer)
  }, [refresh])
  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(undefined), 3_500)
    return () => window.clearTimeout(timer)
  }, [notice, setNotice])

  const sidebarLabel = sidebarCollapsed ? t('action.showSidebar') : t('action.hideSidebar')
  const macOS = navigator.userAgent.includes('Macintosh')

  return (
    <div className={`app-shell${sidebarCollapsed ? ' sidebar-collapsed' : ''}`}>
      {/* Compact sidebar toggle beside the window chrome (docs/ui.md 5) */}
      <header className={macOS ? 'app-toolbar macos' : 'app-toolbar'}>
        <div className="toolbar-leading">
          <button
            className="plain-icon"
            aria-label={sidebarLabel}
            title={sidebarLabel}
            aria-expanded={!sidebarCollapsed}
            onClick={toggleSidebar}
          >
            <PanelLeft size={16} />
          </button>
        </div>
        <div className="toolbar-drag" />
      </header>
      <div className="app-main">
        <Sidebar t={t} />
        <SessionWorkspace t={t} />
      </div>
      <SettingsSheet t={t} theme={theme} setTheme={setTheme} fontSize={fontSize} setFontSize={setFontSize} />
      {error && <div className="toast error"><CircleAlert size={14} />{error}</div>}
      {notice && <div className="toast"><Check size={14} />{notice}</div>}
    </div>
  )
}
