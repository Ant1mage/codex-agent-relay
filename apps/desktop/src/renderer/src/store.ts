import { create } from 'zustand'
import type { Locale, Step } from '@relay/protocol'
import type { DesktopRunView, DesktopSnapshot } from '../../shared/api.js'
import { resolveLocale } from '@relay/i18n'

/**
 * The step the user should be looking at for a run: the active one when work is
 * in flight, otherwise the first (docs/ui.md 7.4 keeps current execution visible).
 */
function preferredStepId(steps: Step[] | undefined): string | undefined {
  return steps?.find((step) => step.status === 'running')?.id ?? steps?.[0]?.id
}

/** Settings pages the menu bar can open directly. */
export type SettingsSection = 'general' | 'agents' | 'workspace' | 'advanced'

interface AppState {
  locale: Locale
  snapshot: DesktopSnapshot | undefined
  selectedSessionId: string | undefined
  selectedRunId: string | undefined
  selectedStepId: string | undefined
  settingsOpen: boolean
  /**
   * Page Settings should show the next time it is open. Set by the menu bar,
   * which can point at the Agents page but cannot render it (docs/menu-bar.md).
   */
  settingsSection: SettingsSection | undefined
  onboardingOpen: boolean
  /** Prevents polling from treating an unfinished setup value as a first load forever. */
  hasLoadedInitialSnapshot: boolean
  loading: boolean
  error: string | undefined
  notice: string | undefined
  setSettingsOpen(open: boolean): void
  openSettings(section?: SettingsSection): void
  openOnboarding(): void
  closeOnboarding(): void
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
  settingsSection: undefined,
  onboardingOpen: false,
  hasLoadedInitialSnapshot: false,
  loading: true,
  error: undefined,
  notice: undefined,
  setSettingsOpen: (settingsOpen) =>
    set(settingsOpen ? { settingsOpen } : { settingsOpen, settingsSection: undefined }),
  openSettings: (section) => set({ settingsOpen: true, settingsSection: section }),
  openOnboarding: () => set({ onboardingOpen: true }),
  closeOnboarding: () => set({ onboardingOpen: false }),
  setLocale: (locale) => {
    localStorage.setItem('relay.locale', locale)
    set({ locale })
  },
  selectSession: (selectedSessionId) => {
    const runs = get().snapshot?.runs ?? []
    const selectedRunId = runs.find((item) => item.run.hostSessionId === selectedSessionId)?.run.id
    const selectedStepId = preferredStepId(runs.find((item) => item.run.id === selectedRunId)?.steps)
    // Remember the session so the next launch restores it (docs/ui.md 22.4).
    localStorage.setItem('relay.last-session', selectedSessionId)
    set({ selectedSessionId, selectedRunId, selectedStepId })
  },
  selectRun: (selectedRunId) => {
    const selectedStepId = preferredStepId(
      get().snapshot?.runs.find((item) => item.run.id === selectedRunId)?.steps,
    )
    set({ selectedRunId, selectedStepId })
  },
  selectStep: (selectedRunId, selectedStepId) => set({ selectedRunId, selectedStepId }),
  setNotice: (notice) => set({ notice }),
  refresh: async () => {
    set({ loading: true, error: undefined })
    try {
      const snapshot = await window.relay.snapshot()
      const { selectedSessionId: currentSessionId, selectedRunId: currentRunId, selectedStepId: currentStepId } = get()
      const lastSessionId = localStorage.getItem('relay.last-session') ?? undefined
      // Restore the last viewed session, falling back to the newest one.
      const sessionId = [currentSessionId, lastSessionId].find((id) =>
        id ? snapshot.sessions.some((session) => session.id === id) : false,
      ) ?? snapshot.sessions[0]?.id
      const sessionRuns = snapshot.runs.filter((item) => item.run.hostSessionId === sessionId)
      const runId = (currentRunId && sessionRuns.some((item) => item.run.id === currentRunId)
        ? currentRunId
        : undefined) ??
        sessionRuns.find((item) => item.run.status === 'running')?.run.id ??
        sessionRuns.find((item) => item.run.status === 'starting')?.run.id ??
        sessionRuns[0]?.run.id
      const selectedRun = sessionRuns.find((item) => item.run.id === runId)
      const stepId = (currentStepId && selectedRun?.steps.some((step) => step.id === currentStepId)
        ? currentStepId
        : undefined) ?? preferredStepId(selectedRun?.steps)
      /*
       * The first-run gate is decided once, when the settings document first
       * arrives. Later polls must not recompute it: they run every two seconds
       * and would immediately close a setup screen the user reopened from the
       * toolbar (docs/ui.md 22.3/22.4).
       */
      const completedAt = snapshot.settings.onboardingCompletedAt
      const firstSnapshot = !get().hasLoadedInitialSnapshot
      set({
        snapshot,
        selectedSessionId: sessionId,
        selectedRunId: runId,
        selectedStepId: stepId,
        loading: false,
        hasLoadedInitialSnapshot: true,
        ...(firstSnapshot ? { onboardingOpen: !completedAt } : {}),
      })
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      })
    }
  },
}))

export type { AppState }
