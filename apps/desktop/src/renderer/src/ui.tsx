import { useEffect, useState, type ReactNode } from 'react'
import type {
  AgentProfile,
  RelayEvent,
  ReasoningLevel,
  RunStatus,
  Runtime,
  RuntimeOptions,
  StepStatus,
} from '@relay/protocol'
import { createTranslator, type TranslationKey } from '@relay/i18n'

export type Translator = (key: TranslationKey) => string
export type Inspector = 'changes' | 'raw' | undefined
export type Theme = 'system' | 'light' | 'dark'

export const DEFAULT_FONT_SIZE = 14
export const MAX_FONT_SIZE = 20

export type ProviderIconId = 'deepseek' | 'gemini' | 'glm' | 'grok' | 'kimi'

/**
 * Provider marks come straight from assets/providers (served at /providers).
 * Provider branding is a separate asset domain from the Relay app icon, and each
 * logo is chosen for being recognisable at the 14-15px size these are drawn at.
 */
export const providerMetadata: Record<ProviderIconId, { label: string; src: string }> = {
  deepseek: { label: 'DeepSeek', src: '/providers/deepseek.svg' },
  gemini: { label: 'Gemini', src: '/providers/gemini.svg' },
  glm: { label: 'GLM', src: '/providers/glm.svg' },
  grok: { label: 'Grok', src: '/providers/grok.svg' },
  kimi: { label: 'Kimi', src: '/providers/kimi.svg' },
}

/** The provider order used by onboarding and Settings (docs/ui.md 17.2). */
export const providerOrder: ProviderIconId[] = ['deepseek', 'glm', 'gemini', 'grok', 'kimi']

export function providerIconId(
  name: string | undefined,
  adapterId: string | undefined,
): ProviderIconId | undefined {
  const value = `${name ?? ''} ${adapterId ?? ''}`.toLowerCase()
  if (value.includes('deepseek')) return 'deepseek'
  if (value.includes('glm') || value.includes('z.ai') || value.includes('zhipu') || value.includes('zai')) return 'glm'
  if (value.includes('gemini') || value.includes('antigravity')) return 'gemini'
  if (value.includes('grok') || value.includes('x.ai')) return 'grok'
  if (value.includes('kimi') || value.includes('moonshot')) return 'kimi'
  return undefined
}

/** Matches a provider to a detected runtime so Settings can show real status. */
export function runtimeForProvider(
  provider: ProviderIconId,
  runtimes: Runtime[],
): Runtime | undefined {
  return runtimes.find((runtime) => providerIconId(undefined, runtime.adapterId) === provider)
}

export function ProviderIcon({ name, adapterId }: {
  name: string | undefined
  adapterId: string | undefined
}) {
  const id = providerIconId(name, adapterId)
  if (!id) return null
  const provider = providerMetadata[id]
  return <img className="provider-icon" src={provider.src} alt={provider.label} title={provider.label} />
}

/**
 * Only the profile name is Relay's own; the model and reasoning are the CLI's to
 * report, so a new profile carries no model and lets the runtime default apply
 * until the CLI's own list is fetched (docs/ui.md 16.1).
 */
export const providerDefaults: Record<ProviderIconId, { profile: string }> = {
  deepseek: { profile: 'DeepSeek Code' },
  glm: { profile: 'GLM Code' },
  gemini: { profile: 'Gemini Code' },
  grok: { profile: 'Grok Code' },
  kimi: { profile: 'Kimi Code' },
}

/** A new agent: named by Relay, model and reasoning left to the CLI (docs/ui.md 16.1). */
export function newProfileFor(provider: ProviderIconId, runtimeId: string): AgentProfile {
  const defaults = providerDefaults[provider]
  return {
    id: `${provider}-${Date.now()}`,
    name: defaults.profile,
    runtimeId,
    description: `${providerMetadata[provider].label} worker added from Relay.`,
    capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: false },
    enabled: true,
  }
}

export function storedFontSize(): number {
  const value = Number(localStorage.getItem('relay.font-size'))
  return Number.isInteger(value) && value >= DEFAULT_FONT_SIZE && value <= MAX_FONT_SIZE
    ? value
    : DEFAULT_FONT_SIZE
}

export function elapsed(start: string, end?: string): string {
  const total = Math.max(0, new Date(end ?? Date.now()).getTime() - new Date(start).getTime())
  const seconds = Math.floor(total / 1_000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  return `${minutes}m ${seconds % 60}s`
}

/** Compact sidebar timestamp, including a live hint for active sessions. */
export function relativeTime(timestamp: string, status: string, t: Translator): string {
  if (status === 'active') return `${t('run.status.running')} · ${elapsed(timestamp)}`
  const date = new Date(timestamp)
  const now = new Date()
  const sameDay = date.toDateString() === now.toDateString()
  if (sameDay) return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString([], { month: 'short', day: 'numeric' })
  }
  return date.toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' })
}

/** ✓ / ● / ○ status glyph from docs/ui.md 8. */
export function statusGlyph(status: RunStatus | StepStatus): string {
  if (status === 'completed') return '✓'
  if (status === 'running' || status === 'starting') return '●'
  if (status === 'failed' || status === 'orphaned' || status === 'cancelled' || status === 'interrupted') return '✕'
  if (status === 'awaiting_host') return '◆'
  return '○'
}

export function StatusLabel({ status, t }: { status: RunStatus; t: Translator }) {
  return <span className={`status-label ${status}`}><i />{t(`run.status.${status}`)}</span>
}

/* ------------------------------------------------------------------ */
/* Console event mapping (docs/ui.md 12.2)                             */
/* ------------------------------------------------------------------ */

export type ConsoleKind =
  | 'read'
  | 'search'
  | 'edit'
  | 'create'
  | 'delete'
  | 'command'
  | 'test'
  | 'build'
  | 'result'
  | 'error'
  | 'warning'
  | 'install'
  | 'download'
  | 'commit'
  | 'status'

export interface ConsoleRow {
  id: string
  kind: ConsoleKind
  timestamp: string
  label: string
  additions?: number
  deletions?: number
  /** Repeated low-value events grouped per docs/ui.md 12.3. */
  count?: number
}

/**
 * Maps one observable event to a Console category. Returns undefined for events
 * Console must not show: hidden reasoning, runtime-internal child agents, and
 * relay lifecycle bookkeeping (docs/ui.md 7.3, 12.4, 23).
 */
export function eventKind(event: RelayEvent): ConsoleKind | undefined {
  switch (event.type) {
    case 'tool/read':
      return 'read'
    case 'tool/search':
      return 'search'
    case 'tool/edit':
      return 'edit'
    case 'tool/command':
      return 'command'
    case 'test/result':
      return 'test'
    case 'tool/result':
    case 'worker/completed':
      return 'result'
    case 'worker/failed':
    case 'worker/orphaned':
      return 'error'
    case 'worker/cancelled':
    case 'worker/interrupted':
      return 'warning'
    case 'worker/started':
    case 'worker/message':
    case 'worker/status':
    case 'run/awaiting_host':
    case 'run/accepted':
      return 'status'
    default:
      // run/created, step/*, child/* and worker/reasoning are intentionally hidden.
      return undefined
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : undefined
}

export function eventSummary(event: RelayEvent): string {
  const data = asRecord(event.data)
  if (!data) return String(event.data ?? event.type)
  const value =
    data.path ?? data.file ?? data.files ?? data.command ?? data.query ?? data.summary ??
    data.text ?? data.message ?? data.result ?? data.status ?? data.tool
  if (value !== undefined) {
    if (Array.isArray(value)) return value.length === 1 ? String(value[0]) : `${value.length}`
    return typeof value === 'string' ? value : JSON.stringify(value)
  }
  if (event.type === 'worker/started') {
    const worker = asRecord(data.worker)
    return worker?.runtimeId ? String(worker.runtimeId) : 'Worker started'
  }
  if (event.type === 'run/awaiting_host') return 'Worker finished; waiting for Codex review'
  if (event.type === 'run/accepted') return 'Accepted by Codex'
  return event.type
}

/** Diff stat for an edit, when the CLI reported one (docs/ui.md 12.2 "+42 -12"). */
function diffStat(event: RelayEvent): { additions: number; deletions: number } | undefined {
  const data = asRecord(event.data)
  if (!data) return undefined
  const diff = asRecord(data.diff)
  const additions = Number(diff?.additions ?? data.additions)
  const deletions = Number(diff?.deletions ?? data.deletions)
  if (!Number.isFinite(additions) && !Number.isFinite(deletions)) return undefined
  return {
    additions: Number.isFinite(additions) ? additions : 0,
    deletions: Number.isFinite(deletions) ? deletions : 0,
  }
}

function toRow(event: RelayEvent): ConsoleRow | undefined {
  const kind = eventKind(event)
  if (!kind) return undefined
  const stat = kind === 'edit' ? diffStat(event) : undefined
  return {
    id: event.id,
    kind,
    timestamp: event.timestamp,
    label: eventSummary(event),
    ...(stat ?? {}),
  }
}

/**
 * Builds Console rows, collapsing runs of three or more consecutive Read/Search
 * events into one summary row so repetitive low-value work stays quiet.
 */
export function consoleRows(events: RelayEvent[]): ConsoleRow[] {
  const rows: ConsoleRow[] = []
  let index = 0
  while (index < events.length) {
    const event = events[index]
    if (!event) break
    const row = toRow(event)
    if (!row) {
      index += 1
      continue
    }
    if (row.kind !== 'read' && row.kind !== 'search') {
      rows.push(row)
      index += 1
      continue
    }
    let end = index
    while (end + 1 < events.length) {
      const candidate = events[end + 1]
      const next = candidate ? toRow(candidate) : undefined
      if (!next || next.kind !== row.kind) break
      end += 1
    }
    const run = events.slice(index, end + 1)
    if (run.length >= 3) {
      rows.push({
        id: `${row.id}:group`,
        kind: row.kind,
        timestamp: row.timestamp,
        label: row.label,
        count: run.length,
      })
    } else {
      for (const item of run) {
        const single = toRow(item)
        if (single) rows.push(single)
      }
    }
    index = end + 1
  }
  return rows
}

/* ------------------------------------------------------------------ */
/* Small shared views                                                  */
/* ------------------------------------------------------------------ */

export function ConsoleRows({ rows, t }: { rows: ConsoleRow[]; t: Translator }) {
  return (
    <>
      {rows.map((row) => (
        <div className={`console-row ${row.kind}`} key={row.id}>
          <time>{new Date(row.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
          <span className="console-kind">{t(`console.${row.kind}`)}</span>
          <code>
            {row.label}
            {row.count ? <i className="console-count"> {row.count} {t('console.files')}</i> : null}
            {row.additions !== undefined || row.deletions !== undefined ? (
              <i className="console-stat">
                {row.additions ? ` +${row.additions}` : ''}
                {row.deletions ? ` -${row.deletions}` : ''}
              </i>
            ) : null}
          </code>
        </div>
      ))}
    </>
  )
}

/** Provider rows shared by onboarding step 2 (docs/ui.md 22.2 "Add Agents"). */
export function ProviderRow({
  provider,
  status,
  action,
}: {
  provider: ProviderIconId
  status: string
  action?: ReactNode
}) {
  const metadata = providerMetadata[provider]
  return (
    <div className="provider-row">
      <img className="provider-icon" src={metadata.src} alt="" />
      <span className="provider-row-name">{metadata.label}</span>
      <span className="provider-row-status">{status}</span>
      {action}
    </div>
  )
}

export { createTranslator }

/* ------------------------------------------------------------------ */
/* Runtime options (model list and reasoning levels from the CLI)       */
/* ------------------------------------------------------------------ */

const runtimeOptionsCache = new Map<string, Promise<RuntimeOptions>>()

/**
 * Reads a runtime's model list and reasoning levels from its CLI. Results are
 * shared across mounts so opening several editors does not spawn the CLI probe
 * repeatedly. The UI renders only what the CLI reports (docs/ui.md 16.1).
 */
export function useRuntimeOptions(runtimeId: string | undefined): {
  options: RuntimeOptions | undefined
  loading: boolean
} {
  const [options, setOptions] = useState<RuntimeOptions>()
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!runtimeId) {
      setOptions(undefined)
      return
    }
    let active = true
    setLoading(true)
    const pending = runtimeOptionsCache.get(runtimeId) ?? window.relay.runtimeOptions(runtimeId)
    runtimeOptionsCache.set(runtimeId, pending)
    void pending
      .then((value) => { if (active) { setOptions(value); setLoading(false) } })
      .catch(() => {
        // A failed probe must not cache, or the retry is stuck with the failure.
        runtimeOptionsCache.delete(runtimeId)
        if (active) { setOptions(undefined); setLoading(false) }
      })
    return () => { active = false }
  }, [runtimeId])

  return { options, loading }
}

