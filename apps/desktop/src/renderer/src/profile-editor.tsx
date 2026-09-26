import { useState } from 'react'
import { X } from 'lucide-react'
import type { AgentProfile, Runtime } from '@relay/protocol'
import { useAppStore } from './store.js'
import { ReasoningSlider, useRuntimeOptions, type Translator } from './ui.js'

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
  layout = 'stacked',
}: {
  profile: AgentProfile
  runtimes: Runtime[]
  t: Translator
  onChange(next: AgentProfile): void
  layout?: 'stacked' | 'rows'
}) {
  const update = (next: Partial<AgentProfile>) => onChange({ ...profile, ...next })
  const capabilities = profile.capabilities
  // Model list and reasoning levels come from the CLI, never from Relay.
  const { options, loading } = useRuntimeOptions(profile.runtimeId)
  const models = options?.models ?? []
  const levels = options?.levels ?? []
  const unsupported = !loading && options !== undefined && !models.length && !levels.length

  const runtimeRow = (
    <label className={layout === 'rows' ? 'setting-row' : undefined}>
      <span>{t('cliInfo.runtime')}</span>
      <select value={profile.runtimeId} onChange={(event) => update({ runtimeId: event.target.value, model: undefined, reasoning: undefined })}>
        {runtimes.map((runtime) => (
          <option value={runtime.id} key={runtime.id}>
            {runtime.adapterId} · {runtime.version ?? runtime.executablePath}
          </option>
        ))}
      </select>
    </label>
  )

  const modelRow = (
    <label className={layout === 'rows' ? 'setting-row' : undefined}>
      <span>{t('agents.model')}</span>
      {models.length ? (
        <select
          value={profile.model ?? ''}
          onChange={(event) => update({ model: event.target.value || undefined })}
        >
          {/* Empty means "whatever the CLI defaults to", not a Relay choice. */}
          <option value="">{t('agents.runtimeDefault')}</option>
          {models.map((model) => (
            <option value={model.value} key={model.value}>{model.label ?? model.value}</option>
          ))}
        </select>
      ) : (
        <span className="field-note">
          {loading ? t('agents.readingRuntime') : t('agents.noModelList')}
        </span>
      )}
    </label>
  )

  const reasoningRow = (
    <div className={layout === 'rows' ? 'setting-row setting-row-slider' : 'field-slider'}>
      <span>{t('agents.reasoning')}</span>
      {levels.length ? (
        <ReasoningSlider
          levels={levels}
          value={profile.reasoning}
          onChange={(value) => update({ reasoning: value })}
          t={t}
        />
      ) : (
        <span className="field-note">
          {loading ? t('agents.readingRuntime') : t('agents.noReasoningLevels')}
        </span>
      )}
    </div>
  )

  const permissions = (
    <>
      <div className="setting-heading"><strong>{t('agents.permissions')}</strong></div>
      <div className="capability-grid">
        {capabilityRows.map(([key, label]) => (
          <label className="check-row" key={key}>
            <input
              type="checkbox"
              checked={capabilities[key]}
              onChange={() => update({ capabilities: { ...capabilities, [key]: !capabilities[key] } })}
            />
            <span>{t(label)}</span>
          </label>
        ))}
      </div>
    </>
  )

  const description = (
    <label className={layout === 'rows' ? 'setting-row setting-row-block' : undefined}>
      <span>{t('agents.description')}</span>
      <textarea value={profile.description} onChange={(event) => update({ description: event.target.value })} />
    </label>
  )

  // Say where the list came from: the CLI, or the provider's official API.
  const provenance = options?.source === 'api' ? (
    <p className="field-note">{t('agents.fromApi')}</p>
  ) : null
  const diagnostics = options?.diagnostics.length && options.source !== 'api' ? (
    <p className="field-note">{options.diagnostics.join(' ')}</p>
  ) : null

  if (layout === 'rows') {
    return (
      <>
        <label className="setting-row">
          <span>{t('agents.enabled')}</span>
          <input type="checkbox" checked={profile.enabled} onChange={() => update({ enabled: !profile.enabled })} />
        </label>
        <label className="setting-row">
          <span>{t('agents.name')}</span>
          <input required value={profile.name} onChange={(event) => update({ name: event.target.value })} />
        </label>
        {runtimeRow}
        {modelRow}
        {reasoningRow}
        {provenance}
        {unsupported && diagnostics}
        {permissions}
        {description}
      </>
    )
  }

  return (
    <>
      <label>
        <span>{t('agents.name')}</span>
        <input required value={profile.name} onChange={(event) => update({ name: event.target.value })} />
      </label>
      {runtimeRow}
      {modelRow}
      {reasoningRow}
      {provenance}
      {unsupported && diagnostics}
      <label className="check-row">
        <input type="checkbox" checked={profile.enabled} onChange={() => update({ enabled: !profile.enabled })} />
        <span>{t('agents.enabled')}</span>
      </label>
      {permissions}
      {description}
    </>
  )
}

/**
 * Agent editor modal. Used from Settings and from the onboarding "Add Agents"
 * step, which keeps the form minimal and prefilled (docs/ui.md 22.2).
 */
export function ProfileEditor({
  profile,
  runtimes,
  t,
  close,
}: {
  profile: AgentProfile
  runtimes: Runtime[]
  t: Translator
  close(): void
}) {
  const { refresh, setNotice } = useAppStore()
  const [draft, setDraft] = useState(profile)
  return (
    <div className="modal-backdrop">
      <form
        className="profile-editor"
        onSubmit={(event) => {
          event.preventDefault()
          void window.relay
            .saveProfile(draft)
            .then(async () => {
              await refresh()
              setNotice(t('agents.profileSaved'))
              close()
            })
            .catch((error: unknown) => setNotice(error instanceof Error ? error.message : String(error)))
        }}
      >
        <header>
          <h2>{t('agents.edit')}</h2>
          <button type="button" className="plain-icon" onClick={close}><X size={16} /></button>
        </header>
        <ProfileFields profile={draft} runtimes={runtimes} t={t} onChange={setDraft} />
        <footer>
          <button type="button" className="secondary" onClick={close}>{t('action.close')}</button>
          <button className="primary">{t('action.save')}</button>
        </footer>
      </form>
    </div>
  )
}
