import { create } from 'zustand'
import type { Locale } from '@relay/protocol'
import type { DesktopSnapshot } from '../../shared/api.js'
import { resolveLocale } from '@relay/i18n'

interface AppState {
  locale: Locale
  snapshot: DesktopSnapshot | undefined
  selectedSessionId: string | undefined
  selectedRunId: string | undefined
  selectedStepId: string | undefined
  settingsOpen: boolean
  loading: boolean
  error: string | undefined
  notice: string | undefined
  setSettingsOpen(open: boolean): void
  setLocale(locale: Locale): void
  selectSession(id: string): void
  selectRun(id: string): void
  selectStep(runId: string, stepId: string): void
  setNotice(notice?: string): void
  refresh(): Promise<void>
}

const storedLocale = localStorage.getItem('relay.locale')
const initialLocale: Locale =
  storedLocale === 'en' || storedLocale === 'zh-CN'
    ? storedLocale
    : resolveLocale(navigator.language)

export const useAppStore = create<AppState>((set, get) => ({
  locale: initialLocale,
  snapshot: undefined,
  selectedSessionId: undefined,
  selectedRunId: undefined,
  selectedStepId: undefined,
  settingsOpen: false,
  loading: true,
  error: undefined,
  notice: undefined,
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  setLocale: (locale) => {
    localStorage.setItem('relay.locale', locale)
    set({ locale })
  },
  selectSession: (selectedSessionId) => {
    const selectedRunId = get().snapshot?.runs.find(
      (item) => item.run.hostSessionId === selectedSessionId,
    )?.run.id
    const selectedStepId = get().snapshot?.runs.find((item) => item.run.id === selectedRunId)?.steps[0]?.id
    set({ selectedSessionId, selectedRunId, selectedStepId })
  },
  selectRun: (selectedRunId) => {
    const selectedStepId = get().snapshot?.runs.find((item) => item.run.id === selectedRunId)?.steps[0]?.id
    set({ selectedRunId, selectedStepId })
  },
  selectStep: (selectedRunId, selectedStepId) => set({ selectedRunId, selectedStepId }),
  setNotice: (notice) => set({ notice }),
  refresh: async () => {
    set({ loading: true, error: undefined })
    try {
      const snapshot = await window.relay.snapshot()
      const selectedSessionId =
        get().selectedSessionId && snapshot.sessions.some((item) => item.id === get().selectedSessionId)
          ? get().selectedSessionId
          : snapshot.sessions[0]?.id
      const sessionRuns = snapshot.runs.filter(
        (item) => item.run.hostSessionId === selectedSessionId,
      )
      const selectedRunId =
        get().selectedRunId && sessionRuns.some((item) => item.run.id === get().selectedRunId)
          ? get().selectedRunId
          : sessionRuns.find((item) => item.run.status === 'running')?.run.id ??
            sessionRuns.find((item) => item.run.status === 'starting')?.run.id ??
            sessionRuns[0]?.run.id
      const selectedRun = sessionRuns.find((item) => item.run.id === selectedRunId)
      const selectedStepId =
        get().selectedStepId && selectedRun?.steps.some((step) => step.id === get().selectedStepId)
          ? get().selectedStepId
          : selectedRun?.steps.find((step) => step.status === 'running')?.id ??
            selectedRun?.steps[0]?.id
      set({ snapshot, selectedSessionId, selectedRunId, selectedStepId, loading: false })
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      })
    }
  },
}))
