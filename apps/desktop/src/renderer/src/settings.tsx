import { useEffect, useRef, useState, type ReactElement } from 'react'
import {
  Bot,
  ChevronRight,
  Plus,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
} from 'lucide-react'
import { Button } from './components/ui/button.js'
import { SettingsFrame } from './components/settings-frame.js'
import { Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from './components/ui/card.js'
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldSeparator } from './components/ui/field.js'
import { Empty, EmptyHeader, EmptyTitle } from './components/ui/empty.js'
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from './components/ui/item.js'
import { Input } from './components/ui/input.js'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select.js'
import { ToggleGroup, ToggleGroupItem } from './components/ui/toggle-group.js'
import { Switch } from './components/ui/switch.js'
import type { AgentProfile, Locale, RelayPolicy } from '@relay/protocol'
import type { DesktopSettings } from '../../shared/api.js'
import { useAppStore } from './store.js'
import { ProfileFields } from './profile-editor.js'
import { cn } from './lib/utils.js'
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
  const { snapshot, settingsOpen, settingsSection, setSettingsOpen, locale, setLocale, setNotice, refresh } =
    useAppStore()
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

  // The menu bar names a page, never a control: Settings still owns its layout
  // and the user keeps navigating inside it from there.
  useEffect(() => {
    if (settingsOpen && settingsSection) setSelection(settingsSection)
  }, [settingsOpen, settingsSection])

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

  const navButton = (id: Selection, label: string, icon: ReactElement) => (
    <Item
      asChild
      key={id}
      size="sm"
      className={cn('rounded-md', selection === id ? 'bg-accent text-accent-foreground' : 'text-muted-foreground')}
    >
      <button
        type="button"
        className="w-full text-left"
        onClick={() => { setSelection(id); setDraft(undefined) }}
      >
        <ItemMedia>{icon}</ItemMedia>
        <ItemContent>
          <ItemTitle>{label}</ItemTitle>
        </ItemContent>
      </button>
    </Item>
  )

  return (
    <SettingsFrame
      open
      onOpenChange={(next) => { if (!next) setSettingsOpen(false) }}
      title={t('settings.title')}
      description={t('settings.subtitle')}
      closeLabel={t('action.close')}
      navigation={(
        <ItemGroup className="gap-0.5">
            {navButton('general', t('settings.general'), <Settings2 size={14} />)}
            {navButton('agents', t('settings.providerAgents'), <Bot size={14} />)}
            {providerOrder.map((provider) => {
              const runtime = runtimeForProvider(provider, snapshot.runtimes)
              const count = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id).length
              const active = selection === `agent:${provider}`
              return (
                <Item
                  asChild
                  key={provider}
                  className={cn('ml-6 rounded-md', active ? 'bg-accent text-accent-foreground' : 'text-muted-foreground')}
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
                      <ItemTitle>{providerMetadata[provider].label}</ItemTitle>
                    </ItemContent>
                    {count > 0 && <span className="text-xs text-muted-foreground">{count}</span>}
                  </button>
                </Item>
              )
            })}

            {navButton('workspace', t('settings.workspaceSection'), <ShieldCheck size={14} />)}
            {navButton('advanced', t('settings.advanced'), <SlidersHorizontal size={14} />)}
        </ItemGroup>
      )}
    >
      <div className="p-5">
            {selection === 'general' && (
              <div className="flex flex-col gap-4">
                <Card>
                  <CardHeader>
                    <CardTitle>{t('settings.language')}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <FieldGroup className="gap-4">
                      <Field orientation="horizontal">
                        <FieldLabel className="sr-only">{t('settings.language')}</FieldLabel>
                        <ToggleGroup
                          type="single"
                          value={locale}
                          onValueChange={(next) => { if (next) setLocale(next as Locale) }}
                          variant="outline"
                          size="sm"
                          aria-label={t('settings.language')}
                        >
                          <ToggleGroupItem value="en">English</ToggleGroupItem>
                          <ToggleGroupItem value="zh-CN">简体中文</ToggleGroupItem>
                        </ToggleGroup>
                      </Field>
                    </FieldGroup>
                  </CardContent>
                </Card>

                <Card>
                  <CardHeader>
                    <CardTitle>{t('settings.appearance')}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <FieldGroup className="gap-4">
                      <Field orientation="horizontal">
                        <FieldLabel className="sr-only">{t('settings.appearance')}</FieldLabel>
                        <ToggleGroup
                          type="single"
                          value={theme}
                          onValueChange={(next) => { if (next) setTheme(next as Theme) }}
                          variant="outline"
                          size="sm"
                          aria-label={t('settings.appearance')}
                        >
                          {(['system', 'light', 'dark'] as Theme[]).map((value) => (
                            <ToggleGroupItem value={value} key={value}>{t(`settings.${value}`)}</ToggleGroupItem>
                          ))}
                        </ToggleGroup>
                      </Field>
                      <FieldSeparator />
                      <Field orientation="horizontal">
                      <FieldLabel htmlFor="relay-font-size">{t('settings.fontSize')}</FieldLabel>
                      <Select value={String(fontSize)} onValueChange={(next) => setFontSize(Number(next))}>
                        <SelectTrigger id="relay-font-size" className="w-[96px]"><SelectValue /></SelectTrigger>
                        <SelectContent>
                          {Array.from({ length: MAX_FONT_SIZE - MIN_FONT_SIZE + 1 }, (_, index) => MIN_FONT_SIZE + index).map((value) => (
                            <SelectItem value={String(value)} key={value}>{value} px</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      </Field>
                    </FieldGroup>
                  </CardContent>
                </Card>
              </div>
            )}

            {selection === 'agents' && (
              <Card>
                <CardHeader>
                  <CardTitle>{t('settings.providerAgents')}</CardTitle>
                  <CardDescription>{t('agents.subtitle')}</CardDescription>
                </CardHeader>
                <CardContent>
                  <ItemGroup>
                  {providerOrder.map((provider) => {
                    const runtime = runtimeForProvider(provider, snapshot.runtimes)
                    const count = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id).length
                    return (
                      <Item asChild key={provider} variant="outline" size="sm">
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
                            <ItemDescription>{runtime ? t('agents.detected') : t('agents.notInstalled')}</ItemDescription>
                          </ItemContent>
                          <ItemActions>
                            {count > 0 && <span className="text-xs text-muted-foreground">{count} {t('settings.profilesSuffix')}</span>}
                            <ChevronRight aria-hidden="true" />
                          </ItemActions>
                        </button>
                      </Item>
                    )
                  })}
                  </ItemGroup>
                </CardContent>
              </Card>
            )}

            {selectedProvider && (() => {
              const runtime = runtimeForProvider(selectedProvider, snapshot.runtimes)
              const profiles = snapshot.profiles.filter((profile) => profile.runtimeId === runtime?.id)
              const metadata = providerMetadata[selectedProvider]
              return (
                <div className="flex flex-col gap-4">
                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2">
                        <img className="size-5" src={metadata.src} alt="" />
                        {metadata.label}
                      </CardTitle>
                      <CardDescription>{runtime ? `${runtime.adapterId}${runtime.version ? ` · ${runtime.version}` : ''}` : t('agents.notInstalled')}</CardDescription>
                      {runtime && (
                        <CardAction>
                          <Button type="button" variant="outline" size="sm" onClick={() => setDraft(newProfileFor(selectedProvider, runtime.id))}>
                            <Plus data-icon="inline-start" />{t('agents.create')}
                          </Button>
                        </CardAction>
                      )}
                    </CardHeader>
                    <CardContent>
                      <FieldGroup className="gap-4">
                        <Field>
                          <FieldLabel>{t('settings.profilesHeading')}</FieldLabel>
                          {!runtime && <FieldDescription>{t('agents.notInstalled')}</FieldDescription>}
                          {runtime && !profiles.length && <FieldDescription>{t('settings.noProfiles')}</FieldDescription>}
                          {runtime && profiles.length > 0 && (
                            <ItemGroup>
                              {profiles.map((profile) => (
                                <Item key={profile.id} variant="outline" size="sm">
                                  <ItemContent>
                                    <ItemTitle>{profile.name}</ItemTitle>
                                    <ItemDescription>{[profile.model, profile.reasoning].filter(Boolean).join(' · ') || t('agents.default')}</ItemDescription>
                                  </ItemContent>
                                  <ItemActions>
                                    <Button type="button" variant="outline" size="sm" onClick={() => setDraft(profile)}>{t('agents.edit')}</Button>
                                  </ItemActions>
                                </Item>
                              ))}
                            </ItemGroup>
                          )}
                        </Field>
                      </FieldGroup>
                    </CardContent>
                  </Card>

                  {draft && (
                    <Card>
                      <CardHeader>
                        <CardTitle>{draft.name || t('agents.create')}</CardTitle>
                      </CardHeader>
                      <form onSubmit={(event) => { event.preventDefault(); saveDraft() }}>
                        <CardContent>
                          <ProfileFields profile={draft} runtimes={snapshot.runtimes} t={t} onChange={setDraft} />
                        </CardContent>
                        <CardFooter className="justify-end gap-2">
                          <Button type="button" variant="outline" size="sm" onClick={() => setDraft(undefined)}>{t('action.close')}</Button>
                          <Button size="sm">{t('action.save')}</Button>
                        </CardFooter>
                      </form>
                    </Card>
                  )}
                </div>
              )
            })()}

            {selection === 'workspace' && (
              <Card>
                <CardHeader>
                  <CardTitle>{t('settings.policy')}</CardTitle>
                  <CardDescription>{scope === 'global' ? t('settings.global') : t('settings.workspace')}</CardDescription>
                </CardHeader>
                <CardContent>
                  <FieldGroup>
                    <Field orientation="horizontal">
                      <FieldLabel htmlFor="relay-policy-scope">{t('settings.scope')}</FieldLabel>
                      <Select value={scope} onValueChange={setScope}>
                        <SelectTrigger id="relay-policy-scope" className="max-w-[290px]"><SelectValue /></SelectTrigger>
                        <SelectContent>
                          <SelectItem value="global">{t('settings.global')}</SelectItem>
                          {workspaces.map((workspace) => (
                            <SelectItem value={workspace} key={workspace}>{workspace}</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </Field>
                    <FieldSeparator />
                    <Field orientation="horizontal">
                      <FieldLabel htmlFor="relay-max-runs">{t('settings.maxRuns')}</FieldLabel>
                      <PolicyNumberInput value={policy.maxConcurrentRuns} min={1} max={16}
                        onCommit={(value) => updatePolicy({ maxConcurrentRuns: value })} />
                    </Field>
                    <Field orientation="horizontal">
                      <FieldLabel htmlFor="relay-max-writers">{t('settings.maxWriters')}</FieldLabel>
                      <PolicyNumberInput value={policy.maxConcurrentWriters} min={1} max={8}
                        onCommit={(value) => updatePolicy({ maxConcurrentWriters: value })} />
                    </Field>
                    {([
                      ['requireWorktreeForParallelWriters', 'settings.requireWorktree'],
                      ['allowWrite', 'settings.allowWrite'],
                      ['allowCommands', 'settings.allowCommands'],
                      ['allowNetwork', 'settings.allowNetwork'],
                    ] as const).map(([key, label]) => (
                      <Field orientation="horizontal" key={key}>
                        <FieldLabel>{t(label)}</FieldLabel>
                        <Switch checked={policy[key]} onCheckedChange={(next) => updatePolicy({ [key]: next })} />
                      </Field>
                    ))}
                  </FieldGroup>
                </CardContent>
              </Card>
            )}

            {selection === 'advanced' && (
              <Card>
                <CardHeader>
                  <CardTitle>{t('settings.advanced')}</CardTitle>
                  <CardDescription>{t('settings.advancedHint')}</CardDescription>
                </CardHeader>
                <CardContent>
                  {!snapshot.diagnostics.length ? (
                    <Empty className="border-0 p-0">
                      <EmptyHeader>
                        <EmptyTitle>{t('settings.diagnosticsEmpty')}</EmptyTitle>
                      </EmptyHeader>
                    </Empty>
                  ) : (
                    <ItemGroup>
                      {snapshot.diagnostics.map((diagnostic, index) => (
                        <Item key={index} variant="muted" size="sm">
                          <ItemContent><ItemDescription>{diagnostic}</ItemDescription></ItemContent>
                        </Item>
                      ))}
                    </ItemGroup>
                  )}
                </CardContent>
              </Card>
            )}
      </div>
    </SettingsFrame>
  )
}
