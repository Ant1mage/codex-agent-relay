import type {
  AgentProfile,
  HostSession,
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

export interface DesktopSettings {
  policy: RelayPolicy
  workspaceOverrides: Record<string, RelayPolicyOverride>
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

export interface RelayDesktopApi {
  snapshot(): Promise<DesktopSnapshot>
  saveSettings(settings: DesktopSettings): Promise<DesktopSettings>
  saveProfile(profile: AgentProfile): Promise<AgentProfile>
  cancelWorker(workerSessionId: string): Promise<{ accepted: boolean; message?: string }>
  cancelSessionWorkers(hostSessionId: string): Promise<{ accepted: boolean; count: number }>
  codexStatus(): Promise<CodexIntegrationStatus>
  runtimeOptions(runtimeId: string): Promise<RuntimeOptions>
  completeOnboarding(): Promise<DesktopSettings>
  openWorkspace(path: string): Promise<{ ok: boolean; message?: string }>
}
