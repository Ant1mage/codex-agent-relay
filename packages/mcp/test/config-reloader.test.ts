import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import type { AgentAdapter, DetectionResult, WorkerSessionHandle } from '@relay/adapter-sdk'
import { RelayConfigStore } from '@relay/config'
import { MemoryEventStore, RunController } from '@relay/core'
import type { AdapterCapabilities, Runtime, StartInput } from '@relay/protocol'
import { RuntimeConfigReloader } from '../src/config-reloader.js'

const directories: string[] = []

class ChangingAdapter implements AgentAdapter {
  readonly id = 'changing'
  runtimes: Runtime[] = []

  capabilities(): AdapterCapabilities {
    return {
      nonInteractive: true,
      structuredEvents: false,
      cwd: true,
      resume: false,
      send: false,
      cancel: false,
      childSessions: false,
    }
  }

  async detect(): Promise<DetectionResult> {
    return { runtimes: this.runtimes, diagnostics: [] }
  }

  async start(_input: StartInput): Promise<WorkerSessionHandle> {
    throw new Error('not used')
  }

  async cancel(): Promise<void> {}
  dispose(): void {}
}

function runtime(id: string, health: Runtime['health'] = 'available'): Runtime {
  return {
    id,
    adapterId: 'changing',
    executablePath: process.execPath,
    health,
    capabilities: new ChangingAdapter().capabilities(),
  }
}

afterEach(() => {
  for (const directory of directories.splice(0)) rmSync(directory, { recursive: true, force: true })
})

describe('RuntimeConfigReloader', () => {
  it('synchronizes detected and manual runtimes across a long-lived MCP process', async () => {
    const directory = mkdtempSync(join(tmpdir(), 'relay-reloader-'))
    directories.push(directory)
    const executable = join(directory, 'manual-cli')
    writeFileSync(executable, '#!/bin/sh\nexit 0\n', 'utf8')
    chmodSync(executable, 0o755)
    const config = new RelayConfigStore({
      profiles: join(directory, 'profiles.json'),
      settings: join(directory, 'settings.json'),
      runtimes: join(directory, 'runtimes.json'),
    })
    config.writeManualRuntimes([{ id: 'manual', adapterId: 'changing', executablePath: executable }])
    const controller = new RunController(new MemoryEventStore())
    const adapter = new ChangingAdapter()
    adapter.runtimes = [runtime('detected')]
    const reloader = new RuntimeConfigReloader(controller, config, [adapter])

    await reloader.refresh()
    expect(controller.runtimes.list().map((item) => item.id)).toEqual(['detected', 'manual'])

    adapter.runtimes = [runtime('detected', 'authentication_required'), runtime('new-cli')]
    config.removeManualRuntime('manual')
    await reloader.refresh()
    expect(controller.runtimes.list().map((item) => [item.id, item.health])).toEqual([
      ['detected', 'authentication_required'],
      ['new-cli', 'available'],
    ])
    expect(() => controller.runtimes.require('manual')).toThrow(/Unknown runtime manual/)
  })
})
