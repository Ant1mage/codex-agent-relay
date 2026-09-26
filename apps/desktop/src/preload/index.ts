import { contextBridge, ipcRenderer } from 'electron'
import type { AgentProfile, Locale } from '@relay/protocol'
import type {
  DesktopNavigateRequest,
  DesktopSettings,
  RelayDesktopApi,
} from '../shared/api.js'

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
  // Menu bar navigation is push-only: the renderer hears what the user picked in
  // the tray and never asks the tray for state it already has in its snapshot.
  onNavigate: (listener: (request: DesktopNavigateRequest) => void) => {
    const handler = (_event: unknown, request: DesktopNavigateRequest) => listener(request)
    ipcRenderer.on('relay:navigate', handler)
    return () => ipcRenderer.removeListener('relay:navigate', handler)
  },
  setLocale: (locale: Locale) => ipcRenderer.send('relay:locale:set', locale),
}

contextBridge.exposeInMainWorld('relay', api)
