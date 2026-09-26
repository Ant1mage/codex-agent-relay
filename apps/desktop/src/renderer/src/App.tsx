import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Check,
  ChevronRight,
  CircleAlert,
  FileDiff,
  Languages,
  Moon,
  Plus,
  RefreshCw,
  Settings,
  ShieldCheck,
  Square,
  Sun,
  X,
} from 'lucide-react'
import { createTranslator, type TranslationKey } from '@relay/i18n'
import type { AgentProfile, RelayEvent, RelayPolicy, RunStatus, Step } from '@relay/protocol'
import type { DesktopRunView, DesktopSettings } from '../../shared/api.js'
import { useAppStore } from './store.js'

type Translator = (key: TranslationKey) => string
type Inspector = 'changes' | 'raw' | undefined
type Theme = 'system' | 'light' | 'dark'
type ConsoleKind = 'read' | 'search' | 'edit' | 'command' | 'test' | 'result' | 'error' | 'status'

function elapsed(start: string, end?: string): string {
  const total = Math.max(0, new Date(end ?? Date.now()).getTime() - new Date(start).getTime())
  const seconds = Math.floor(total / 1_000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  return `${minutes}m ${seconds % 60}s`
}

function eventKind(event: RelayEvent): ConsoleKind | undefined {
  if (event.type === 'tool/read') return 'read'
  if (event.type === 'tool/search') return 'search'
  if (event.type === 'tool/edit') return 'edit'
  if (event.type === 'tool/command') return 'command'
  if (event.type === 'test/result') return 'test'
  if (event.type === 'tool/result' || event.type === 'worker/completed') return 'result'
  if (event.type === 'worker/failed' || event.type === 'worker/orphaned') return 'error'
  if (
    event.type === 'worker/started' ||
    event.type === 'worker/status' ||
    event.type === 'worker/message' ||
    event.type === 'worker/cancelled' ||
    event.type === 'worker/interrupted' ||
    event.type === 'run/awaiting_host' ||
    event.type === 'run/accepted'
  ) return 'status'
  return undefined
}

function eventSummary(event: RelayEvent): string {
  const data = event.data
  if (!data || typeof data !== 'object') return String(data ?? event.type)
  const record = data as Record<string, unknown>
  const value = record.path ?? record.file ?? record.command ?? record.query ?? record.summary ??
    record.text ?? record.message ?? record.result ?? record.status ?? record.tool
  if (value !== undefined) return typeof value === 'string' ? value : JSON.stringify(value)
  if (event.type === 'worker/started') {
    const worker = record.worker as Record<string, unknown> | undefined
    return worker?.runtimeId ? String(worker.runtimeId) : 'Worker started'
  }
  if (event.type === 'run/awaiting_host') return 'Worker finished; waiting for Codex review'
  if (event.type === 'run/accepted') return 'Accepted by Codex'
  return event.type
}

function StatusLabel({ status, t }: { status: RunStatus; t: Translator }) {
  return <span className={`status-label ${status}`}><i />{t(`run.status.${status}`)}</span>
}

function Sidebar({ t }: { t: Translator }) {
  const { snapshot, selectedSessionId, selectSession, setSettingsOpen } = useAppStore()
  return (
    <aside className="sidebar">
      <div className="window-drag" />
      <div className="sidebar-brand">
        <strong>Relay</strong>
        <button className="plain-icon" aria-label={t('settings.title')} onClick={() => setSettingsOpen(true)}><Settings size={15} /></button>
      </div>
      <div className="sidebar-section-title">{t('sessions.title')}</div>
      <div className="session-list">
        {snapshot?.sessions.map((session) => (
          <button className={session.id === selectedSessionId ? 'session-name selected' : 'session-name'} key={session.id} onClick={() => selectSession(session.id)} title={session.displayName}>
            {session.displayName}
          </button>
        ))}
        {!snapshot?.sessions.length && <p className="sidebar-empty">{t('sessions.empty')}</p>}
      </div>
    </aside>
  )
}

interface StepItem { view: DesktopRunView; step: Step }

function StepNavigator({ items, t }: { items: StepItem[]; t: Translator }) {
  const { selectedRunId, selectedStepId, selectStep } = useAppStore()
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
        {ordered.map(({ view, step }, index) => {
          const selected = view.run.id === selectedRunId && step.id === selectedStepId
          return (
            <div className="step-link" key={step.id}>
              {index > 0 && <ChevronRight size={13} />}
              <button ref={selected ? currentRef : undefined} className={selected ? 'step-node selected' : `step-node ${step.status}`} onClick={() => selectStep(view.run.id, step.id)}>
                <span>{t('steps.step')} {index + 1}</span>
                <strong>{view.run.profileId}</strong>
                <i />
              </button>
            </div>
          )
        })}
      </div>
    </section>
  )
}

function Console({ view, step, t, openInspector }: { view: DesktopRunView; step: Step; t: Translator; openInspector(inspector: Exclude<Inspector, undefined>): void }) {
  const { setNotice } = useAppStore()
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const events = view.events.filter((event) => (!event.stepId || event.stepId === step.id) && eventKind(event) !== undefined)
  const changeCount = events.filter((event) => event.type === 'tool/edit').length
  const rawCount = view.events.filter((event) => (!event.stepId || event.stepId === step.id) && event.nativeEvent !== undefined).length

  return (
    <section className="console-panel">
      <header className="console-header">
        <div><strong>{t('console.title')}</strong><span>{view.run.profileId} · {t('steps.iteration')} {step.iteration} · {elapsed(step.createdAt, worker?.endedAt)}</span></div>
        <div className="console-actions">
          {changeCount > 0 && <button onClick={() => openInspector('changes')}><FileDiff size={13} />{t('console.changes')} <b>{changeCount}</b></button>}
          <button onClick={() => openInspector('raw')}>{t('console.rawOutput')} <b>{rawCount}</b></button>
          {(step.status === 'running' || step.status === 'starting') && worker && (
            <button className="stop-button" onClick={async () => {
              const response = await window.relay.cancelWorker(worker.id)
              setNotice(response.accepted ? t('runs.cancelQueued') : response.message)
            }}><Square size={11} />{t('action.cancel')}</button>
          )}
        </div>
      </header>
      <div className="console-body">
        {!events.length && <div className="console-empty">{t('console.empty')}</div>}
        {events.map((event) => {
          const kind = eventKind(event)!
          return <div className={`console-row ${kind}`} key={event.id}><time>{new Date(event.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</time><span className="console-kind">{t(`console.${kind}`)}</span><code>{eventSummary(event)}</code></div>
        })}
      </div>
    </section>
  )
}

function InspectorSheet({ inspector, view, step, t, close }: { inspector: Exclude<Inspector, undefined>; view: DesktopRunView; step: Step; t: Translator; close(): void }) {
  const events = view.events.filter((event) => !event.stepId || event.stepId === step.id)
  const changes = events.filter((event) => event.type === 'tool/edit')
  const raw = events.filter((event) => event.nativeEvent !== undefined)
  return (
    <div className="sheet-backdrop" onMouseDown={(event) => event.target === event.currentTarget && close()}>
      <aside className="inspector-sheet">
        <header><div><span>{view.run.profileId}</span><h2>{inspector === 'changes' ? t('console.changes') : t('console.rawOutput')}</h2></div><button className="plain-icon" onClick={close}><X size={16} /></button></header>
        {inspector === 'changes' ? <div className="change-list">{!changes.length && <p>{t('console.noChanges')}</p>}{changes.map((event) => <article key={event.id}><strong>{eventSummary(event)}</strong><pre>{JSON.stringify(event.data, null, 2)}</pre></article>)}</div>
          : <div className="raw-output">{!raw.length && <p>{t('console.noRawOutput')}</p>}{raw.map((event) => <article key={event.id}><time>{event.timestamp}</time><strong>{event.type}</strong><pre>{JSON.stringify(event.nativeEvent, null, 2)}</pre></article>)}</div>}
      </aside>
    </div>
  )
}

function SessionWorkspace({ t }: { t: Translator }) {
  const { snapshot, selectedSessionId, selectedRunId, selectedStepId, loading, refresh } = useAppStore()
  const [inspector, setInspector] = useState<Inspector>()
  const session = snapshot?.sessions.find((item) => item.id === selectedSessionId)
  const views = useMemo(() => snapshot?.runs.filter((item) => item.run.hostSessionId === selectedSessionId) ?? [], [snapshot, selectedSessionId])
  const stepItems = views.flatMap((view) => view.steps.map((step) => ({ view, step })))
  const selectedView = views.find((view) => view.run.id === selectedRunId) ?? views[0]
  const selectedStep = selectedView?.steps.find((step) => step.id === selectedStepId) ?? selectedView?.steps[0]

  if (!session) return <main className="workspace empty-workspace"><h1>{t('sessions.empty')}</h1><p>{t('sessions.emptyHint')}</p></main>
  return (
    <main className="workspace">
      <header className="workspace-heading">
        <div><span>{session.displayName}</span><h1>{selectedView?.run.task ?? session.displayName}</h1><p>{session.cwd}</p></div>
        <div className="heading-actions">{selectedView && <StatusLabel status={selectedView.run.status} t={t} />}<button className="plain-icon" title={t('common.refresh')} onClick={() => void refresh()}><RefreshCw className={loading ? 'spin' : ''} size={15} /></button></div>
      </header>
      {stepItems.length ? <><StepNavigator items={stepItems} t={t} />{selectedView && selectedStep && <Console view={selectedView} step={selectedStep} t={t} openInspector={setInspector} />}</> : <div className="workspace-placeholder">{t('runs.empty')}</div>}
      {inspector && selectedView && selectedStep && <InspectorSheet inspector={inspector} view={selectedView} step={selectedStep} t={t} close={() => setInspector(undefined)} />}
    </main>
  )
}

function ProfileEditor({ profile, runtimes, t, close }: { profile: AgentProfile; runtimes: NonNullable<ReturnType<typeof useAppStore.getState>['snapshot']>['runtimes']; t: Translator; close(): void }) {
  const { refresh, setNotice } = useAppStore()
  const [draft, setDraft] = useState(profile)
  return (
    <div className="modal-backdrop">
      <form className="profile-editor" onSubmit={(event) => { event.preventDefault(); void window.relay.saveProfile(draft).then(async () => { await refresh(); setNotice(t('agents.profileSaved')); close() }) }}>
        <header><h2>{t('agents.edit')}</h2><button type="button" className="plain-icon" onClick={close}><X size={16} /></button></header>
        <label><span>{t('agents.name')}</span><input required value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
        <label><span>Runtime</span><select value={draft.runtimeId} onChange={(event) => setDraft({ ...draft, runtimeId: event.target.value })}>{runtimes.map((runtime) => <option value={runtime.id} key={runtime.id}>{runtime.adapterId} · {runtime.version ?? runtime.executablePath}</option>)}</select></label>
        <label><span>{t('agents.description')}</span><textarea value={draft.description} onChange={(event) => setDraft({ ...draft, description: event.target.value })} /></label>
        <div className="capability-grid">{([['readWorkspace', 'agents.read'], ['writeWorkspace', 'agents.write'], ['executeCommands', 'agents.shell'], ['networkAccess', 'agents.network']] as const).map(([key, label]) => <label className="check-row" key={key}><input type="checkbox" checked={draft.capabilities[key]} onChange={() => setDraft({ ...draft, capabilities: { ...draft.capabilities, [key]: !draft.capabilities[key] } })} /><span>{t(label)}</span></label>)}</div>
        <footer><button type="button" className="secondary" onClick={close}>{t('action.close')}</button><button className="primary">{t('action.save')}</button></footer>
      </form>
    </div>
  )
}

function SettingsSheet({ t, theme, setTheme }: { t: Translator; theme: Theme; setTheme(theme: Theme): void }) {
  const { snapshot, settingsOpen, setSettingsOpen, locale, setLocale, setNotice } = useAppStore()
  const [settings, setSettings] = useState<DesktopSettings | undefined>(snapshot?.settings)
  const [scope, setScope] = useState('global')
  const [editing, setEditing] = useState<AgentProfile>()
  useEffect(() => setSettings(snapshot?.settings), [snapshot?.settings])
  if (!settingsOpen || !settings || !snapshot) return null
  const workspaces = [...new Set(snapshot.sessions.map((session) => session.cwd))]
  const policy = scope === 'global' ? settings.policy : { ...settings.policy, ...settings.workspaceOverrides[scope] }
  const updatePolicy = (next: Partial<RelayPolicy>) => setSettings(scope === 'global' ? { ...settings, policy: { ...settings.policy, ...next } } : { ...settings, workspaceOverrides: { ...settings.workspaceOverrides, [scope]: { ...settings.workspaceOverrides[scope], ...next } } })
  const createProfile = () => {
    const runtime = snapshot.runtimes[0]
    if (!runtime) return
    setEditing({ id: `profile-${Date.now()}`, name: runtime.adapterId, runtimeId: runtime.id, description: '', capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: false }, enabled: true })
  }
  return (
    <div className="sheet-backdrop">
      <aside className="settings-sheet">
        <header><h1>{t('settings.title')}</h1><button className="plain-icon" onClick={() => setSettingsOpen(false)}><X size={16} /></button></header>
        <section><div className="setting-heading"><Languages size={16} /><div><strong>{t('settings.language')}</strong><span>English / 简体中文</span></div></div><div className="segmented"><button className={locale === 'en' ? 'selected' : ''} onClick={() => setLocale('en')}>English</button><button className={locale === 'zh-CN' ? 'selected' : ''} onClick={() => setLocale('zh-CN')}>简体中文</button></div></section>
        <section><div className="setting-heading"><Sun size={16} /><strong>{t('settings.appearance')}</strong></div><div className="segmented">{(['system', 'light', 'dark'] as Theme[]).map((value) => <button className={theme === value ? 'selected' : ''} onClick={() => setTheme(value)} key={value}>{value === 'dark' && <Moon size={12} />}{t(`settings.${value}`)}</button>)}</div></section>
        <section><div className="setting-heading section-action"><div><strong>{t('settings.profiles')}</strong><span>{snapshot.runtimes.length} runtimes · {snapshot.profiles.length} profiles</span></div><button className="secondary" onClick={createProfile} disabled={!snapshot.runtimes.length}><Plus size={12} />{t('agents.create')}</button></div><div className="profile-list">{snapshot.profiles.map((profile) => { const runtime = snapshot.runtimes.find((item) => item.id === profile.runtimeId); return <button key={profile.id} onClick={() => setEditing(profile)}><div><strong>{profile.name}</strong><span>{runtime?.adapterId} · {runtime?.version ?? t('common.notAvailable')}</span></div><ChevronRight size={14} /></button> })}</div></section>
        <section>
          <div className="setting-heading"><ShieldCheck size={16} /><div><strong>{t('settings.policy')}</strong><span>{scope === 'global' ? t('settings.global') : t('settings.workspace')}</span></div></div>
          <label className="setting-row"><span>{t('settings.scope')}</span><select value={scope} onChange={(event) => setScope(event.target.value)}><option value="global">{t('settings.global')}</option>{workspaces.map((workspace) => <option value={workspace} key={workspace}>{workspace}</option>)}</select></label>
          <label className="setting-row"><span>{t('settings.maxRuns')}</span><input type="number" min="1" max="16" value={policy.maxConcurrentRuns} onChange={(event) => updatePolicy({ maxConcurrentRuns: Number(event.target.value) })} /></label>
          <label className="setting-row"><span>{t('settings.maxWriters')}</span><input type="number" min="1" max="8" value={policy.maxConcurrentWriters} onChange={(event) => updatePolicy({ maxConcurrentWriters: Number(event.target.value) })} /></label>
          {([['requireWorktreeForParallelWriters', 'settings.requireWorktree'], ['allowWrite', 'settings.allowWrite'], ['allowCommands', 'settings.allowCommands'], ['allowNetwork', 'settings.allowNetwork']] as const).map(([key, label]) => <label className="setting-row" key={key}><span>{t(label)}</span><input type="checkbox" checked={policy[key]} onChange={() => updatePolicy({ [key]: !policy[key] })} /></label>)}
          <button className="primary save-settings" onClick={async () => { const saved = await window.relay.saveSettings(settings); setSettings(saved); setNotice(t('settings.saved')) }}>{t('action.save')}</button>
        </section>
      </aside>
      {editing && <ProfileEditor profile={editing} runtimes={snapshot.runtimes} t={t} close={() => setEditing(undefined)} />}
    </div>
  )
}

export function App() {
  const { locale, refresh, error, notice, setNotice } = useAppStore()
  const [theme, setThemeState] = useState<Theme>(() => (localStorage.getItem('relay.theme') as Theme | null) ?? 'system')
  const t = createTranslator(locale)
  const setTheme = (next: Theme) => { localStorage.setItem('relay.theme', next); setThemeState(next) }
  useEffect(() => { document.documentElement.dataset.theme = theme; if (theme === 'system') document.documentElement.removeAttribute('data-theme') }, [theme])
  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 2_000); return () => window.clearInterval(timer) }, [refresh])
  useEffect(() => { if (!notice) return; const timer = window.setTimeout(() => setNotice(undefined), 3_500); return () => window.clearTimeout(timer) }, [notice, setNotice])
  return <div className="app-shell"><Sidebar t={t} /><SessionWorkspace t={t} /><SettingsSheet t={t} theme={theme} setTheme={setTheme} />{error && <div className="toast error"><CircleAlert size={14} />{error}</div>}{notice && <div className="toast"><Check size={14} />{notice}</div>}</div>
}
