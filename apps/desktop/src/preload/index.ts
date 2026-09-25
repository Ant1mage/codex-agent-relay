import { contextBridge, ipcRenderer } from 'electron'
import type { AgentProfile } from '@relay/protocol'
import type { DesktopSettings, RelayDesktopApi } from '../shared/api.js'

const api: RelayDesktopApi = {
  snapshot: () => ipcRenderer.invoke('relay:snapshot'),
  saveSettings: (settings: DesktopSettings) => ipcRenderer.invoke('relay:settings:save', settings),
  saveProfile: (profile: AgentProfile) => ipcRenderer.invoke('relay:profile:save', profile),
  cancelWorker: (workerSessionId: string) =>
    ipcRenderer.invoke('relay:worker:cancel', workerSessionId),
}

contextBridge.exposeInMainWorld('relay', api)
