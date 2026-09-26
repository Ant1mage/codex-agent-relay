import { useEffect, useState } from 'react'
import type { AgentProfile, Runtime } from '@relay/protocol'
import type { RelayClient, RelayConfigView, RuntimeOptionsView } from '@relay/relay-api'
import { Plus, Trash2 } from 'lucide-react'
import { Badge } from '../components/ui/badge.js'
import { Button } from '../components/ui/button.js'
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card.js'
import { Input } from '../components/ui/input.js'
import { Label } from '../components/ui/label.js'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select.js'
import { Switch } from '../components/ui/switch.js'
import { Textarea } from '../components/ui/textarea.js'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '../components/ui/field.js'
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
    name: 'New agent',
    runtimeId,
    description: '',
    capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: false },
    enabled: true,
  }
}

/** Agent Profiles: the list Codex may delegate to, edited here and nowhere else. */
export function AgentsView({
  t,
  client,
  config,
  runtimes,
  onSave,
  onDelete,
}: {
  t: Translator
  client: RelayClient
  config: RelayConfigView
  runtimes: Runtime[]
  onSave(profile: AgentProfile): void
  onDelete(profileId: string): void
}) {
  const [draft, setDraft] = useState<AgentProfile>()
  const [options, setOptions] = useState<RuntimeOptionsView>()
  const runtimeName = (id: string) => runtimes.find((runtime) => runtime.id === id)?.adapterId ?? id

  // Model and reasoning are whatever the CLI advertises, never an invented list.
  useEffect(() => {
    if (!draft) return
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

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t('settings.profilesHeading')}
        </span>
        <span className="text-xs tabular-nums text-muted-foreground">{config.profiles.length}</span>
        <Button
          size="xs"
          variant="outline"
          className="ml-auto"
          disabled={runtimes.length === 0}
          onClick={() => setDraft(draftProfile(runtimes.find((runtime) => runtime.health === 'available')?.id ?? runtimes[0]?.id ?? ''))}
        >
          <Plus /> {t('panel.newAgent')}
        </Button>
      </div>

      {config.profiles.length === 0 && <p className="text-xs text-muted-foreground">{t('panel.agents.empty')}</p>}

      <div className="flex flex-col gap-1.5">
        {config.profiles.map((profile) => {
          const runtime = runtimes.find((candidate) => candidate.id === profile.runtimeId)
          const available = runtime?.health === 'available'
          return (
            <button
              key={profile.id}
              type="button"
              onClick={() => setDraft({ ...profile })}
              className="flex w-full items-center gap-2 rounded-md border border-border px-2.5 py-2 text-left hover:bg-accent/50"
            >
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2 text-[13px] font-medium">
                  {profile.name}
                  {!available && (
                    <Badge variant="destructive" className="text-[10px]">
                      {runtime?.health === 'authentication_required' ? t('agents.authRequired') : t('agents.notInstalled')}
                    </Badge>
                  )}
                </span>
                <span className="block truncate text-[11px] text-muted-foreground">
                  {runtimeName(profile.runtimeId)}
                  {profile.model ? ` · ${profile.model}` : ''}
                  {profile.reasoning ? ` · ${profile.reasoning}` : ''}
                </span>
              </span>
              <Switch
                checked={profile.enabled}
                onClick={(event) => event.stopPropagation()}
                onCheckedChange={(checked) => onSave({ ...profile, enabled: checked })}
              />
            </button>
          )
        })}
      </div>

      {draft && (
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">{t('agents.edit')}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <FieldGroup className="gap-3">
              <Field>
                <FieldLabel htmlFor="agent-name">{t('agents.name')}</FieldLabel>
                <Input id="agent-name" value={draft.name} onChange={(event) => patch({ name: event.target.value })} />
              </Field>
              <Field>
                <FieldLabel htmlFor="agent-description">{t('agents.description')}</FieldLabel>
                <Input
                  id="agent-description"
                  value={draft.description}
                  onChange={(event) => patch({ description: event.target.value })}
                />
              </Field>
              <Field>
                <FieldLabel>{t('cliInfo.runtime')}</FieldLabel>
                <Select value={draft.runtimeId} onValueChange={(value) => patch({ runtimeId: value })}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {runtimes.map((runtime) => (
                      <SelectItem key={runtime.id} value={runtime.id}>
                        {runtime.adapterId}
                        {runtime.health === 'available' ? '' : ` (${runtime.health})`}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
              <Field>
                <FieldLabel>{t('agents.model')}</FieldLabel>
                <Select
                  value={draft.model ?? '__default__'}
                  onValueChange={(value) => patch({ model: value === '__default__' ? undefined : value })}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__default__">{t('panel.modelAuto')}</SelectItem>
                    {(options?.models ?? []).map((model) => (
                      <SelectItem key={model.value} value={model.value}>
                        {model.label ?? model.value}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <FieldDescription>
                  {options
                    ? options.models.length > 0
                      ? t('agents.modelHint')
                      : t('agents.noModelList')
                    : t('agents.readingRuntime')}
                </FieldDescription>
              </Field>
              <Field>
                <FieldLabel>{t('agents.reasoning')}</FieldLabel>
                <Select
                  value={draft.reasoning ?? '__default__'}
                  onValueChange={(value) => patch({ reasoning: value === '__default__' ? undefined : value })}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__default__">{t('agents.runtimeDefault')}</SelectItem>
                    {(options?.levels ?? []).map((level) => (
                      <SelectItem key={level.value} value={level.value}>
                        {level.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {options && options.levels.length === 0 && (
                  <FieldDescription>{t('agents.noReasoningLevels')}</FieldDescription>
                )}
              </Field>
              <Field>
                <FieldLabel htmlFor="agent-instructions">{t('panel.instructions')}</FieldLabel>
                <Textarea
                  id="agent-instructions"
                  rows={3}
                  value={draft.instructions ?? ''}
                  onChange={(event) => patch({ instructions: event.target.value || undefined })}
                />
              </Field>
              <Field>
                <FieldLabel>{t('agents.permissions')}</FieldLabel>
                <div className="flex flex-col gap-2">
                  {CAPABILITIES.map((capability) => (
                    <div key={capability.key} className="flex items-center justify-between">
                      <Label className="text-xs font-normal">{t(capability.label)}</Label>
                      <Switch
                        checked={draft.capabilities[capability.key]}
                        onCheckedChange={(checked) =>
                          patch({ capabilities: { ...draft.capabilities, [capability.key]: checked } })
                        }
                      />
                    </div>
                  ))}
                </div>
              </Field>
              <div className="flex items-center justify-between">
                <Label className="text-xs font-normal">{t('agents.enabled')}</Label>
                <Switch checked={draft.enabled} onCheckedChange={(checked) => patch({ enabled: checked })} />
              </div>
            </FieldGroup>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                disabled={!draft.name.trim() || !draft.runtimeId}
                onClick={() => {
                  onSave({ ...draft, description: draft.description || draft.name })
                  setDraft(undefined)
                }}
              >
                {t('action.save')}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setDraft(undefined)}>
                {t('action.cancel')}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="ml-auto text-destructive"
                onClick={() => {
                  if (!window.confirm(t('panel.deleteConfirm'))) return
                  onDelete(draft.id)
                  setDraft(undefined)
                }}
              >
                <Trash2 /> {t('panel.delete')}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
