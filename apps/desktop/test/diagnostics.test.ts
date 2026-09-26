import { describe, expect, it } from 'vitest'
import type { AgentProfile, HostSession, Run, Runtime, Step, WorkerSession } from '@relay/protocol'
import type { CodexIntegrationStatus, DesktopSnapshot } from '../src/shared/api.js'
import { buildDiagnosticsReport } from '../src/main/diagnostics.js'

const now = '2026-01-01T10:00:00.000Z'

const session: HostSession = {
  id: 'codex:one',
  host: 'codex',
  nativeSessionId: 'one',
  displayName: 'Refine the menu bar',
  nameSource: 'codex',
  cwd: '/tmp/relay',
  status: 'active',
  startedAt: now,
  updatedAt: now,
}

const runtime: Runtime = {
  id: 'runtime:deepseek',
  adapterId: 'deepseek-harness',
  executablePath: '/usr/local/bin/dsh',
  version: '1.2.3',
  health: 'available',
  capabilities: {
    nonInteractive: true,
    structuredEvents: true,
    cwd: true,
    resume: true,
    send: false,
    cancel: true,
    childSessions: false,
  },
}

const profile: AgentProfile = {
  id: 'deepseek-code',
  name: 'DeepSeek Code',
  runtimeId: 'runtime:deepseek',
  description: 'Coding worker',
  capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: false },
  enabled: true,
}

const run: Run = {
  id: 'run-1',
  hostSessionId: 'codex:one',
  profileId: 'deepseek-code',
  task: 'Refactor the adapter registry',
  cwd: '/tmp/relay',
  accessMode: 'write',
  isolation: 'shared',
  status: 'running',
  createdAt: now,
  updatedAt: now,
}

const step: Step = {
  id: 'step-1',
  runId: 'run-1',
  profileId: 'deepseek-code',
  task: 'Refactor the adapter registry',
  accessMode: 'write',
  isolation: 'shared',
  status: 'running',
  iteration: 1,
  createdAt: now,
  updatedAt: now,
}

const worker: WorkerSession = {
  id: 'worker-1',
  runId: 'run-1',
  stepId: 'step-1',
  iteration: 1,
  runtimeId: 'runtime:deepseek',
  status: 'running',
  startedAt: now,
}

const codex: CodexIntegrationStatus = {
  configured: false,
  checks: [
    { id: 'codex-cli', ok: true, detail: 'codex 1.0.0' },
    { id: 'relay-mcp', ok: false, detail: '/tmp/.codex/config.toml' },
  ],
}

function snapshot(overrides: Partial<DesktopSnapshot> = {}): DesktopSnapshot {
  return {
    sessions: [session],
    runs: [{ run, steps: [step], workers: [worker], events: [] }],
    runtimes: [runtime],
    profiles: [profile],
    diagnostics: ['gemini: no executable found'],
    settings: { policy: {} as never, workspaceOverrides: {}, menuBar: { hideDockIcon: false } },
    refreshedAt: now,
    ...overrides,
  }
}

describe('buildDiagnosticsReport', () => {
  it('summarises the projection a support request would need', () => {
    const report = buildDiagnosticsReport({
      appVersion: '0.0.0',
      electronVersion: '44.4.5',
      chromeVersion: '140.0.0',
      nodeVersion: '24.14.0',
      platform: 'darwin',
      arch: 'arm64',
      databasePath: '/tmp/relay.sqlite',
      settingsPath: '/tmp/settings.json',
      snapshot: snapshot(),
      codex,
      menuBar: { hideDockIcon: true, launchAtLogin: false },
      generatedAt: now,
    })
    expect(report).toContain('Generated: 2026-01-01T10:00:00.000Z')
    expect(report).toContain('Relay 0.0.0 · Electron 44.4.5 · Chromium 140.0.0 · Node 24.14.0')
    expect(report).toContain('Platform: darwin arm64')
    expect(report).toContain('Sessions: 1')
    expect(report).toContain('Runs: 1 (running 1)')
    expect(report).toContain('Workers: 1 (1 active)')
    expect(report).toContain('Runtimes: 1 (available 1)')
    expect(report).toContain('Profiles: 1 (1 enabled)')
    expect(report).toContain('Menu bar: hide dock icon on, launch at login off')
    expect(report).toContain('✓ codex-cli — codex 1.0.0')
    expect(report).toContain('✗ relay-mcp — /tmp/.codex/config.toml')
    expect(report).toContain('- deepseek-harness · available · /usr/local/bin/dsh · 1.2.3')
    expect(report).toContain('Database: /tmp/relay.sqlite')
    expect(report).toContain('Adapter diagnostics\n- gemini: no executable found')
  })

  it('says so instead of printing empty sections', () => {
    const report = buildDiagnosticsReport({
      appVersion: '0.0.0',
      electronVersion: '44.4.5',
      chromeVersion: '140.0.0',
      nodeVersion: '24.14.0',
      platform: 'darwin',
      arch: 'arm64',
      databasePath: '/tmp/relay.sqlite',
      settingsPath: '/tmp/settings.json',
      snapshot: snapshot({ runtimes: [], profiles: [], runs: [], sessions: [], diagnostics: [] }),
      codex,
      menuBar: { hideDockIcon: false, launchAtLogin: false },
      generatedAt: now,
    })
    expect(report).toContain('Runs: 0')
    expect(report).not.toContain('Runs: 0 (')
    expect(report).toContain('Runtimes: 0')
    expect(report).toContain('(none detected)')
    expect(report).not.toContain('Adapter diagnostics')
    expect(report.endsWith('\n')).toBe(true)
  })
})
