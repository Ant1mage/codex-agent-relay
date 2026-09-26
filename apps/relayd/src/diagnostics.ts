import type { InspectorSnapshot } from '@relay/relay-api'

/**
 * "Copy diagnostics" is the one support action a window-less Relay can still
 * perform. The report is plain text so it can be pasted into an issue without a
 * viewer, and every line comes from the same projection the inspector shows.
 */
export interface DiagnosticsInput {
  appVersion: string
  nodeVersion: string
  platform: string
  arch: string
  port: number
  databasePath: string
  snapshot: InspectorSnapshot
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
  const { snapshot } = input
  const runs = snapshot.sessions.flatMap((session) => session.runs)
  const workers = runs.flatMap((view) => view.workers)
  const activeWorkers = workers.filter(
    (worker) => worker.status === 'running' || worker.status === 'starting',
  )
  const health = countByStatus(snapshot.runtimes.map((runtime) => runtime.health))
  const lines: string[] = [
    'Relay diagnostics',
    `Generated: ${input.generatedAt ?? new Date().toISOString()}`,
    `Relay ${input.appVersion} · Node ${input.nodeVersion} · ${input.platform} ${input.arch}`,
    `Daemon: http://127.0.0.1:${input.port}`,
    `Database: ${input.databasePath}`,
    '',
    'State',
    `Sessions: ${snapshot.sessions.length}`,
    `Runs: ${runs.length}${runs.length > 0 ? ` (${countByStatus(runs.map((view) => view.run.status))})` : ''}`,
    `Workers: ${workers.length} (${activeWorkers.length} active)`,
    `Runtimes: ${snapshot.runtimes.length}${health ? ` (${health})` : ''}`,
    `Profiles: ${snapshot.profiles.length} (${snapshot.profiles.filter((profile) => profile.enabled).length} enabled)`,
    '',
    'Codex integration',
    ...snapshot.codex.checks.map((check) => `${check.ok ? '✓' : '✗'} ${check.id} — ${check.detail}`),
    '',
    'Runtimes',
    ...(snapshot.runtimes.length === 0
      ? ['(none detected)']
      : snapshot.runtimes.map(
          (runtime) =>
            `- ${runtime.adapterId} · ${runtime.health} · ${runtime.executablePath}${runtime.version ? ` · ${runtime.version}` : ''}`,
        )),
  ]
  if (snapshot.diagnostics.length > 0) {
    lines.push('', 'Adapter diagnostics', ...snapshot.diagnostics.map((line) => `- ${line}`))
  }
  return `${lines.join('\n')}\n`
}
