import { useEffect, useMemo, useState } from 'react'
import {
  Activity,
  Bot,
  Braces,
  Check,
  ChevronRight,
  CircleAlert,
  Clock3,
  Command,
  FilePenLine,
  FileSearch,
  Globe2,
  Languages,
  LoaderCircle,
  Play,
  Plus,
  Pencil,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Square,
  TerminalSquare,
  Users,
  X,
} from 'lucide-react'
import { createTranslator, type TranslationKey } from '@relay/i18n'
import type { AgentProfile, RelayEvent, RelayPolicy, RunStatus } from '@relay/protocol'
import type { DesktopRunView, DesktopSettings } from '../../shared/api.js'
import { useAppStore } from './store.js'

type Translator = (key: TranslationKey) => string

const statusIcon: Record<RunStatus, typeof Clock3> = {
  queued: Clock3,
  starting: LoaderCircle,
  running: Play,
  completed: Check,
  failed: X,
  cancelled: Square,
  handed_off: ChevronRight,
}

function elapsed(start: string, end?: string): string {
  const total = Math.max(0, new Date(end ?? Date.now()).getTime() - new Date(start).getTime())
  const seconds = Math.floor(total / 1_000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  return `${minutes}m ${seconds % 60}s`
}

function eventIcon(event: RelayEvent): typeof Activity {
  if (event.type === 'tool/read') return FileSearch
  if (event.type === 'tool/edit') return FilePenLine
  if (event.type === 'tool/search') return Globe2
  if (event.type === 'tool/command') return TerminalSquare
  if (event.type === 'worker/failed') return CircleAlert
  if (event.type === 'worker/completed') return Check
  return Activity
}

function Sidebar({ t }: { t: Translator }) {
  const { view, setView } = useAppStore()
  const items = [
    { id: 'sessions' as const, label: t('nav.sessions'), icon: Users },
    { id: 'agents' as const, label: t('nav.agents'), icon: Bot },
    { id: 'settings' as const, label: t('nav.settings'), icon: Settings2 },
  ]
  return (
    <aside className="sidebar">
      <div className="traffic-space" />
      <div className="brand">
        <div className="brand-mark"><Braces size={19} /></div>
        <div><strong>Relay</strong><span>{t('app.tagline')}</span></div>
      </div>
      <nav>
        {items.map(({ id, label, icon: Icon }) => (
          <button className={view === id ? 'nav-item active' : 'nav-item'} key={id} onClick={() => setView(id)}>
            <Icon size={17} /> <span>{label}</span>
          </button>
        ))}
      </nav>
      <div className="sidebar-foot">
        <span className="pulse-dot" />
        <span>Core online</span>
      </div>
    </aside>
  )
}

function Header({ title, subtitle, t }: { title: string; subtitle: string; t: Translator }) {
  const { loading, refresh } = useAppStore()
  return (
    <header className="page-header">
      <div><h1>{title}</h1><p>{subtitle}</p></div>
      <button className="icon-button" title={t('common.refresh')} onClick={() => void refresh()}>
        <RefreshCw size={16} className={loading ? 'spin' : ''} />
      </button>
    </header>
  )
}

function StatusPill({ status, t }: { status: RunStatus; t: Translator }) {
  const Icon = statusIcon[status]
  return <span className={`status-pill ${status}`}><Icon size={12} />{t(`run.status.${status}`)}</span>
}

function EventTimeline({ run, t }: { run: DesktopRunView; t: Translator }) {
  return (
    <section className="timeline-panel">
      <div className="panel-title"><Activity size={15} /><span>{t('runs.events')}</span><b>{run.events.length}</b></div>
      <div className="timeline">
        {run.events.map((event) => {
          const Icon = eventIcon(event)
          const data = event.data as Record<string, unknown> | null
          const summary = data && typeof data === 'object'
            ? String(data.text ?? data.summary ?? data.message ?? data.path ?? data.tool ?? event.type)
            : event.type
          return (
            <div className="event-row" key={event.id}>
              <time>{new Date(event.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</time>
              <span className={`event-icon ${event.type.replace('/', '-')}`}><Icon size={13} /></span>
              <div><strong>{event.type}</strong><p>{summary}</p></div>
            </div>
          )
        })}
      </div>
    </section>
  )
}

function SessionsPage({ t }: { t: Translator }) {
  const { snapshot, selectedSessionId, selectedRunId, selectSession, selectRun, setNotice } = useAppStore()
  const session = snapshot?.sessions.find((item) => item.id === selectedSessionId)
  const runs = useMemo(
    () => snapshot?.runs.filter((item) => item.run.hostSessionId === selectedSessionId) ?? [],
    [snapshot, selectedSessionId],
  )
  const selectedRun = runs.find((item) => item.run.id === selectedRunId) ?? runs[0]

  return (
    <main className="page">
      <Header title={t('sessions.title')} subtitle={t('sessions.subtitle')} t={t} />
      {!snapshot?.sessions.length ? (
        <div className="empty-state"><div className="empty-icon"><Users size={25} /></div><h2>{t('sessions.empty')}</h2><p>{t('sessions.emptyHint')}</p></div>
      ) : (
        <div className="session-layout">
          <section className="session-list panel">
            {snapshot.sessions.map((item) => {
              const count = snapshot.runs.filter((run) => run.run.hostSessionId === item.id).length
              return (
                <button key={item.id} className={item.id === session?.id ? 'session-row selected' : 'session-row'} onClick={() => selectSession(item.id)}>
                  <span className={`session-status ${item.status}`} />
                  <div><strong>{item.displayName}</strong><p>{item.cwd}</p></div>
                  <span className="run-count">{count}</span>
                </button>
              )
            })}
          </section>
          <section className="session-detail">
            <div className="session-heading panel">
              <div><span className="eyebrow">CODEX SESSION</span><h2>{session?.displayName}</h2><p>{session?.cwd}</p></div>
              <span className={`host-status ${session?.status}`}>{session?.status === 'active' ? t('common.active') : session?.status === 'ended' ? t('common.ended') : t('common.offline')}</span>
            </div>
            <div className="section-label"><span>{t('runs.title')}</span><b>{runs.length}</b></div>
            {!runs.length ? <div className="empty-runs panel">{t('runs.empty')}</div> : (
              <div className="run-workspace">
                <div className="run-list">
                  {runs.map((item) => (
                    <button className={selectedRun?.run.id === item.run.id ? 'run-card selected' : 'run-card'} key={item.run.id} onClick={() => selectRun(item.run.id)}>
                      <div className="run-card-top"><StatusPill status={item.run.status} t={t} /><span>{elapsed(item.run.createdAt, item.workers[0]?.endedAt)}</span></div>
                      <strong>{item.run.task}</strong>
                      <p>{item.run.profileId} · {item.run.accessMode.replace('_', ' ')}</p>
                    </button>
                  ))}
                </div>
                {selectedRun && (
                  <div className="run-detail panel">
                    <div className="run-summary">
                      <div><StatusPill status={selectedRun.run.status} t={t} /><h3>{selectedRun.run.task}</h3></div>
                      {(selectedRun.run.status === 'running' || selectedRun.run.status === 'starting') && (
                        <button className="danger-button" onClick={async () => {
                          const worker = selectedRun.workers[0]
                          if (!worker) return
                          const response = await window.relay.cancelWorker(worker.id)
                          setNotice(response.accepted ? t('runs.cancelQueued') : response.message)
                        }}><Square size={13} />{t('action.cancel')}</button>
                      )}
                    </div>
                    <div className="run-meta">
                      <div><span>{t('runs.worker')}</span><strong>{selectedRun.workers[0]?.runtimeId ?? t('common.notAvailable')}</strong></div>
                      <div><span>{t('runs.duration')}</span><strong>{elapsed(selectedRun.run.createdAt, selectedRun.workers[0]?.endedAt)}</strong></div>
                      <div><span>Isolation</span><strong>{selectedRun.run.isolation}</strong></div>
                    </div>
                    <EventTimeline run={selectedRun} t={t} />
                  </div>
                )}
              </div>
            )}
          </section>
        </div>
      )}
    </main>
  )
}

function AgentsPage({ t }: { t: Translator }) {
  const { snapshot, refresh, setNotice } = useAppStore()
  const [editing, setEditing] = useState<AgentProfile | undefined>()
  const createProfile = () => {
    const runtime = snapshot?.runtimes[0]
    if (!runtime) return
    setEditing({
      id: `deepseek-${Date.now()}`,
      name: 'DeepSeek Agent',
      runtimeId: runtime.id,
      description: '',
      capabilities: {
        readWorkspace: true,
        writeWorkspace: false,
        executeCommands: false,
        networkAccess: false,
      },
      enabled: true,
    })
  }
  return (
    <main className="page">
      <Header title={t('agents.title')} subtitle={t('agents.subtitle')} t={t} />
      {!!snapshot?.runtimes.length && <div className="page-actions"><button className="primary-button" onClick={createProfile}><Plus size={13} />{t('agents.create')}</button></div>}
      {!snapshot?.runtimes.length ? <div className="empty-state"><div className="empty-icon"><Bot size={25} /></div><h2>{t('agents.empty')}</h2><p>{snapshot?.diagnostics[0]}</p></div> : (
        <div className="agent-grid">
          {snapshot.profiles.map((profile) => {
            const runtime = snapshot.runtimes.find((item) => item.id === profile.runtimeId)
            const caps = profile.capabilities
            return (
              <article className="agent-card panel" key={profile.id}>
                <div className="agent-card-head"><div className="agent-avatar"><Bot size={20} /></div><span className="available-dot">{t('agents.available')}</span></div>
                <h2>{profile.name}</h2><p>{profile.description}</p>
                <div className="runtime-line"><Command size={14} /><span>{runtime?.executablePath}</span></div>
                <div className="capabilities">
                  {caps.readWorkspace && <span><FileSearch size={12} />Read</span>}
                  {caps.writeWorkspace && <span><FilePenLine size={12} />Write</span>}
                  {caps.executeCommands && <span><TerminalSquare size={12} />Shell</span>}
                  {caps.networkAccess && <span><Globe2 size={12} />Network</span>}
                </div>
                <footer><span>{runtime?.version}</span><code>{profile.id}</code><button className="edit-button" onClick={() => setEditing(profile)}><Pencil size={11} />{t('agents.edit')}</button></footer>
              </article>
            )
          })}
        </div>
      )}
      {editing && (
        <div className="modal-backdrop">
          <form className="profile-editor panel" onSubmit={(event) => {
            event.preventDefault()
            void window.relay.saveProfile(editing).then(async () => {
              setEditing(undefined)
              await refresh()
              setNotice(t('agents.profileSaved'))
            })
          }}>
            <div className="editor-head"><div><span className="eyebrow">AGENT PROFILE</span><h2>{t('agents.edit')}</h2></div><button type="button" className="icon-button" onClick={() => setEditing(undefined)}><X size={15} /></button></div>
            <label className="field"><span>{t('agents.name')}</span><input required value={editing.name} onChange={(event) => setEditing({ ...editing, name: event.target.value })} /></label>
            <label className="field"><span>{t('agents.description')}</span><textarea value={editing.description ?? ''} onChange={(event) => setEditing({ ...editing, description: event.target.value })} /></label>
            <div className="editor-toggles">
              {([
                ['readWorkspace', 'agents.read'],
                ['writeWorkspace', 'agents.write'],
                ['executeCommands', 'agents.shell'],
                ['networkAccess', 'agents.network'],
              ] as const).map(([capability, label]) => (
                <label className="setting-row compact" key={capability}><span><strong>{t(label)}</strong></span><button type="button" className={editing.capabilities[capability] ? 'switch on' : 'switch'} onClick={() => setEditing({ ...editing, capabilities: { ...editing.capabilities, [capability]: !editing.capabilities[capability] } })}><span /></button></label>
              ))}
              <label className="setting-row compact"><span><strong>{t('agents.enabled')}</strong></span><button type="button" className={editing.enabled ? 'switch on' : 'switch'} onClick={() => setEditing({ ...editing, enabled: !editing.enabled })}><span /></button></label>
            </div>
            <div className="editor-actions"><button type="button" className="secondary-button" onClick={() => setEditing(undefined)}>{t('action.close')}</button><button className="primary-button" type="submit">{t('action.save')}</button></div>
          </form>
        </div>
      )}
    </main>
  )
}

function SettingsPage({ t }: { t: Translator }) {
  const { snapshot, locale, setLocale, setNotice } = useAppStore()
  const [settings, setSettings] = useState<DesktopSettings | undefined>(snapshot?.settings)
  const [scope, setScope] = useState('global')
  useEffect(() => setSettings(snapshot?.settings), [snapshot?.settings])
  if (!settings) return null
  const workspaces = [...new Set(snapshot?.sessions.map((session) => session.cwd) ?? [])]
  const policy = scope === 'global'
    ? settings.policy
    : { ...settings.policy, ...settings.workspaceOverrides[scope] }
  const updatePolicy = (next: Partial<RelayPolicy>) => {
    if (scope === 'global') {
      setSettings({ ...settings, policy: { ...settings.policy, ...next } })
      return
    }
    setSettings({
      ...settings,
      workspaceOverrides: {
        ...settings.workspaceOverrides,
        [scope]: { ...settings.workspaceOverrides[scope], ...next },
      },
    })
  }
  return (
    <main className="page settings-page">
      <Header title={t('settings.title')} subtitle={t('settings.subtitle')} t={t} />
      <section className="settings-card panel">
        <div className="settings-title"><Languages size={18} /><div><h2>{t('settings.language')}</h2><p>English / 简体中文</p></div></div>
        <div className="segmented">
          <button className={locale === 'en' ? 'active' : ''} onClick={() => setLocale('en')}>English</button>
          <button className={locale === 'zh-CN' ? 'active' : ''} onClick={() => setLocale('zh-CN')}>简体中文</button>
        </div>
      </section>
      <section className="settings-card panel">
        <div className="settings-title"><ShieldCheck size={18} /><div><h2>{t('settings.policy')}</h2><p>{scope === 'global' ? t('settings.global') : t('settings.workspace')}</p></div></div>
        <label className="setting-row"><span><strong>{t('settings.scope')}</strong><small>{t('settings.workspace')}</small></span><select className="scope-select" value={scope} onChange={(event) => setScope(event.target.value)}><option value="global">{t('settings.global')}</option>{workspaces.map((workspace) => <option value={workspace} key={workspace}>{workspace}</option>)}</select></label>
        <label className="setting-row"><span><strong>{t('settings.maxRuns')}</strong><small>1–16</small></span><input type="number" min="1" max="16" value={policy.maxConcurrentRuns} onChange={(event) => updatePolicy({ maxConcurrentRuns: Number(event.target.value) })} /></label>
        <label className="setting-row"><span><strong>{t('settings.maxWriters')}</strong><small>1–8</small></span><input type="number" min="1" max="8" value={policy.maxConcurrentWriters} onChange={(event) => updatePolicy({ maxConcurrentWriters: Number(event.target.value) })} /></label>
        <label className="setting-row"><span><strong>{t('settings.requireWorktree')}</strong><small>Protect shared workspaces</small></span><button className={policy.requireWorktreeForParallelWriters ? 'switch on' : 'switch'} onClick={() => updatePolicy({ requireWorktreeForParallelWriters: !policy.requireWorktreeForParallelWriters })}><span /></button></label>
        <label className="setting-row"><span><strong>{t('settings.allowWrite')}</strong></span><button className={policy.allowWrite ? 'switch on' : 'switch'} onClick={() => updatePolicy({ allowWrite: !policy.allowWrite })}><span /></button></label>
        <label className="setting-row"><span><strong>{t('settings.allowCommands')}</strong></span><button className={policy.allowCommands ? 'switch on' : 'switch'} onClick={() => updatePolicy({ allowCommands: !policy.allowCommands })}><span /></button></label>
        <label className="setting-row"><span><strong>{t('settings.allowNetwork')}</strong></span><button className={policy.allowNetwork ? 'switch on' : 'switch'} onClick={() => updatePolicy({ allowNetwork: !policy.allowNetwork })}><span /></button></label>
        <button className="primary-button" onClick={async () => {
          const saved = await window.relay.saveSettings(settings)
          setSettings(saved)
          setNotice(t('settings.saved'))
        }}>{t('action.save')}</button>
      </section>
    </main>
  )
}

export function App() {
  const { view, locale, refresh, error, notice, setNotice } = useAppStore()
  const t = createTranslator(locale)
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
  return (
    <div className="app-shell">
      <Sidebar t={t} />
      <div className="content-shell">
        {view === 'sessions' && <SessionsPage t={t} />}
        {view === 'agents' && <AgentsPage t={t} />}
        {view === 'settings' && <SettingsPage t={t} />}
      </div>
      {error && <div className="toast error"><CircleAlert size={15} />{error}</div>}
      {notice && <div className="toast"><Check size={15} />{notice}</div>}
    </div>
  )
}
