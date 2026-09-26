import { useEffect, useRef, useState, type ReactElement } from 'react'
import {
  Bot,
  ChevronRight,
  Languages,
  Plus,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Sun,
  X,
} from 'lucide-react'
import { Button } from './components/ui/button.js'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from './components/ui/dialog.js'
import {
  Item,
  ItemContent,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from './components/ui/item.js'
import { Input } from './components/ui/input.js'
import { Label } from './components/ui/label.js'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select.js'
import { Separator } from './components/ui/separator.js'
import { ToggleGroup, ToggleGroupItem } from './components/ui/toggle-group.js'
import { Switch } from './components/ui/switch.js'
import type { AgentProfile, Locale, RelayPolicy } from '@relay/protocol'
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

type Selection = 'general' | 'agents' | 'workspace' | 'advanced' | `agent:${ProviderIconId}`

/**
 * Number inputs keep their in-progress text locally. A controlled integer value
 * must not snap back while the user temporarily clears it to type a new value.
 */
function PolicyNumberInput({ value, min, max, onCommit }: {
  value: number
  min: number
  max: number
  onCommit(value: number): void
}) {
  const [draft, setDraft] = useState(String(value))
  useEffect(() => setDraft(String(value)), [value])
  const commit = () => {
    const next = Number(draft)
    if (Number.isInteger(next) && next >= min && next <= max) onCommit(next)
    else setDraft(String(value))
  }
  return (
    <Input
      type="number"
      min={min}
      max={max}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur() }}
    />
  )
}

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

  /*
   * A flat option list. There are only four top-level destinations, so grouping
   * headers added a layer that repeated the item labels ("General > General")
   * without telling the reader anything.
   */
  const navButton = (id: Selection, label: string, icon: ReactElement) => (
    <Item
      asChild
      key={id}
      size="sm"
      className={`rounded-md ${selection === id ? 'bg-accent-soft text-text' : 'text-muted'}`}
    >
      <button
        type="button"
        className="w-full text-left"
        onClick={() => { setSelection(id); setDraft(undefined) }}
      >
        <ItemMedia>{icon}</ItemMedia>
        <ItemContent>
          <ItemTitle className="text-[11px] font-normal">{label}</ItemTitle>
        </ItemContent>
      </button>
    </Item>
  )

  return (
    <Dialog open onOpenChange={(next) => { if (!next) setSettingsOpen(false) }}>
      <DialogContent showCloseButton={false} className="settings-sheet max-w-[760px] gap-0 p-0">
        <DialogHeader className="flex-row items-center justify-between gap-2 border-b border-line px-5 py-3">
          <DialogTitle className="text-base">{t('settings.title')}</DialogTitle>
          <DialogDescription className="sr-only">{t('settings.subtitle')}</DialogDescription>
          <Button variant="ghost" size="icon-xs" aria-label={t('action.close')} onClick={() => setSettingsOpen(false)}><X size={16} /></Button>
        </DialogHeader>
        <div className="settings-layout">
          <nav className="settings-navigation">
            <ItemGroup className="gap-0.5">
            {navButton('general', t('settings.general'), <Settings2 size={14} />)}
            {navButton('agents', t('settings.providerAgents'), <Bot size={14} />)}
            {/* Provider sub-items (docs/ui.md 17.2): indented under Agents */}
            {providerOrder.map((provider) => {
              const runtime = runtimeForProvider(provider, snapshot.runtimes)
              const count = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id).length
              const active = selection === `agent:${provider}`
              return (
                <Item
                  asChild
                  key={provider}
                  className={`ml-6 rounded-md ${active ? 'bg-accent-soft text-text' : 'text-muted'}`}
                >
                  <button
                    type="button"
                    className="w-full text-left"
                    onClick={() => { setSelection(`agent:${provider}`); setDraft(undefined) }}
                  >
                    <ItemMedia>
                      <img className="size-3.5" src={providerMetadata[provider].src} alt="" />
                    </ItemMedia>
                    <ItemContent>
                      <ItemTitle className="text-[10px] font-normal">{providerMetadata[provider].label}</ItemTitle>
                    </ItemContent>
                    {count > 0 && <span className="text-[9px] text-faint">{count}</span>}
                  </button>
                </Item>
              )
            })}

            {navButton('workspace', t('settings.workspaceSection'), <ShieldCheck size={14} />)}
            {navButton('advanced', t('settings.advanced'), <SlidersHorizontal size={14} />)}
            </ItemGroup>
          </nav>

          <div className="settings-content">
            {/*
              General holds every non-core preference: language, theme and text
              size. None of them is worth a top-level row of its own, and they
              are all "set once" choices rather than part of the delegation path.
            */}
            {selection === 'general' && (
              <div className="settings-preferences">
                <section className="settings-preference">
                  <h2>{t('settings.language')}</h2>
                  <div className="settings-preference-control">
                    <ToggleGroup
                      type="single"
                      value={locale}
                      onValueChange={(next) => { if (next) setLocale(next as Locale) }}
                      variant="outline"
                      size="sm"
                    >
                      <ToggleGroupItem value="en">English</ToggleGroupItem>
                      <ToggleGroupItem value="zh-CN">简体中文</ToggleGroupItem>
                    </ToggleGroup>
                  </div>
                </section>

                <section className="settings-preference">
                  <h2>{t('settings.appearance')}</h2>
                  <div className="settings-preference-control flex flex-col gap-4">
                    <ToggleGroup
                      type="single"
                      value={theme}
                      onValueChange={(next) => { if (next) setTheme(next as Theme) }}
                      variant="outline"
                      size="sm"
                    >
                      {(['system', 'light', 'dark'] as Theme[]).map((value) => (
                        <ToggleGroupItem value={value} key={value}>{t(`settings.${value}`)}</ToggleGroupItem>
                      ))}
                    </ToggleGroup>
                    <Separator />
                    <div className="flex items-center justify-between gap-4">
                      <Label htmlFor="relay-font-size">{t('settings.fontSize')}</Label>
                      <Select value={String(fontSize)} onValueChange={(next) => setFontSize(Number(next))}>
                        <SelectTrigger id="relay-font-size" className="w-[96px]"><SelectValue /></SelectTrigger>
                        <SelectContent>
                          {Array.from({ length: MAX_FONT_SIZE - MIN_FONT_SIZE + 1 }, (_, index) => MIN_FONT_SIZE + index).map((value) => (
                            <SelectItem value={String(value)} key={value}>{value} px</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                  </div>
                </section>
              </div>
            )}

            {/*
              The provider list is navigation: pick a provider to work on its
              profiles. Creating is an action, so it lives once in the selected
              provider's panel rather than on every row.
            */}
            {selection === 'agents' && (
              <section className="flex flex-col gap-4">
                <div className="setting-heading">
                  <div><strong>{t('settings.providerAgents')}</strong><span>{t('agents.subtitle')}</span></div>
                </div>
                {/* A selectable list of providers, so each row is an Item. */}
                <ItemGroup className="border-t border-line">
                  {providerOrder.map((provider) => {
                    const runtime = runtimeForProvider(provider, snapshot.runtimes)
                    const count = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id).length
                    return (
                      <Item asChild key={provider} className="rounded-none border-b border-line px-0">
                        <button
                          type="button"
                          className="w-full text-left"
                          onClick={() => { setSelection(`agent:${provider}`); setDraft(undefined) }}
                        >
                          <ItemMedia>
                            <img className="size-[15px]" src={providerMetadata[provider].src} alt="" />
                          </ItemMedia>
                          <ItemContent>
                            <ItemTitle>{providerMetadata[provider].label}</ItemTitle>
                          </ItemContent>
                          <span className={runtime ? 'text-[11px] text-ok' : 'text-[11px] text-faint'}>
                            {runtime ? t('agents.detected') : t('agents.notInstalled')}
                          </span>
                          <span className="text-[10px] text-faint">
                            {count > 0 ? `${count} ${t('settings.profilesSuffix')}` : ''}
                          </span>
                          <ChevronRight size={14} className="text-faint" aria-hidden="true" />
                        </button>
                      </Item>
                    )
                  })}
                </ItemGroup>
              </section>
            )}

            {selectedProvider && (() => {
              const runtime = runtimeForProvider(selectedProvider, snapshot.runtimes)
              const profiles = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id)
              const metadata = providerMetadata[selectedProvider]
              return (
                <section className="flex flex-col gap-4">
                  <div className="setting-heading">
                    <img className="provider-icon provider-icon-lg" src={metadata.src} alt="" />
                    <div><strong>{metadata.label}</strong>
                      <span>{runtime ? `${runtime.adapterId}${runtime.version ? ` · ${runtime.version}` : ''}` : t('agents.notInstalled')}</span>
                    </div>
                  </div>

                  {/*
                    One place to create, one place to edit. The profiles list is
                    the section's content, so the add action sits in its header
                    next to the thing it adds to.
                  */}
                  <div className="setting-heading section-action">
                    <div><strong>{t('settings.profilesHeading')}</strong></div>
                    {runtime && (
                      <Button type="button" variant="outline" size="sm" onClick={() => setDraft(newProfileFor(selectedProvider, runtime.id))}>
                        <Plus size={12} />{t('agents.create')}
                      </Button>
                    )}
                  </div>

                  {!runtime && <p className="settings-note">{t('agents.notInstalled')}</p>}
                  {runtime && !profiles.length && <p className="settings-note">{t('settings.noProfiles')}</p>}
                  {runtime && profiles.length > 0 && (
                    <div className="agent-list">
                      {profiles.map((profile) => (
                        <div className="agent-card" key={profile.id}>
                          <div className="agent-card-head">
                            <div><strong>{profile.name}</strong>
                              <span>{[profile.model, profile.reasoning].filter(Boolean).join(' · ') || t('agents.default')}</span>
                            </div>
                            <Button type="button" variant="outline" size="sm" onClick={() => setDraft(profile)}>{t('agents.edit')}</Button>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}

                  {draft && (
                    <form
                      className="agent-detail"
                      onSubmit={(event) => { event.preventDefault(); saveDraft() }}
                    >
                      <ProfileFields profile={draft} runtimes={snapshot.runtimes} t={t} onChange={setDraft} />
                      <div className="agent-detail-actions">
                        <Button type="button" variant="outline" size="sm" onClick={() => setDraft(undefined)}>{t('action.close')}</Button>
                        <Button size="sm">{t('action.save')}</Button>
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
                  <Select value={scope} onValueChange={setScope}>
                    <SelectTrigger className="max-w-[290px]"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="global">{t('settings.global')}</SelectItem>
                      {workspaces.map((workspace) => (
                        <SelectItem value={workspace} key={workspace}>{workspace}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>
                <div className="setting-row">
                  <span>{t('settings.maxRuns')}</span>
                  <PolicyNumberInput value={policy.maxConcurrentRuns} min={1} max={16}
                    onCommit={(value) => updatePolicy({ maxConcurrentRuns: value })} />
                </div>
                <div className="setting-row">
                  <span>{t('settings.maxWriters')}</span>
                  <PolicyNumberInput value={policy.maxConcurrentWriters} min={1} max={8}
                    onCommit={(value) => updatePolicy({ maxConcurrentWriters: value })} />
                </div>
                {([
                  ['requireWorktreeForParallelWriters', 'settings.requireWorktree'],
                  ['allowWrite', 'settings.allowWrite'],
                  ['allowCommands', 'settings.allowCommands'],
                  ['allowNetwork', 'settings.allowNetwork'],
                ] as const).map(([key, label]) => (
                  <div className="setting-row" key={key}>
  <span>{t(label)}</span>
  <Switch checked={policy[key]} onCheckedChange={(next) => updatePolicy({ [key]: next })} />
</div>
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
      </DialogContent>
    </Dialog>
  )
}
