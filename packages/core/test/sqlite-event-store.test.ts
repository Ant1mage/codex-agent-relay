import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { projectRun, RunController, SqliteEventStore } from '../src/index.js'
import { FakeAdapter } from './fake-adapter.js'
import type { AgentProfile, Runtime } from '@relay/protocol'

const temporaryDirectories: string[] = []

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true })
  }
})

describe('SqliteEventStore', () => {
  it('restores an awaiting-host run projection after reopening the database', async () => {
    const directory = mkdtempSync(join(tmpdir(), 'relay-core-'))
    temporaryDirectories.push(directory)
    const path = join(directory, 'relay.sqlite')
    const runtime: Runtime = {
      id: 'runtime:fake',
      adapterId: 'fake',
      executablePath: '/usr/bin/true',
      health: 'available',
      capabilities: new FakeAdapter().capabilities(),
    }
    const profile: AgentProfile = {
      id: 'profile:fake',
      name: 'Fake',
      runtimeId: runtime.id,
      description: 'Persistence fixture',
      capabilities: {
        readWorkspace: true,
        writeWorkspace: false,
        executeCommands: false,
        networkAccess: false,
      },
      enabled: true,
    }

    const firstStore = new SqliteEventStore(path)
    const controller = new RunController(firstStore)
    controller.registerAdapter(new FakeAdapter())
    controller.registerRuntime(runtime)
    controller.registerProfile(profile)
    const active = await controller.start({
      hostSessionId: 'codex:persistence',
      profileId: profile.id,
      task: 'Persist this run',
      cwd: directory,
      accessMode: 'read_only',
      isolation: 'shared',
    })
    await active.completion
    firstStore.close()

    const reopenedStore = new SqliteEventStore(path)
    expect(reopenedStore.listRunIds()).toEqual([active.run.id])
    const projection = projectRun(reopenedStore.list(active.run.id))
    expect(projection.run.status).toBe('awaiting_host')
    expect(projection.steps).toHaveLength(1)
    expect(projection.workers).toHaveLength(1)
    expect(projection.workers[0]?.status).toBe('completed')
    expect(projection.workers[0]?.stepId).toBe(projection.steps[0]?.id)
    reopenedStore.close()
  })
})
