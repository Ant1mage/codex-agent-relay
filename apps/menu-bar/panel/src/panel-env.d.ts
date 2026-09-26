export interface PanelNavigateRequest {
  tab?: 'agents' | 'policy' | 'codex' | 'runtime'
  intent?: 'new-agent' | 'edit-agent' | 'add-runtime' | 'codex-actions'
  profileId?: string
}

declare global {
  interface Window {
    relayPanel?: { onNavigate(listener: (request: PanelNavigateRequest) => void): () => void }
  }
}

export {}
