import { describe, expect, it } from 'vitest'
import type { InspectorSnapshot } from '@relay/relay-api'
import { buildDiagnosticsReport } from '../src/diagnostics.js'
import { profiles, runtimes, seedDatabase } from './fixtures.js'
import { RelayStore } from '../src/store.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

function snapshot(): InspectorSnapshot {
  const directory = mkdtempSync(join(tmpdir(), 'relay-diag-'))
  const store = new RelayStore(join(directory, 'relay.sqlite'))
  seedDatabase(join(directory, 'relay.sqlite'))
  store.setEnvironment({ runtimes, profiles })
  const value = store.snapshot({ runtimes, profiles, diagnostics: ['kimi: no executable found'] }, {
    checks: [{ id: 'relay-mcp', ok: false, status: 'stale', detail: '/tmp/.codex/config.toml' }],
    configured: false,
  })
  store.close()
  rmSync(directory, { recursive: true, force: true })
  return value
}

describe('buildDiagnosticsReport', () => {
  it('summarises what a support request needs', () => {
    const report = buildDiagnosticsReport({
      appVersion: '0.0.0',
      nodeVersion: '24.14.0',
      platform: 'darwin',
      arch: 'arm64',
      port: 7352,
      databasePath: '/tmp/relay.sqlite',
      snapshot: snapshot(),
      generatedAt: '2026-01-01T10:06:00.000Z',
    })
    expect(report).toContain('Generated: 2026-01-01T10:06:00.000Z')
    expect(report).toContain('Daemon: http://127.0.0.1:7352')
    expect(report).toContain('Sessions: 1')
    expect(report).toContain('Runs: 1 (running 1)')
    expect(report).toContain('Workers: 1 (1 active)')
    expect(report).toContain('Runtimes: 2 (authentication_required 1, available 1)')
    expect(report).toContain('✗ relay-mcp — /tmp/.codex/config.toml')
    expect(report).toContain('- kimi: no executable found')
  })
})
