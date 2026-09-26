import type { RelayEvent, RunStatus, StepStatus } from '@relay/protocol'
import type { TranslationKey } from '@relay/i18n'

export type Translator = (key: TranslationKey) => string

export function elapsed(start: string, end?: string): string {
  const total = Math.max(0, new Date(end ?? Date.now()).getTime() - new Date(start).getTime())
  const seconds = Math.floor(total / 1_000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

/** Compact "how long ago", with a live hint while a run is still going. */
export function relativeTime(timestamp: string, status: string, t: Translator): string {
  if (status === 'running' || status === 'starting') return t('common.active')
  const seconds = Math.max(0, Math.round((Date.now() - new Date(timestamp).getTime()) / 1_000))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  return new Date(timestamp).toLocaleDateString([], { month: 'short', day: 'numeric' })
}

/** ✓ / ● / ◆ / ✕ / ○ status glyph. */
export function statusGlyph(status: RunStatus | StepStatus): string {
  if (status === 'completed') return '✓'
  if (status === 'running' || status === 'starting') return '●'
  if (status === 'failed' || status === 'orphaned' || status === 'cancelled' || status === 'interrupted') return '✕'
  if (status === 'awaiting_host') return '◆'
  return '○'
}

export type ConsoleKind =
  | 'read'
  | 'search'
  | 'edit'
  | 'command'
  | 'test'
  | 'result'
  | 'error'
  | 'warning'
  | 'status'

export interface ConsoleRow {
  id: string
  kind: ConsoleKind
  timestamp: string
  label: string
  additions?: number
  deletions?: number
  /** Repeated low-value events grouped into one row. */
  count?: number
}

/**
 * Maps one observable event to a Console category, and returns undefined for
 * everything the inspector must not show: hidden reasoning, runtime-internal
 * child agents and relay lifecycle bookkeeping.
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

/** Diff stat for an edit, when the CLI reported one ("+42 -12"). */
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
      const next = toRow(events[end + 1] as RelayEvent)
      if (!next || next.kind !== row.kind) break
      end += 1
    }
    const run = events.slice(index, end + 1)
    if (run.length >= 3) {
      rows.push({ id: `${row.id}:group`, kind: row.kind, timestamp: row.timestamp, label: row.label, count: run.length })
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

/** Every event the inspector shows, in order, for the Raw view. */
export function visibleEvents(events: RelayEvent[]): RelayEvent[] {
  return events.filter((event) => eventKind(event) !== undefined)
}

/** File changes reported by edit events, for the Changes view. */
export function changedFiles(events: RelayEvent[]): Array<{ path: string; additions: number; deletions: number }> {
  const files = new Map<string, { path: string; additions: number; deletions: number }>()
  for (const event of events) {
    if (eventKind(event) !== 'edit') continue
    const data = asRecord(event.data)
    const path = typeof data?.path === 'string' ? data.path : typeof data?.file === 'string' ? data.file : undefined
    if (!path) continue
    const stat = diffStat(event) ?? { additions: 0, deletions: 0 }
    const existing = files.get(path)
    files.set(path, {
      path,
      additions: (existing?.additions ?? 0) + stat.additions,
      deletions: (existing?.deletions ?? 0) + stat.deletions,
    })
  }
  return [...files.values()].sort((left, right) => left.path.localeCompare(right.path))
}
