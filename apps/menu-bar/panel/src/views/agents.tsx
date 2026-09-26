import { useEffect, useState } from 'react'
import type { AgentProfile, Runtime } from '@relay/protocol'
import type { RelayClient, RelayConfigView, RuntimeOptionsView } from '@relay/relay-api'
import { ArrowLeft, Plus, Trash2 } from 'lucide-react'
import { ConfirmDialog } from '../components/confirm-dialog.js'
import { Badge } from '../components/ui/badge.js'
import { Button } from '../components/ui/button.js'
import { Empty, EmptyDescription, EmptyHeader } from '../components/ui/empty.js'
import { Input } from '../components/ui/input.js'
import { Item, ItemActions, ItemContent, ItemGroup } from '../components/ui/item.js'
import { Label } from '../components/ui/label.js'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select.js'
import { Switch } from '../components/ui/switch.js'
import { Textarea } from '../components/ui/textarea.js'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '../components/ui/field.js'
import { cn } from '../lib/utils.js'
import type { Translator } from '../lib/i18n.js'

const CAPABILITIES = [
  { key: 'readWorkspace', label: 'agents.read' },
  { key: 'writeWorkspace', label: 'agents.write' },
  { key: 'executeCommands', label: 'agents.shell' },
  { key: 'networkAccess', label: 'agents.network' },
] as const

function draftProfile(runtimeId: string): AgentProfile {
  return {
    id: `agent-${Date.now()}`,
    name: '',
    runtimeId,
    description: '',
    capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: false },
    enabled: true,
  }
}

/**
 * Agent Profiles. The list and the editor are two views rather than one long
 * page: in a 420px popover an editor appended under the list lands below the
 * fold and reads as "the button did nothing".
 */
export function AgentsView({
  t,
  client,
  config,
  runtimes,
  startNew,
  editProfileId,
  onIntentHandled,
  onSave,
  onDelete,
}: {
  t: Translator
  client: RelayClient
  config: RelayConfigView
  runtimes: Runtime[]
  startNew: boolean
  /** Set when the tray asked for a specific profile's editor. */
  editProfileId?: string | undefined
  onIntentHandled(): void
  onSave(profile: AgentProfile): void
  onDelete(profileId: string): void
}) {
  const [draft, setDraft] = useState<AgentProfile>()
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [options, setOptions] = useState<RuntimeOptionsView>()
  const runtimeName = (id: string) => runtimes.find((runtime) => runtime.id === id)?.adapterId ?? id

  const beginNew = () => {
    const preferred = runtimes.find((runtime) => runtime.health === 'available') ?? runtimes[0]
    setDraft(draftProfile(preferred?.id ?? ''))
  }

  useEffect(() => {
    if (startNew) {
      beginNew()
      onIntentHandled()
    }
  }, [startNew])

  useEffect(() => {
    if (!editProfileId) return
    const profile = config.profiles.find((candidate) => candidate.id === editProfileId)
    if (profile) setDraft({ ...profile })
    onIntentHandled()
  }, [editProfileId])

  // Model and reasoning are whatever the CLI advertises, never an invented list.
  useEffect(() => {
    if (!draft?.runtimeId) {
      setOptions(undefined)
      return
    }
    let cancelled = false
    setOptions(undefined)
    void client
      .runtimeOptions(draft.runtimeId)
      .then((value) => {
        if (!cancelled) setOptions(value)
      })
      .catch(() => {
        if (!cancelled) setOptions(undefined)
      })
    return () => {
      cancelled = true
    }
  }, [client, draft?.runtimeId])

  const patch = (change: Partial<AgentProfile>) => setDraft((current) => (current ? { ...current, ...change } : current))

  if (draft) {
    const persisted = config.profiles.some((profile) => profile.id === draft.id)
    return (
      <div className="flex min-w-0 flex-col gap-3">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon-xs" onClick={() => setDraft(undefined)} title={t('action.cancel')}>
            <ArrowLeft />
          </Button>
          <span className="min-w-0 flex-1 truncate text-xs font-semibold">
            {persisted ? t('agents.edit') : t('panel.newAgent')}
          </span>
        </div>

        <FieldGroup className="min-w-0 gap-3">
          <Field className="min-w-0">
            <FieldLabel htmlFor="agent-name">{t('agents.name')}</FieldLabel>
            <Input
              id="agent-name"
              value={draft.name}
              placeholder={t('panel.newAgent')}
              onChange={(event) => patch({ name: event.target.value })}
            />
          </Field>
          <Field className="min-w-0">
            <FieldLabel htmlFor="agent-description">{t('agents.description')}</FieldLabel>
            <Input
              id="agent-description"
              className="min-w-0"
              value={draft.description}
              onChange={(event) => patch({ description: event.target.value })}
            />
          </Field>
          <Field className="min-w-0">
            <FieldLabel>{t('cliInfo.runtime')}</FieldLabel>
            <Select value={draft.runtimeId} onValueChange={(value) => patch({ runtimeId: value })}>
              <SelectTrigger className="w-full min-w-0">
                <SelectValue placeholder={t('panel.noRuntimes')} />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {runtimes.map((runtime) => (
                    <SelectItem key={runtime.id} value={runtime.id}>
                      {runtime.adapterId}
                      {runtime.health === 'available' ? '' : ` (${runtime.health})`}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          <Field className="min-w-0">
            <FieldLabel>{t('agents.model')}</FieldLabel>
            <Select
              value={draft.model ?? '__default__'}
              onValueChange={(value) => patch({ model: value === '__default__' ? undefined : value })}
            >
              <SelectTrigger className="w-full min-w-0">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="__default__">{t('panel.modelAuto')}</SelectItem>
                  {(options?.models ?? []).map((model) => (
                    <SelectItem key={model.value} value={model.value}>
                      {model.label ?? model.value}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
            <FieldDescription className="break-words">
              {options
                ? options.models.length > 0
                  ? t('agents.modelHint')
                  : t('agents.noModelList')
                : t('agents.readingRuntime')}
            </FieldDescription>
          </Field>
          <Field className="min-w-0">
            <FieldLabel>{t('agents.reasoning')}</FieldLabel>
            <Select
              value={draft.reasoning ?? '__default__'}
              onValueChange={(value) => patch({ reasoning: value === '__default__' ? undefined : value })}
            >
              <SelectTrigger className="w-full min-w-0">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="__default__">{t('agents.runtimeDefault')}</SelectItem>
                  {(options?.levels ?? []).map((level) => (
                    <SelectItem key={level.value} value={level.value}>
                      {level.label}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
            {options && options.levels.length === 0 && (
              <FieldDescription className="break-words">{t('agents.noReasoningLevels')}</FieldDescription>
            )}
          </Field>
          <Field className="min-w-0">
            <FieldLabel htmlFor="agent-instructions">{t('panel.instructions')}</FieldLabel>
            <Textarea
              id="agent-instructions"
              rows={3}
              className="min-w-0"
              value={draft.instructions ?? ''}
              onChange={(event) => patch({ instructions: event.target.value || undefined })}
            />
          </Field>
          <Field className="min-w-0">
            <FieldLabel>{t('agents.permissions')}</FieldLabel>
            <div className="flex flex-col gap-2">
              {CAPABILITIES.map((capability) => (
                <div key={capability.key} className="flex items-center justify-between gap-3">
                  <Label className="min-w-0 truncate text-xs font-normal">{t(capability.label)}</Label>
                  <Switch
                    className="shrink-0"
                    checked={draft.capabilities[capability.key]}
                    onCheckedChange={(checked: boolean) =>
                      patch({ capabilities: { ...draft.capabilities, [capability.key]: checked } })
                    }
                  />
                </div>
              ))}
            </div>
          </Field>
          <div className="flex items-center justify-between gap-3">
            <Label className="min-w-0 truncate text-xs font-normal">{t('agents.enabled')}</Label>
            <Switch
              className="shrink-0"
              checked={draft.enabled}
              onCheckedChange={(checked: boolean) => patch({ enabled: checked })}
            />
          </div>
        </FieldGroup>

        <div className="flex items-center gap-2">
          <Button
            size="sm"
            className="flex-1"
            disabled={!draft.name.trim() || !draft.runtimeId}
            onClick={() => {
              onSave({ ...draft, description: draft.description || draft.name })
              setDraft(undefined)
            }}
          >
            {t('action.save')}
          </Button>
          <Button size="sm" variant="outline" onClick={() => setDraft(undefined)}>
            {t('action.cancel')}
          </Button>
          {persisted && (
            <Button
              size="sm"
              variant="ghost"
              className="shrink-0 text-destructive hover:text-destructive"
              title={t('panel.delete')}
              onClick={() => setDeleteOpen(true)}
            >
              <Trash2 />
            </Button>
          )}
        </div>
        <ConfirmDialog
          open={deleteOpen}
          onOpenChange={setDeleteOpen}
          title={t('panel.agent.deleteTitle')}
          description={t('panel.deleteConfirm')}
          cancelLabel={t('action.cancel')}
          confirmLabel={t('panel.delete')}
          onConfirm={() => {
            onDelete(draft.id)
            setDraft(undefined)
          }}
        />
      </div>
    )
  }

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex min-w-0 items-center gap-2">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-xs font-semibold">{t('nav.agents')}</h2>
          <p className="text-[10px] tabular-nums text-muted-foreground">
            {config.profiles.length} {t('panel.agent.profiles')}
          </p>
        </div>
        <Button size="xs" disabled={runtimes.length === 0} onClick={beginNew}>
          <Plus /> {t('panel.newAgent')}
        </Button>
      </div>

      {config.profiles.length === 0 && (
        <Empty className="gap-2 border py-8">
          <EmptyHeader>
            <EmptyDescription>{t('panel.agents.empty')}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      )}

      <ItemGroup className="gap-1.5">
        {config.profiles.map((profile) => {
          const runtime = runtimes.find((candidate) => candidate.id === profile.runtimeId)
          const available = runtime?.health === 'available'
          return (
            <Item key={profile.id} variant="outline" size="sm" className="min-w-0 flex-nowrap px-3 py-2.5">
              <ItemContent className="min-w-0">
                <Button
                  variant="ghost"
                  className="h-auto w-full min-w-0 justify-start whitespace-normal p-0 text-left hover:bg-transparent"
                  onClick={() => setDraft({ ...profile })}
                >
                  <span className="flex min-w-0 flex-1 flex-col items-start gap-0.5">
                    <span className="flex min-w-0 max-w-full items-center gap-1.5">
                      <span className="min-w-0 truncate text-[13px] font-medium">{profile.name}</span>
                      {!available && (
                        <Badge variant="destructive" className="shrink-0 text-[10px]">
                          {runtime?.health === 'authentication_required' ? t('agents.authRequired') : t('agents.notInstalled')}
                        </Badge>
                      )}
                    </span>
                    <span className="block max-w-full truncate text-[11px] font-normal text-muted-foreground">
                      {runtimeName(profile.runtimeId)}
                      {profile.model ? ` · ${profile.model}` : ''}
                      {profile.reasoning ? ` · ${profile.reasoning}` : ''}
                    </span>
                  </span>
                </Button>
              </ItemContent>
              <ItemActions className="shrink-0">
                <Switch
                  className="shrink-0"
                  aria-label={`${profile.name} · ${t('agents.enabled')}`}
                  checked={profile.enabled}
                  onCheckedChange={(checked: boolean) => onSave({ ...profile, enabled: checked })}
                />
              </ItemActions>
            </Item>
          )
        })}
      </ItemGroup>
    </div>
  )
}
