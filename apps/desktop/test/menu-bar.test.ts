import { describe, expect, it } from 'vitest'
import type { AgentProfile, HostSession, Run, Runtime, Step, WorkerSession } from '@relay/protocol'
import type { CodexIntegrationStatus, DesktopSnapshot } from '../src/shared/api.js'
import {
  buildMenuBarItems,
  menuBarStatus,
  menuBarStatusLabel,
  menuBarViewFromSnapshot,
  taskPreview,
  type MenuBarItem,
  type MenuBarView,
} from '../src/main/menu-bar-model.js'

const now = '2026-01-01T10:00:00.000Z'

const prefs = { hideDockIcon: false, launchAtLogin: false }

function session(overrides: Partial<HostSession> = {}): HostSession {
  return {
    id: 'codex:one',
    host: 'codex',
    nativeSessionId: 'one',
    displayName: 'Refine the menu bar',
    nameSource: 'codex',
    cwd: '/tmp/relay',
    status: 'active',
    startedAt: now,
    updatedAt: now,
    ...overrides,
  }
}

function runtime(overrides: Partial<Runtime> = {}): Runtime {
  return {
    id: 'runtime:deepseek',
    adapterId: 'deepseek-harness',
    executablePath: 'dsh',
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
    ...overrides,
  }
}

function profile(overrides: Partial<AgentProfile> = {}): AgentProfile {
  return {
    id: 'deepseek-code',
    name: 'DeepSeek Code',
    runtimeId: 'runtime:deepseek',
    description: 'Coding worker',
    capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: false },
    enabled: true,
    ...overrides,
  }
}

function run(overrides: Partial<Run> = {}, status: Run['status'] = 'running'): Run {
  return {
    id: 'run-1',
    hostSessionId: 'codex:one',
    profileId: 'deepseek-code',
    task: 'Refactor the adapter registry',
    cwd: '/tmp/relay',
    accessMode: 'write',
    isolation: 'shared',
    status,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  }
}

function step(overrides: Partial<Step> = {}): Step {
  return {
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
    ...overrides,
  }
}

function worker(overrides: Partial<WorkerSession> = {}): WorkerSession {
  return {
    id: 'worker-1',
    runId: 'run-1',
    stepId: 'step-1',
    iteration: 1,
    runtimeId: 'runtime:deepseek',
    status: 'running',
    startedAt: now,
    ...overrides,
  }
}

function snapshot(input: {
  sessions?: HostSession[]
  runs?: DesktopSnapshot['runs']
  runtimes?: Runtime[]
  profiles?: AgentProfile[]
} = {}): DesktopSnapshot {
  return {
    sessions: input.sessions ?? [session()],
    runs: input.runs ?? [{ run: run(), steps: [step()], workers: [worker()], events: [] }],
    runtimes: input.runtimes ?? [runtime()],
    profiles: input.profiles ?? [profile()],
    diagnostics: [],
    settings: { policy: {} as never, workspaceOverrides: {}, menuBar: prefs },
    refreshedAt: now,
  }
}

const codexOk: CodexIntegrationStatus = {
  configured: true,
  checks: [
    { id: 'codex-cli', ok: true, detail: 'codex 1.0.0' },
    { id: 'relay-mcp', ok: true, detail: '/tmp/.codex/config.toml' },
    { id: 'relay-skill', ok: true, detail: '/tmp/.codex/skills/relay/SKILL.md' },
  ],
}

function view(overrides: Partial<MenuBarView> = {}): MenuBarView {
  return {
    locale: 'en',
    platform: 'darwin',
    status: 'ready',
    runningWorkers: 0,
    awaitingHost: 0,
    sessions: [],
    agents: [],
    codex: { configured: true, checks: [] },
    prefs,
    ...overrides,
  }
}

/** Flattens only the top level, which is what most assertions care about. */
function labels(items: MenuBarItem[]): string[] {
  return items.map((item) => `${item.kind}:${item.label}`)
}

describe('menuBarStatus', () => {
  it('reports a missing runtime before anything else', () => {
    expect(menuBarStatus({ runtimes: [], profiles: [] }, { configured: true })).toBe('noRuntime')
  })

  it('needs setup while Codex is not wired up', () => {
    const base = { runtimes: [runtime()], profiles: [profile()] }
    expect(menuBarStatus(base, { configured: false })).toBe('needsSetup')
    expect(menuBarStatus(base, { configured: true })).toBe('ready')
  })

  it('is not ready when every profile points at a runtime that cannot run', () => {
    expect(
      menuBarStatus(
        { runtimes: [runtime({ health: 'authentication_required' })], profiles: [profile()] },
        { configured: true },
      ),
    ).toBe('needsSetup')
  })
})

describe('taskPreview', () => {
  it('keeps the first meaningful line and collapses whitespace', () => {
    expect(taskPreview('Fix the tray\n\n  and   the menu ')).toBe('Fix the tray')
  })

  it('truncates with an ellipsis instead of growing the menu', () => {
    const preview = taskPreview('x'.repeat(120))
    expect(preview).toHaveLength(48)
    expect(preview.endsWith('…')).toBe(true)
  })
})

describe('menuBarStatusLabel', () => {
  it('leads with running workers, then with runs waiting for Codex', () => {
    expect(menuBarStatusLabel(view({ runningWorkers: 2, awaitingHost: 1 }))).toBe('Relay · 2 running')
    expect(menuBarStatusLabel(view({ awaitingHost: 2 }))).toBe('Relay · 2 awaiting Codex')
  })

  it('falls back to the environment state the window shows', () => {
    expect(menuBarStatusLabel(view())).toBe('Relay · Ready')
    expect(menuBarStatusLabel(view({ status: 'needsSetup' }))).toBe('Relay · Setup needed')
    expect(menuBarStatusLabel(view({ status: 'noRuntime' }))).toBe('Relay · No runtime detected')
  })

  it('follows the language the window reports', () => {
    expect(menuBarStatusLabel(view({ locale: 'zh-CN', runningWorkers: 3 }))).toBe('Relay · 3 个运行中')
  })
})

describe('buildMenuBarItems', () => {
  it('opens with the state line and the way back into the window', () => {
    const items = buildMenuBarItems(view())
    expect(items[0]).toMatchObject({ kind: 'header', label: 'Relay · Ready', enabled: false })
    expect(items[1]).toMatchObject({
      label: 'Open Relay',
      accelerator: 'CmdOrCtrl+O',
      action: { type: 'open-panel' },
    })
  })

  it('lists active workers with per-worker actions and a bulk stop only when needed', () => {
    const items = buildMenuBarItems(
      view({
        runningWorkers: 2,
        sessions: [
          {
            id: 'codex:one',
            displayName: 'Refine the menu bar',
            cwd: '/tmp/relay',
            activeWorkers: [
              { workerSessionId: 'worker-1', runId: 'run-1', label: 'DeepSeek Code · Fix the tray' },
              { workerSessionId: 'worker-2', runId: 'run-2', label: 'Kimi Code · Review the diff' },
            ],
          },
        ],
      }),
    )
    const active = items.find((item) => item.label === 'Active delegations (2)')
    expect(active).toBeDefined()
    expect(active?.submenu?.map((item) => item.label)).toEqual([
      'Refine the menu bar — DeepSeek Code · Fix the tray',
      'Refine the menu bar — Kimi Code · Review the diff',
      'Stop all workers · Refine the menu bar',
    ])
    expect(active?.submenu?.[0]?.submenu).toEqual([
      {
        kind: 'normal',
        label: 'Show in Relay',
        action: { type: 'open-panel', hostSessionId: 'codex:one', runId: 'run-1' },
      },
      { kind: 'normal', label: 'Cancel worker', action: { type: 'cancel-worker', workerSessionId: 'worker-1' } },
    ])
  })

  it('omits the active section entirely when nothing is running', () => {
    const items = buildMenuBarItems(view())
    expect(items.some((item) => item.label.startsWith('Active delegations'))).toBe(false)
  })

  it('never grows past the configured caps', () => {
    const sessions = Array.from({ length: 12 }, (_, index) => ({
      id: `codex:${index}`,
      displayName: `Session ${index}`,
      cwd: '/tmp/relay',
      activeWorkers: [
        { workerSessionId: `worker-${index}`, runId: `run-${index}`, label: 'DeepSeek Code · work' },
      ],
    }))
    const items = buildMenuBarItems(view({ sessions, runningWorkers: 12, maxSessions: 8, maxWorkers: 12 }))
    const sessionMenu = items.find((item) => item.label === 'Sessions')
    expect(sessionMenu?.submenu).toHaveLength(8)
    const active = items.find((item) => item.label.startsWith('Active delegations'))
    expect(active?.submenu).toHaveLength(12)
  })

  it('shows placeholders instead of empty submenus', () => {
    const items = buildMenuBarItems(view())
    expect(items.find((item) => item.label === 'Sessions')?.submenu).toEqual([
      { kind: 'header', label: 'No sessions yet', enabled: false },
    ])
    expect(items.find((item) => item.label === 'Agents')?.submenu).toEqual([
      { kind: 'header', label: 'No agents configured', enabled: false },
    ])
  })

  it('marks agents that cannot run yet and disables only those', () => {
    const items = buildMenuBarItems(
      view({
        agents: [
          { id: 'a', name: 'DeepSeek Code' },
          { id: 'b', name: 'Kimi Code', blocked: 'auth' },
          { id: 'c', name: 'GLM Code', blocked: 'missing' },
          { id: 'd', name: 'Gemini Code', blocked: 'disabled' },
        ],
      }),
    )
    const agents = items.find((item) => item.label === 'Agents')?.submenu ?? []
    expect(labels(agents)).toEqual([
      'normal:DeepSeek Code',
      'normal:Kimi Code · Authentication required',
      'normal:GLM Code · Not installed',
      'normal:Gemini Code · disabled',
    ])
    expect(agents.map((item) => item.enabled)).toEqual([true, false, false, false])
    expect(agents[0]?.action).toEqual({ type: 'open-settings', section: 'agents' })
  })

  it('reports the three Codex checks and offers the install action', () => {
    const items = buildMenuBarItems(
      view({
        codex: {
          configured: false,
          checks: [
            { id: 'codex-cli', ok: true, detail: 'codex 1.0.0' },
            { id: 'relay-mcp', ok: false, detail: '/tmp/.codex/config.toml' },
            { id: 'unexpected', ok: false, detail: 'raw' },
          ],
        },
      }),
    )
    const codex = items.find((item) => item.label === 'Codex integration')
    expect(codex?.submenu?.map((item) => item.label)).toEqual([
      'Not configured',
      '✓ Codex detected',
      '✗ Relay MCP configured — /tmp/.codex/config.toml',
      '✗ unexpected — raw',
      '',
      'Install in Codex…',
    ])
    expect(codex?.submenu?.at(-1)?.action).toEqual({ type: 'install-codex' })
  })

  it('keeps Dock and login switches macOS-only, and puts quit last', () => {
    const darwin = buildMenuBarItems(view())
    expect(darwin.find((item) => item.label === 'Hide Dock icon')).toMatchObject({
      kind: 'checkbox',
      checked: false,
      action: { type: 'toggle-hide-dock' },
    })
    expect(darwin.find((item) => item.label === 'Launch at login')).toMatchObject({ kind: 'checkbox' })
    expect(darwin.at(-1)).toMatchObject({ label: 'Quit Relay', accelerator: 'CmdOrCtrl+Q' })

    const linux = buildMenuBarItems(view({ platform: 'linux' }))
    expect(linux.some((item) => item.label === 'Hide Dock icon')).toBe(false)
    expect(linux.some((item) => item.label === 'Launch at login')).toBe(false)
  })

  it('translates every label for the reported locale', () => {
    const items = buildMenuBarItems(view({ locale: 'zh-CN', platform: 'linux' }))
    expect(items[1]?.label).toBe('打开 Relay')
    expect(items.some((item) => item.label === '会话')).toBe(true)
    expect(items.at(-1)?.label).toBe('退出 Relay')
  })
})

describe('menuBarViewFromSnapshot', () => {
  it('counts only workers that are still running and resolves their profile and task', () => {
    const projected = menuBarViewFromSnapshot({
      snapshot: snapshot({
        runs: [
          {
            run: run(),
            steps: [step({ task: 'Fix the tray icon' })],
            workers: [worker(), worker({ id: 'worker-2', status: 'completed' })],
            events: [],
          },
          {
            run: run({ id: 'run-2', status: 'awaiting_host', task: 'Review the diff' }),
            steps: [step({ id: 'step-2', runId: 'run-2', status: 'awaiting_host' })],
            workers: [worker({ id: 'worker-3', runId: 'run-2', stepId: 'step-2', status: 'completed' })],
            events: [],
          },
        ],
      }),
      codex: codexOk,
      prefs,
      locale: 'en',
      platform: 'darwin',
    })
    expect(projected.runningWorkers).toBe(1)
    expect(projected.awaitingHost).toBe(1)
    expect(projected.sessions[0]?.activeWorkers).toEqual([
      { workerSessionId: 'worker-1', runId: 'run-1', label: 'DeepSeek Code · Fix the tray icon' },
    ])
  })

  it('reports an agent as blocked when its runtime needs a sign-in', () => {
    const projected = menuBarViewFromSnapshot({
      snapshot: snapshot({ runtimes: [runtime({ health: 'authentication_required' })] }),
      codex: codexOk,
      prefs,
      locale: 'en',
      platform: 'darwin',
    })
    expect(projected.agents).toEqual([{ id: 'deepseek-code', name: 'DeepSeek Code', blocked: 'auth' }])
    expect(projected.status).toBe('needsSetup')
  })

  it('carries the settings switches straight through', () => {
    const projected = menuBarViewFromSnapshot({
      snapshot: snapshot(),
      codex: codexOk,
      prefs: { hideDockIcon: true, launchAtLogin: true },
      locale: 'en',
      platform: 'darwin',
    })
    expect(projected.prefs).toEqual({ hideDockIcon: true, launchAtLogin: true })
  })
})
