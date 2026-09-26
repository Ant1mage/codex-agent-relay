import type { CodexIntegrationStatus, DesktopSnapshot } from '../shared/api.js'

/**
 * The menu bar owns "copy diagnostics" because that is the one support action a
 * window-less Relay can still perform. The report is plain text so it can be
 * pasted into an issue without a viewer, and every line comes from the same
 * projection the UI shows (docs/mvp-roadmap.md Phase 6).
 */
export interface DiagnosticsInput {
  appVersion: string
  electronVersion: string
  chromeVersion: string
  nodeVersion: string
  platform: string
  arch: string
  databasePath: string
  settingsPath: string
  snapshot: DesktopSnapshot
  codex: CodexIntegrationStatus
  menuBar: { hideDockIcon: boolean; launchAtLogin: boolean }
  /** Injected so the report is reproducible in tests. */
  generatedAt?: string
}

function countByStatus(values: string[]): string {
  const counts = new Map<string, number>()
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1)
  return [...counts.entries()]
    .sort((left, right) => left[0].localeCompare(right[0]))
    .map(([status, count]) => `${status} ${count}`)
    .join(', ')
}

export function buildDiagnosticsReport(input: DiagnosticsInput): string {
  const { snapshot, codex } = input
  const workers = snapshot.runs.flatMap((view) => view.workers)
  const activeWorkers = workers.filter(
    (worker) => worker.status === 'running' || worker.status === 'starting',
  )
  const health = countByStatus(snapshot.runtimes.map((runtime) => runtime.health))
  const lines: string[] = [
    'Relay diagnostics',
    `Generated: ${input.generatedAt ?? new Date().toISOString()}`,
    `Relay ${input.appVersion} · Electron ${input.electronVersion} · Chromium ${input.chromeVersion} · Node ${input.nodeVersion}`,
    `Platform: ${input.platform} ${input.arch}`,
    '',
    'State',
    `Sessions: ${snapshot.sessions.length}`,
    `Runs: ${snapshot.runs.length}${snapshot.runs.length > 0 ? ` (${countByStatus(snapshot.runs.map((view) => view.run.status))})` : ''}`,
    `Workers: ${workers.length} (${activeWorkers.length} active)`,
    `Runtimes: ${snapshot.runtimes.length}${health ? ` (${health})` : ''}`,
    `Profiles: ${snapshot.profiles.length} (${snapshot.profiles.filter((profile) => profile.enabled).length} enabled)`,
    `Menu bar: hide dock icon ${input.menuBar.hideDockIcon ? 'on' : 'off'}, launch at login ${input.menuBar.launchAtLogin ? 'on' : 'off'}`,
    '',
    'Codex integration',
    ...codex.checks.map((check) => `${check.ok ? '✓' : '✗'} ${check.id} — ${check.detail}`),
    '',
    'Runtimes',
    ...(snapshot.runtimes.length === 0
      ? ['(none detected)']
      : snapshot.runtimes.map((runtime) =>
          `- ${runtime.adapterId} · ${runtime.health} · ${runtime.executablePath}${runtime.version ? ` · ${runtime.version}` : ''}`,
        )),
    '',
    'Storage',
    `Database: ${input.databasePath}`,
    `Settings: ${input.settingsPath}`,
  ]
  if (snapshot.diagnostics.length > 0) {
    lines.push('', 'Adapter diagnostics', ...snapshot.diagnostics.map((line) => `- ${line}`))
  }
  return `${lines.join('\n')}\n`
}
