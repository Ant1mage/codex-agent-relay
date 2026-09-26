import { describe, expect, it } from 'vitest'
import type { MenuView } from '@relay/relay-api'
import { buildMenuBarItems, menuBarStatusLabel, menuBarTooltip, type MenuBarView } from '../src/menu-model.js'

const menu = (overrides: Partial<MenuView> = {}): MenuView => ({
  status: 'ready',
  runningWorkers: 0,
  awaitingHost: 0,
  sessions: [],
  agents: [],
  runtimes: [],
  codex: { checks: [], configured: true },
  ...overrides,
})

const view = (overrides: Partial<MenuBarView> = {}): MenuBarView => ({
  locale: 'en',
  platform: 'darwin',
  daemon: 'running',
  menu: menu(),
  launchAtLogin: false,
  browser: 'Microsoft Edge',
  ...overrides,
})

describe('menuBarStatusLabel', () => {
  it('leads with running workers, then runs waiting for Codex', () => {
    expect(menuBarStatusLabel(view({ menu: menu({ runningWorkers: 2, awaitingHost: 1 }) }))).toBe('Relay · 2 running')
    expect(menuBarStatusLabel(view({ menu: menu({ awaitingHost: 2 }) }))).toBe('Relay · 2 awaiting Codex')
  })

  it('says the log service is down before anything else', () => {
    expect(menuBarStatusLabel(view({ daemon: 'stopped', menu: undefined }))).toBe(
      'Relay · Log service is not running',
    )
    expect(menuBarStatusLabel(view({ daemon: 'starting', menu: undefined }))).toBe(
      'Relay · Starting the log service…',
    )
  })

  it('falls back to the environment state', () => {
    expect(menuBarStatusLabel(view())).toBe('Relay · Ready')
    expect(menuBarStatusLabel(view({ menu: menu({ status: 'needsSetup' }) }))).toBe('Relay · Setup needed')
  })

  it('follows the locale and feeds the tray tooltip', () => {
    expect(menuBarStatusLabel(view({ locale: 'zh-CN', menu: menu({ runningWorkers: 3 }) }))).toBe('Relay · 3 个运行中')
    expect(menuBarTooltip(view())).toBe('Relay · Ready')
  })
})

describe('buildMenuBarItems', () => {
  it('offers to start the daemon when it is not running', () => {
    const items = buildMenuBarItems(view({ daemon: 'stopped', menu: undefined }))
    expect(items.map((item) => item.label)).toEqual([
      'Relay · Log service is not running',
      'Start log service',
      'Restart log service',
      '',
      'Copy diagnostics',
      '',
      // App updates are offered even when the log service is down.
      'Check for updates…',
      '',
      'Quit Relay',
    ])
    expect(items[1]?.action).toEqual({ type: 'start-daemon' })
    expect(items.at(-1)).toMatchObject({ label: 'Quit Relay', accelerator: 'CmdOrCtrl+Q' })
  })

  it('opens the inspector and lists active workers with per-worker actions', () => {
    const items = buildMenuBarItems(
      view({
        menu: menu({
          runningWorkers: 2,
          sessions: [
            {
              id: 'codex:one',
              displayName: 'Refine the inspector',
              cwd: '/tmp/relay',
              activeWorkers: [
                { workerSessionId: 'w1', runId: 'run-1', label: 'DeepSeek Code · Wire the tray' },
                { workerSessionId: 'w2', runId: 'run-2', label: 'Kimi Code · Review the diff' },
              ],
            },
          ],
        }),
      }),
    )
    expect(items[1]).toMatchObject({ label: 'Open inspector', accelerator: 'CmdOrCtrl+O' })
    const active = items.find((item) => item.label === 'Active delegations (2)')
    expect(active?.submenu?.map((item) => item.label)).toEqual([
      'Refine the inspector — DeepSeek Code · Wire the tray',
      'Refine the inspector — Kimi Code · Review the diff',
      'Stop all workers · Refine the inspector',
    ])
    expect(active?.submenu?.[0]?.submenu).toEqual([
      {
        kind: 'normal',
        label: 'Show in inspector',
        action: { type: 'open-inspector', hostSessionId: 'codex:one', runId: 'run-1' },
      },
      { kind: 'normal', label: 'Cancel worker', action: { type: 'cancel-worker', workerSessionId: 'w1' } },
    ])
  })

  it('caps the menu and shows placeholders instead of empty submenus', () => {
    const sessions = Array.from({ length: 10 }, (_, index) => ({
      id: `codex:${index}`,
      displayName: `Session ${index}`,
      cwd: '/tmp/relay',
      activeWorkers: [],
    }))
    const items = buildMenuBarItems(view({ menu: menu({ sessions }), maxSessions: 4 }))
    expect(items.find((item) => item.label === 'Sessions')?.submenu).toHaveLength(4)
    const empty = buildMenuBarItems(view())
    expect(empty.find((item) => item.label === 'Sessions')?.submenu).toEqual([
      { kind: 'header', label: 'No sessions yet', enabled: false },
    ])
    // Even with nothing configured, the submenu keeps its way in to the editor.
    expect(empty.find((item) => item.label === 'Agents')?.submenu).toEqual([
      {
        kind: 'normal',
        label: 'New agent…',
        action: { type: 'open-panel', tab: 'agents', intent: 'new-agent' },
      },
      { kind: 'separator', label: '' },
      { kind: 'header', label: 'No agents configured', enabled: false },
    ])
  })

  it('marks blocked agents and keeps them out of reach', () => {
    const items = buildMenuBarItems(
      view({
        menu: menu({
          agents: [
            { id: 'a', name: 'DeepSeek Code' },
            { id: 'b', name: 'Kimi Code', blocked: 'auth' },
            { id: 'c', name: 'GLM Code', blocked: 'missing' },
            { id: 'd', name: 'Gemini Code', blocked: 'disabled' },
          ],
        }),
      }),
    )
    const agents = items.find((item) => item.label === 'Agents')?.submenu ?? []
    // The submenu opens with the create entry, then lists every profile.
    expect(agents[0]?.action).toEqual({ type: 'open-panel', tab: 'agents', intent: 'new-agent' })
    expect(agents.slice(2).map((item) => item.label)).toEqual([
      'DeepSeek Code',
      'Kimi Code · Authentication required',
      'GLM Code · Not installed',
      'Gemini Code · disabled',
    ])
    // A blocked agent is still reachable: the editor is where you fix it.
    expect(agents[2]?.action).toEqual({
      type: 'open-panel',
      tab: 'agents',
      intent: 'edit-agent',
      profileId: 'a',
    })
  })

  it('reports the Codex checks and offers the installer', () => {
    const items = buildMenuBarItems(
      view({
        menu: menu({
          codex: {
            configured: false,
            checks: [
              { id: 'codex-cli', ok: true, status: 'ok', detail: 'codex 1.0.0' },
              { id: 'relay-mcp', ok: false, status: 'stale', detail: '/tmp/.codex/config.toml' },
            ],
          },
        }),
      }),
    )
    const codex = items.find((item) => item.label === 'Codex integration')
    expect(codex?.submenu?.map((item) => item.label)).toEqual([
      'Not configured',
      '✓ Codex detected',
      '✗ Relay MCP configured — stale',
      '',
      'Codex integration settings…',
      'Repair integration',
    ])
  })

  it('keeps the login switch platform-bound and translates the whole menu', () => {
    expect(buildMenuBarItems(view()).some((item) => item.label === 'Launch at login')).toBe(true)
    expect(buildMenuBarItems(view({ platform: 'linux' })).some((item) => item.label === 'Launch at login')).toBe(false)
    const zh = buildMenuBarItems(view({ locale: 'zh-CN', platform: 'linux' }))
    expect(zh[1]?.label).toBe('打开检查器')
    expect(zh.at(-1)?.label).toBe('退出 Relay')
  })

  it('shows every updater state while the daemon is running', () => {
    const cases: Array<{
      update: NonNullable<MenuBarView['update']>
      labels: string[]
      action?: string
    }> = [
      { update: { status: 'checking' }, labels: ['Checking for updates…'] },
      {
        update: { status: 'available', version: '0.2.0' },
        labels: ['Update 0.2.0 available', 'Download update'],
        action: 'download-update',
      },
      { update: { status: 'downloading', percent: 42 }, labels: ['Download update 42%'] },
      {
        update: { status: 'downloaded', version: '0.2.0' },
        labels: ['Update 0.2.0 available', 'Restart and install'],
        action: 'install-update',
      },
      { update: { status: 'none' }, labels: ['Relay is up to date'] },
      { update: { status: 'error', message: 'feed unavailable' }, labels: ['⚠︎ feed unavailable'] },
    ]

    for (const entry of cases) {
      const items = buildMenuBarItems(view({ update: entry.update }))
      const labels = items.map((item) => item.label)
      for (const label of entry.labels) expect(labels).toContain(label)
      expect(labels).toContain('Check for updates…')
      if (entry.action) {
        expect(items.some((item) => item.action?.type === entry.action)).toBe(true)
      }
    }
  })
})
