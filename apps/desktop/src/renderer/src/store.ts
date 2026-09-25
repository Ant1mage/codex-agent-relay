import { create } from 'zustand'
import type { Locale } from '@relay/protocol'
import type { DesktopSnapshot } from '../../shared/api.js'
import { resolveLocale } from '@relay/i18n'

type View = 'sessions' | 'agents' | 'settings'

interface AppState {
  view: View
  locale: Locale
  snapshot: DesktopSnapshot | undefined
  selectedSessionId: string | undefined
  selectedRunId: string | undefined
  loading: boolean
  error: string | undefined
  notice: string | undefined
  setView(view: View): void
  setLocale(locale: Locale): void
  selectSession(id: string): void
  selectRun(id: string): void
  setNotice(notice?: string): void
  refresh(): Promise<void>
}

const storedLocale = localStorage.getItem('relay.locale')
const initialLocale: Locale =
  storedLocale === 'en' || storedLocale === 'zh-CN'
    ? storedLocale
    : resolveLocale(navigator.language)

export const useAppStore = create<AppState>((set, get) => ({
  view: 'sessions',
  locale: initialLocale,
  snapshot: undefined,
  selectedSessionId: undefined,
  selectedRunId: undefined,
  loading: true,
  error: undefined,
  notice: undefined,
  setView: (view) => set({ view }),
  setLocale: (locale) => {
    localStorage.setItem('relay.locale', locale)
    set({ locale })
  },
  selectSession: (selectedSessionId) => set({ selectedSessionId, selectedRunId: undefined }),
  selectRun: (selectedRunId) => set({ selectedRunId }),
  setNotice: (notice) => set({ notice }),
  refresh: async () => {
    set({ loading: true, error: undefined })
    try {
      const snapshot = await window.relay.snapshot()
      const selectedSessionId =
        get().selectedSessionId && snapshot.sessions.some((item) => item.id === get().selectedSessionId)
          ? get().selectedSessionId
          : snapshot.sessions[0]?.id
      set({ snapshot, selectedSessionId, loading: false })
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      })
    }
  },
}))
