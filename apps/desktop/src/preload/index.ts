import { contextBridge, ipcRenderer } from 'electron'
import type { AgentProfile } from '@relay/protocol'
import type { DesktopSettings, RelayDesktopApi } from '../shared/api.js'

const api: RelayDesktopApi = {
  snapshot: () => ipcRenderer.invoke('relay:snapshot'),
  saveSettings: (settings: DesktopSettings) => ipcRenderer.invoke('relay:settings:save', settings),
  saveProfile: (profile: AgentProfile) => ipcRenderer.invoke('relay:profile:save', profile),
  cancelWorker: (workerSessionId: string) =>
    ipcRenderer.invoke('relay:worker:cancel', workerSessionId),
  cancelSessionWorkers: (hostSessionId: string) =>
    ipcRenderer.invoke('relay:session:cancel', hostSessionId),
  codexStatus: () => ipcRenderer.invoke('relay:codex:status'),
  installCodexIntegration: () => ipcRenderer.invoke('relay:codex:install'),
  runtimeOptions: (runtimeId: string) => ipcRenderer.invoke('relay:runtime:options', runtimeId),
  completeOnboarding: () => ipcRenderer.invoke('relay:onboarding:complete'),
  openWorkspace: (path: string) => ipcRenderer.invoke('relay:workspace:open', path),
}

contextBridge.exposeInMainWorld('relay', api)
