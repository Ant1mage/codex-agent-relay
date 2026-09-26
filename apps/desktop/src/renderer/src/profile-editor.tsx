import { useState, type ReactNode } from 'react'
import { Button } from './components/ui/button.js'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from './components/ui/dialog.js'
import { Checkbox } from './components/ui/checkbox.js'
import { Field, FieldLabel, FieldLegend, FieldSet } from './components/ui/field.js'
import { Input } from './components/ui/input.js'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './components/ui/select.js'
import { Switch } from './components/ui/switch.js'
import { Textarea } from './components/ui/textarea.js'
import { X } from 'lucide-react'
import type { AgentProfile, Runtime } from '@relay/protocol'
import { useAppStore } from './store.js'
import { useRuntimeOptions, type Translator } from './ui.js'

/**
 * One agent field: a fixed label column so every control starts at the same X,
 * and a control column that shares one width. Previously each surface set its
 * own widths, which is why the fields did not line up.
 */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid min-h-[38px] grid-cols-[132px_minmax(0,1fr)] items-center gap-3 border-t border-line py-1.5 text-[11px] first:border-t-0">
      <span className="text-muted">{label}</span>
      <span className="flex min-w-0 items-center gap-2">{children}</span>
    </div>
  )
}

const capabilityRows = [
  ['readWorkspace', 'agents.read'],
  ['writeWorkspace', 'agents.write'],
  ['executeCommands', 'agents.shell'],
  ['networkAccess', 'agents.network'],
] as const

/**
 * The editable agent fields, shared by the modal and the Settings detail pane.
 *
 * Order follows docs/ui.md 16.1: the runtime first, then the model and reasoning
 * choices it reports, then permissions, with the description last.
 */
export function ProfileFields({
  profile,
  runtimes,
  t,
  onChange,
  hideRuntime = false,
}: {
  profile: AgentProfile
  runtimes: Runtime[]
  t: Translator
  onChange(next: AgentProfile): void
  /** Onboarding picks the runtime from the provider row, so it is not re-asked. */
  hideRuntime?: boolean
}) {
  const update = (next: Partial<AgentProfile>) => onChange({ ...profile, ...next })
  const capabilities = profile.capabilities
  // Model list and reasoning levels come from the CLI, never from Relay.
  const { options, loading } = useRuntimeOptions(profile.runtimeId)
  const models = options?.models ?? []
  const levels = options?.levels ?? []
  const unsupported = !loading && options !== undefined && !models.length && !levels.length

  const runtimeRow = (
    <Row label={t('cliInfo.runtime')}>
      <Select
        value={profile.runtimeId}
        onValueChange={(next) => update({ runtimeId: next, model: undefined, reasoning: undefined })}
      >
        <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
        <SelectContent>
          {runtimes.map((runtime) => (
            <SelectItem value={runtime.id} key={runtime.id}>
              {runtime.adapterId} · {runtime.version ?? runtime.executablePath}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Row>
  )

  const modelRow = (
    <Row label={t('agents.model')}>
      {models.length ? (
        <Select
          value={profile.model ?? ''}
          onValueChange={(next) => update({ model: next || undefined })}
        >
          <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
          <SelectContent>
            {/* Empty means "whatever the CLI defaults to", not a Relay choice. */}
            <SelectItem value="">{t('agents.runtimeDefault')}</SelectItem>
            {models.map((model) => (
              <SelectItem value={model.value} key={model.value}>{model.label ?? model.value}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <span className="field-note">
          {loading ? t('agents.readingRuntime') : t('agents.noModelList')}
        </span>
      )}
    </Row>
  )

  // Reasoning is a discrete CLI-defined token, so it uses the same select as the
  // model field rather than a slider (docs/ui.md 16.1). Options come from the CLI.
  const reasoningRow = (
    <Row label={t('agents.reasoning')}>
      {levels.length ? (
        <Select
          value={profile.reasoning ?? ''}
          onValueChange={(next) => update({ reasoning: next || undefined })}
        >
          <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="">{t('agents.runtimeDefault')}</SelectItem>
            {levels.map((level) => (
              <SelectItem value={level.value} key={level.value}>{level.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <span className="field-note">
          {loading ? t('agents.readingRuntime') : t('agents.noReasoningLevels')}
        </span>
      )}
    </Row>
  )

  // Say where the list came from: the CLI, or the provider's official API.
  const provenance = options?.source === 'api' ? (
    <p className="field-note">{t('agents.fromApi')}</p>
  ) : null
  const diagnostics = options?.diagnostics.length && options.source !== 'api' ? (
    <p className="field-note">{options.diagnostics.join(' ')}</p>
  ) : null

  const permissions = (
    <FieldSet className="mt-4">
      <FieldLegend className="mb-2 text-[11px] font-semibold">{t('agents.permissions')}</FieldLegend>
      <div className="grid grid-cols-2 gap-2">
        {capabilityRows.map(([key, label]) => (
          <Field key={key} orientation="horizontal">
            <Checkbox
              id={`cap-${key}`}
              checked={capabilities[key]}
              onCheckedChange={() => update({ capabilities: { ...capabilities, [key]: !capabilities[key] } })}
            />
            <FieldLabel htmlFor={`cap-${key}`} className="text-[11px] font-normal">{t(label)}</FieldLabel>
          </Field>
        ))}
      </div>
    </FieldSet>
  )

  return (
    <div className="flex flex-col">
      <Row label={t('agents.enabled')}>
        <Switch
          checked={profile.enabled}
          onCheckedChange={(next) => update({ enabled: next })}
        />
      </Row>
      <Row label={t('agents.name')}>
        <Input required value={profile.name} onChange={(event) => update({ name: event.target.value })} />
      </Row>
      {!hideRuntime && runtimeRow}
      {modelRow}
      {reasoningRow}
      {provenance}
      {unsupported && diagnostics}
      <Row label={t('agents.description')}>
        <Textarea value={profile.description} onChange={(event) => update({ description: event.target.value })} />
      </Row>
      {permissions}
    </div>
  )
}
