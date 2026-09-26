import { contextBridge, ipcRenderer } from 'electron'

/**
 * The tray drives navigation inside the panel instead of reloading it: a menu
 * pick such as "新建智能体…" becomes a message, so the panel keeps its state and
 * no second window is ever created (docs/menu-bar.md 3).
 */
export interface PanelNavigate {
  tab?: 'agents' | 'policy' | 'codex' | 'runtime'
  intent?: 'new-agent' | 'edit-agent' | 'add-runtime' | 'codex-actions'
  profileId?: string
}

contextBridge.exposeInMainWorld('relayPanel', {
  onNavigate(listener: (request: PanelNavigate) => void): () => void {
    const handler = (_event: unknown, request: PanelNavigate) => listener(request)
    ipcRenderer.on('relay:panel', handler)
    return () => ipcRenderer.removeListener('relay:panel', handler)
  },
  openInspector(): Promise<void> {
    return ipcRenderer.invoke('relay:open-inspector') as Promise<void>
  },
})
