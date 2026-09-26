import { useEffect, useRef, useState, type ReactElement } from 'react'
import { Languages, Moon, Plus, ShieldCheck, Sun, X } from 'lucide-react'
import type { AgentProfile, RelayPolicy } from '@relay/protocol'
import type { DesktopSettings } from '../../shared/api.js'
import { useAppStore } from './store.js'
import { ProfileFields } from './profile-editor.js'
import {
  ProviderIcon,
  providerMetadata,
  providerOrder,
  runtimeForProvider,
  newProfileFor,
  type ProviderIconId,
  type Theme,
  type Translator,
} from './ui.js'

const MAX_FONT_SIZE = 20
const MIN_FONT_SIZE = 14

type Selection = 'general' | 'agents' | 'workspace' | 'appearance' | 'advanced' | `agent:${ProviderIconId}`

/**
 * Centered Settings card (docs/ui.md 17.1): never a full-screen page, with a
 * left option list and the selected item's controls on the right of the same card.
 */
export function SettingsSheet({
  t,
  theme,
  setTheme,
  fontSize,
  setFontSize,
}: {
  t: Translator
  theme: Theme
  setTheme(theme: Theme): void
  fontSize: number
  setFontSize(fontSize: number): void
}) {
  const { snapshot, settingsOpen, setSettingsOpen, locale, setLocale, setNotice, refresh } = useAppStore()
  const [settings, setSettings] = useState<DesktopSettings | undefined>(snapshot?.settings)
  const [scope, setScope] = useState('global')
  const [selection, setSelection] = useState<Selection>('general')
  const [draft, setDraft] = useState<AgentProfile>()
  const [dirty, setDirty] = useState(false)
  const latest = useRef<DesktopSettings | undefined>(undefined)
  const revision = useRef(0)

  useEffect(() => {
    if (snapshot?.settings === latest.current) return
    latest.current = snapshot?.settings
    if (!dirty) setSettings(snapshot?.settings)
  }, [snapshot?.settings, dirty])

  // Keep the detail pane pointing at a real profile as the snapshot changes.
  useEffect(() => {
    if (!snapshot) return
    setDraft((current) => {
      if (!current) return current
      return snapshot.profiles.find((profile) => profile.id === current.id) ?? current
    })
  }, [snapshot])

  if (!settingsOpen || !settings || !snapshot) return null

  const workspaces = [...new Set(snapshot.sessions.map((session) => session.cwd))]
  const policy: RelayPolicy = scope === 'global'
    ? settings.policy
    : ({ ...settings.policy, ...settings.workspaceOverrides[scope] } as RelayPolicy)

  const persist = (next: DesktopSettings) => {
    const current = ++revision.current
    setSettings(next)
    setDirty(true)
    void window.relay
      .saveSettings(next)
      .then((saved) => {
        if (revision.current !== current) return
        setSettings(saved)
        setDirty(false)
      })
      .catch((error: unknown) => setNotice(error instanceof Error ? error.message : String(error)))
  }
  const updatePolicy = (next: Partial<RelayPolicy>) =>
    persist(
      scope === 'global'
        ? { ...settings, policy: { ...settings.policy, ...next } }
        : {
            ...settings,
            workspaceOverrides: {
              ...settings.workspaceOverrides,
              [scope]: { ...settings.workspaceOverrides[scope], ...next },
            },
          },
    )

  const saveDraft = () => {
    if (!draft) return
    void window.relay
      .saveProfile(draft)
      .then(async () => {
        await refresh()
        setNotice(t('agents.profileSaved'))
        setDraft(undefined)
      })
      .catch((error: unknown) => setNotice(error instanceof Error ? error.message : String(error)))
  }

  const selectedProvider = selection.startsWith('agent:')
    ? (selection.slice('agent:'.length) as ProviderIconId)
    : undefined

  const navButton = (id: Selection, label: string, icon?: ReactElement) => (
    <button className={selection === id ? 'selected' : ''} onClick={() => { setSelection(id); setDraft(undefined) }} key={id}>
      {icon}{label}
    </button>
  )

  return (
    <div
      className="settings-backdrop"
      onMouseDown={(event) => event.target === event.currentTarget && setSettingsOpen(false)}
    >
      <aside className="settings-sheet" onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-label={t('settings.title')}>
        <header>
          <h1>{t('settings.title')}</h1>
          <button className="plain-icon" aria-label={t('action.close')} onClick={() => setSettingsOpen(false)}><X size={16} /></button>
        </header>
        <div className="settings-layout">
          <nav className="settings-navigation">
            {navButton('general', t('settings.general'))}
            {navButton('agents', t('settings.providerAgents'))}
            {/* Provider sub-items (docs/ui.md 17.2) */}
            {providerOrder.map((provider) => {
              const runtime = runtimeForProvider(provider, snapshot.runtimes)
              const count = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id).length
              return (
                <button
                  className={`settings-subitem${selection === `agent:${provider}` ? ' selected' : ''}`}
                  onClick={() => { setSelection(`agent:${provider}`); setDraft(undefined) }}
                  key={provider}
                >
                  <img className="provider-icon" src={providerMetadata[provider].src} alt="" />
                  <span>{providerMetadata[provider].label}</span>
                  <b>{count || ''}</b>
                </button>
              )
            })}
            {navButton('workspace', t('settings.workspaceSection'), <ShieldCheck size={14} />)}
            {navButton('appearance', t('settings.appearance'), <Sun size={14} />)}
            {navButton('advanced', t('settings.advanced'))}
          </nav>

          <div className="settings-content">
            {selection === 'general' && (
              <section>
                <div className="setting-heading">
                  <Languages size={16} />
                  <div><strong>{t('settings.language')}</strong><span>English / 简体中文</span></div>
                </div>
                <div className="segmented">
                  <button className={locale === 'en' ? 'selected' : ''} onClick={() => setLocale('en')}>English</button>
                  <button className={locale === 'zh-CN' ? 'selected' : ''} onClick={() => setLocale('zh-CN')}>简体中文</button>
                </div>
              </section>
            )}

            {selection === 'appearance' && (
              <section>
                <div className="setting-heading"><Sun size={16} /><strong>{t('settings.appearance')}</strong></div>
                <div className="segmented">
                  {(['system', 'light', 'dark'] as Theme[]).map((value) => (
                    <button className={theme === value ? 'selected' : ''} onClick={() => setTheme(value)} key={value}>
                      {value === 'dark' && <Moon size={12} />}{t(`settings.${value}`)}
                    </button>
                  ))}
                </div>
                <label className="appearance-setting">
                  <div><strong>{t('settings.fontSize')}</strong><span>{t('settings.fontSizeHint')}</span></div>
                  <select value={fontSize} onChange={(event) => setFontSize(Number(event.target.value))}>
                    {Array.from({ length: MAX_FONT_SIZE - MIN_FONT_SIZE + 1 }, (_, index) => MIN_FONT_SIZE + index).map((value) => (
                      <option value={value} key={value}>{value} px</option>
                    ))}
                  </select>
                </label>
              </section>
            )}

            {selection === 'agents' && (
              <section>
                <div className="setting-heading">
                  <div><strong>{t('settings.providerAgents')}</strong><span>{t('agents.subtitle')}</span></div>
                </div>
                <div className="provider-rows provider-table">
                  {providerOrder.map((provider) => {
                    const runtime = runtimeForProvider(provider, snapshot.runtimes)
                    const profiles = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id)
                    return (
                      <div className="provider-row" key={provider}>
                        <img className="provider-icon" src={providerMetadata[provider].src} alt="" />
                        <span className="provider-row-name">{providerMetadata[provider].label}</span>
                        <span className={`provider-row-status${runtime ? ' detected' : ''}`}>
                          {runtime ? t('agents.detected') : t('agents.notInstalled')}
                        </span>
                        <span className="provider-row-count">{profiles.length}</span>
                        <button
                          className="secondary"
                          disabled={!runtime}
                          onClick={() => runtime && setDraft(newProfileFor(provider, runtime.id))}
                        >
                          <Plus size={12} />{t('agents.add')}
                        </button>
                      </div>
                    )
                  })}
                </div>
                <p className="settings-note">{t('agents.subtitle')}</p>
              </section>
            )}

            {selectedProvider && (() => {
              const runtime = runtimeForProvider(selectedProvider, snapshot.runtimes)
              const profiles = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id)
              const metadata = providerMetadata[selectedProvider]
              return (
                <section>
                  <div className="setting-heading">
                    <img className="provider-icon provider-icon-lg" src={metadata.src} alt="" />
                    <div><strong>{metadata.label}</strong>
                      <span>{runtime ? `${runtime.adapterId}${runtime.version ? ` · ${runtime.version}` : ''}` : t('agents.notInstalled')}</span>
                    </div>
                  </div>

                  {!profiles.length && <p className="settings-note">{t('settings.noProfiles')}</p>}
                  {profiles.map((profile) => (
                    <div className="agent-card" key={profile.id}>
                      <div className="agent-card-head">
                        <div><strong>{profile.name}</strong>
                          <span>{[profile.model, profile.reasoning].filter(Boolean).join(' · ') || t('agents.default')}</span>
                        </div>
                        <button className="secondary" onClick={() => setDraft(profile)}>{t('agents.edit')}</button>
                      </div>
                    </div>
                  ))}

                  {!runtime && <p className="settings-note">{t('agents.notInstalled')}</p>}
                  {runtime && (
                    <div className="setting-heading section-action">
                      <div><strong>{t('settings.providerAgents')}</strong></div>
                      <button className="secondary" onClick={() => setDraft(newProfileFor(selectedProvider, runtime.id))}>
                        <Plus size={12} />{t('agents.create')}
                      </button>
                    </div>
                  )}

                  {draft && (
                    <form
                      className="agent-detail"
                      onSubmit={(event) => { event.preventDefault(); saveDraft() }}
                    >
                      <ProfileFields profile={draft} runtimes={snapshot.runtimes} t={t} onChange={setDraft} layout="rows" />
                      <div className="agent-detail-actions">
                        <button type="button" className="secondary" onClick={() => setDraft(undefined)}>{t('action.close')}</button>
                        <button className="primary">{t('action.save')}</button>
                      </div>
                    </form>
                  )}
                </section>
              )
            })()}

            {selection === 'workspace' && (
              <section>
                <div className="setting-heading">
                  <ShieldCheck size={16} />
                  <div><strong>{t('settings.policy')}</strong>
                    <span>{scope === 'global' ? t('settings.global') : t('settings.workspace')}</span>
                  </div>
                </div>
                <label className="setting-row">
                  <span>{t('settings.scope')}</span>
                  <select value={scope} onChange={(event) => setScope(event.target.value)}>
                    <option value="global">{t('settings.global')}</option>
                    {workspaces.map((workspace) => <option value={workspace} key={workspace}>{workspace}</option>)}
                  </select>
                </label>
                <label className="setting-row">
                  <span>{t('settings.maxRuns')}</span>
                  <input type="number" min="1" max="16" value={policy.maxConcurrentRuns}
                    onChange={(event) => { const value = Number(event.target.value); if (Number.isInteger(value)) updatePolicy({ maxConcurrentRuns: value }) }} />
                </label>
                <label className="setting-row">
                  <span>{t('settings.maxWriters')}</span>
                  <input type="number" min="1" max="8" value={policy.maxConcurrentWriters}
                    onChange={(event) => { const value = Number(event.target.value); if (Number.isInteger(value)) updatePolicy({ maxConcurrentWriters: value }) }} />
                </label>
                {([
                  ['requireWorktreeForParallelWriters', 'settings.requireWorktree'],
                  ['allowWrite', 'settings.allowWrite'],
                  ['allowCommands', 'settings.allowCommands'],
                  ['allowNetwork', 'settings.allowNetwork'],
                ] as const).map(([key, label]) => (
                  <label className="setting-row" key={key}>
                    <span>{t(label)}</span>
                    <input type="checkbox" checked={policy[key]} onChange={() => updatePolicy({ [key]: !policy[key] })} />
                  </label>
                ))}
              </section>
            )}

            {selection === 'advanced' && (
              <section>
                <div className="setting-heading">
                  <div><strong>{t('settings.advanced')}</strong><span>{t('settings.advancedHint')}</span></div>
                </div>
                {!snapshot.diagnostics.length && <p className="settings-note">{t('settings.diagnosticsEmpty')}</p>}
                {snapshot.diagnostics.length > 0 && (
                  <ul className="diagnostics-list">
                    {snapshot.diagnostics.map((diagnostic, index) => <li key={index}>{diagnostic}</li>)}
                  </ul>
                )}
              </section>
            )}
          </div>
        </div>
      </aside>
    </div>
  )
}
