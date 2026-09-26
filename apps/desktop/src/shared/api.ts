import type {
  AgentProfile,
  HostSession,
  RelayEvent,
  RelayPolicy,
  RelayPolicyOverride,
  Run,
  Runtime,
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

export interface RelayDesktopApi {
  snapshot(): Promise<DesktopSnapshot>
  saveSettings(settings: DesktopSettings): Promise<DesktopSettings>
  saveProfile(profile: AgentProfile): Promise<AgentProfile>
  cancelWorker(workerSessionId: string): Promise<{ accepted: boolean; message?: string }>
}
