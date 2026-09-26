import type {
  AgentProfile,
  HostSession,
  Locale,
  RelayEvent,
  RelayPolicy,
  RelayPolicyOverride,
  Run,
  Runtime,
  RuntimeOptions,
  Step,
  WorkerSession,
} from '@relay/protocol'

export interface DesktopRunView {
  run: Run
  steps: Step[]
  workers: WorkerSession[]
  events: RelayEvent[]
}

/**
 * Menu bar presentation switches. Both describe how Relay shows up on the
 * machine, never how work is routed: routing policy stays in RelayPolicy and
 * the choice of agent stays with Codex (docs/architecture.md 1).
 */
export interface DesktopMenuBarPrefs {
  /** macOS accessory mode: menu bar icon only, no Dock icon and no app menu. */
  hideDockIcon: boolean
}

export interface DesktopSettings {
  policy: RelayPolicy
  workspaceOverrides: Record<string, RelayPolicyOverride>
  menuBar: DesktopMenuBarPrefs
  /**
   * Set once first-run onboarding finishes. Its absence is what routes a launch
   * to onboarding instead of the normal workspace (docs/ui.md 22.4).
   */
  onboardingCompletedAt?: string
}

export interface DesktopSnapshot {
  sessions: HostSession[]
  runs: DesktopRunView[]
  runtimes: Runtime[]
  profiles: AgentProfile[]
  diagnostics: string[]
  settings: DesktopSettings
  refreshedAt: string
}

/** One real check from docs/ui.md 22.2 "Connect Codex". */
export interface CodexIntegrationCheck {
  id: 'codex-cli' | 'relay-mcp' | 'relay-skill'
  ok: boolean
  /** Where the check looked, shown so manual setup is possible when automatic fails. */
  detail: string
}

export interface CodexIntegrationStatus {
  checks: CodexIntegrationCheck[]
  configured: boolean
}

/** Result of the explicit first-run Codex installation step. */
export interface CodexIntegrationInstallResult {
  status: CodexIntegrationStatus
  messages: string[]
}

/**
 * A navigation request from the menu bar. The renderer only selects what the
 * request names; it never learns why, so the menu cannot smuggle in state the
 * snapshot does not already contain.
 */
export interface DesktopNavigateRequest {
  hostSessionId?: string
  runId?: string
  settingsSection?: 'general' | 'agents' | 'workspace' | 'advanced'
  /** One-off confirmation from the menu bar, e.g. a copied report. */
  notice?: string
}

export interface RelayDesktopApi {
  snapshot(): Promise<DesktopSnapshot>
  saveSettings(settings: DesktopSettings): Promise<DesktopSettings>
  saveProfile(profile: AgentProfile): Promise<AgentProfile>
  cancelWorker(workerSessionId: string): Promise<{ accepted: boolean; message?: string }>
  cancelSessionWorkers(hostSessionId: string): Promise<{ accepted: boolean; count: number }>
  codexStatus(): Promise<CodexIntegrationStatus>
  installCodexIntegration(): Promise<CodexIntegrationInstallResult>
  runtimeOptions(runtimeId: string): Promise<RuntimeOptions>
  completeOnboarding(): Promise<DesktopSettings>
  openWorkspace(path: string): Promise<{ ok: boolean; message?: string }>
  /** Menu bar navigation; returns an unsubscribe function. */
  onNavigate(listener: (request: DesktopNavigateRequest) => void): () => void
  /** Lets the menu bar follow the language chosen in the window. */
  setLocale(locale: Locale): void
}
