import { create } from 'zustand'
import { resolveLocale } from '@relay/i18n'
import type { InspectorSnapshot, RelayClient } from '@relay/relay-api'
import type { Locale, RelayEvent } from '@relay/protocol'

export type Connection = 'connecting' | 'live' | 'error'
export type InspectorTab = 'console' | 'changes' | 'raw'

/**
 * Client-side projection. The daemon owns the truth; this store only merges what
 * arrived over SSE, de-duplicating by sequence so a reconnect, a history fetch
 * and a live delta can all land in the same list (docs/inspector.md 3).
 */
export function mergeEvents(existing: RelayEvent[] | undefined, incoming: RelayEvent[]): RelayEvent[] {
  if (!existing || existing.length === 0) return [...incoming].sort((l, r) => l.seq - r.seq)
  if (incoming.length === 0) return existing
  const bySeq = new Map<number, RelayEvent>()
  for (const event of existing) bySeq.set(event.seq, event)
  for (const event of incoming) bySeq.set(event.seq, event)
  return [...bySeq.values()].sort((left, right) => left.seq - right.seq)
}

interface AppState {
  snapshot: InspectorSnapshot | undefined
  events: Record<string, RelayEvent[]>
  connection: Connection
  error: string | undefined
  selectedSessionId: string | undefined
  selectedRunId: string | undefined
  selectedStepId: string | undefined
  tab: InspectorTab
  locale: Locale
  notice: string | undefined
  setConnection(connection: Connection, error?: string): void
  setSnapshot(snapshot: InspectorSnapshot): void
  mergeRunEvents(runId: string, events: RelayEvent[]): void
  select(sessionId: string | undefined, runId: string | undefined, stepId?: string): void
  selectStep(stepId: string): void
  setTab(tab: InspectorTab): void
  setLocale(locale: Locale): void
  setNotice(notice?: string): void
}

const storedLocale = typeof localStorage === 'undefined' ? undefined : localStorage.getItem('relay.locale')
const initialLocale: Locale =
  storedLocale === 'en' || storedLocale === 'zh-CN' ? storedLocale : resolveLocale(navigator.language)

export const useAppStore = create<AppState>((set) => ({
  snapshot: undefined,
  events: {},
  connection: 'connecting',
  error: undefined,
  selectedSessionId: undefined,
  selectedRunId: undefined,
  selectedStepId: undefined,
  tab: 'console',
  locale: initialLocale,
  notice: undefined,
  setConnection: (connection, error) => set({ connection, error }),
  setSnapshot: (snapshot) => set({ snapshot }),
  mergeRunEvents: (runId, events) =>
    set((state) => ({ events: { ...state.events, [runId]: mergeEvents(state.events[runId], events) } })),
  select: (selectedSessionId, selectedRunId, stepId) =>
    set({ selectedSessionId, selectedRunId, selectedStepId: stepId }),
  selectStep: (selectedStepId) => set({ selectedStepId }),
  setTab: (tab) => set({ tab }),
  setLocale: (locale) => {
    localStorage.setItem('relay.locale', locale)
    set({ locale })
  },
  setNotice: (notice) => set({ notice }),
}))

/** Keeps the live stream wired to the store for the lifetime of the page. */
export function connectStream(client: RelayClient, store = useAppStore.getState): () => void {
  const { setConnection, setSnapshot, mergeRunEvents } = store()
  void client
    .snapshot()
    .then((snapshot) => setSnapshot(snapshot))
    .catch((error: unknown) => setConnection('error', error instanceof Error ? error.message : String(error)))
  return client.stream({
    onOpen: () => setConnection('live'),
    onError: (error) => setConnection('error', error instanceof Error ? error.message : String(error)),
    onMessage: (message) => {
      if (message.type === 'snapshot') {
        setSnapshot(message.snapshot)
        setConnection('live')
        return
      }
      if (message.type === 'events') {
        mergeRunEvents(message.batch.runId, message.batch.events)
      }
    },
  })
}
