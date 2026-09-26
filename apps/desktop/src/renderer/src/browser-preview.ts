import type { AgentProfile, RuntimeOptions } from '@relay/protocol'
import type {
  CodexIntegrationStatus,
  DesktopSettings,
  DesktopSnapshot,
  RelayDesktopApi,
} from '../../shared/api.js'

/*
 * Browser-only visual fixture. Electron always provides `window.relay` through
 * preload; the fixture exists only behind `?preview=1` so Chrome/Edge can show
 * the exact renderer without a Node bridge or live user data.
 */
const now = '2026-09-26T12:00:00.000Z'
const previewMode = new URLSearchParams(window.location.search).get('preview')

let settings: DesktopSettings = {
  policy: {
    maxConcurrentRuns: 4,
    maxConcurrentWriters: 2,
    requireWorktreeForParallelWriters: true,
    allowWrite: true,
    allowCommands: true,
    allowNetwork: false,
  },
  workspaceOverrides: {},
  menuBar: { hideDockIcon: false },
  ...(previewMode === 'onboarding' ? {} : { onboardingCompletedAt: now }),
}

let profiles: AgentProfile[] = [{
  id: 'deepseek-code',
  name: 'DeepSeek Code',
  runtimeId: 'runtime:deepseek-preview',
  description: 'A local coding worker for implementation and review.',
  model: 'deepseek-reasoner',
  reasoning: 'high',
  capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: false },
  enabled: true,
}]

const runtimeOptions: RuntimeOptions = {
  runtimeId: 'runtime:deepseek-preview',
  adapterId: 'deepseek-harness',
  models: [
    { value: 'deepseek-chat', label: 'DeepSeek Chat' },
    { value: 'deepseek-reasoner', label: 'DeepSeek Reasoner' },
  ],
  levels: [
    { strength: 1, value: 'low', label: 'Low' },
    { strength: 2, value: 'medium', label: 'Medium' },
    { strength: 3, value: 'high', label: 'High' },
  ],
  source: 'cli',
  diagnostics: [],
}

function snapshot(): DesktopSnapshot {
  return {
    sessions: [{
      id: 'codex-session-ui-review',
      nativeSessionId: 'codex-session-ui-review',
      host: 'codex',
      nameSource: 'codex',
      displayName: 'Refine the Relay desktop interface',
      cwd: '/Users/ant1mage/Desktop/relay',
      model: 'gpt-6',
      status: 'active',
      startedAt: now,
      updatedAt: now,
    }],
    runs: [{
      run: {
        id: 'run-ui-review', hostSessionId: 'codex-session-ui-review', profileId: 'deepseek-code',
        task: 'Review the current settings surface and report visual regressions.',
        cwd: '/Users/ant1mage/Desktop/relay', accessMode: 'propose', isolation: 'worktree',
        status: 'running', createdAt: now, updatedAt: now,
      },
      steps: [{
        id: 'step-ui-review', runId: 'run-ui-review', profileId: 'deepseek-code',
        task: 'Review the current settings surface and report visual regressions.',
        accessMode: 'propose', isolation: 'worktree', status: 'running', iteration: 1,
        createdAt: now, updatedAt: now,
      }],
      workers: [{
        id: 'worker-ui-review', runId: 'run-ui-review', stepId: 'step-ui-review', iteration: 1,
        runtimeId: 'runtime:deepseek-preview', nativeSessionId: 'dsh-preview-session', status: 'running', startedAt: now,
      }],
      events: [
        { id: 'event-1', runId: 'run-ui-review', stepId: 'step-ui-review', workerSessionId: 'worker-ui-review', seq: 1, timestamp: now, type: 'worker/started', data: { status: 'running' } },
        { id: 'event-2', runId: 'run-ui-review', stepId: 'step-ui-review', workerSessionId: 'worker-ui-review', seq: 2, timestamp: now, type: 'tool/read', data: { path: 'docs/ui.md' } },
        { id: 'event-3', runId: 'run-ui-review', stepId: 'step-ui-review', workerSessionId: 'worker-ui-review', seq: 3, timestamp: now, type: 'tool/search', data: { query: 'settings-sheet' } },
        { id: 'event-4', runId: 'run-ui-review', stepId: 'step-ui-review', workerSessionId: 'worker-ui-review', seq: 4, timestamp: now, type: 'tool/edit', data: { path: 'apps/desktop/src/renderer/src/styles.css', diff: { additions: 24, deletions: 11 } } },
      ],
    }],
    runtimes: [{
      id: 'runtime:deepseek-preview', adapterId: 'deepseek-harness', executablePath: 'dsh', version: 'preview', health: 'available',
      capabilities: { nonInteractive: true, structuredEvents: true, cwd: true, resume: true, send: false, cancel: true, childSessions: false, modelSelection: true },
    }],
    profiles,
    diagnostics: [],
    settings,
    refreshedAt: now,
  }
}

const codexStatus: CodexIntegrationStatus = {
  configured: true,
  checks: [
    { id: 'codex-cli', ok: true, detail: 'Codex preview' },
    { id: 'relay-mcp', ok: true, detail: 'Relay preview' },
    { id: 'relay-skill', ok: true, detail: 'Relay preview' },
  ],
}

export function installBrowserPreview(): void {
  const target = window as Window & { relay?: RelayDesktopApi }
  if (target.relay || (previewMode !== '1' && previewMode !== 'onboarding')) return
  target.relay = {
    snapshot: async () => snapshot(),
    saveSettings: async (next) => { settings = next; return settings },
    saveProfile: async (profile) => {
      profiles = profiles.some((candidate) => candidate.id === profile.id)
        ? profiles.map((candidate) => candidate.id === profile.id ? profile : candidate)
        : [...profiles, profile]
      return profile
    },
    cancelWorker: async () => ({ accepted: true }),
    cancelSessionWorkers: async () => ({ accepted: true, count: 1 }),
    codexStatus: async () => codexStatus,
    installCodexIntegration: async () => ({ status: codexStatus, messages: ['Browser preview does not modify Codex'] }),
    runtimeOptions: async () => runtimeOptions,
    completeOnboarding: async () => { settings = { ...settings, onboardingCompletedAt: now }; return settings },
    openWorkspace: async () => ({ ok: true }),
    // The browser has no menu bar; these exist so the fixture satisfies the same
    // contract Electron provides (docs/menu-bar.md).
    onNavigate: () => () => {},
    setLocale: () => {},
  }
  document.title = 'Relay UI preview'
}
