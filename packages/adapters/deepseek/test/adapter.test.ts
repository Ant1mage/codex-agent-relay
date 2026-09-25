import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { exerciseAdapter } from '@relay/adapter-sdk'
import { DeepSeekAdapter } from '../src/index.js'

const fixture = fileURLToPath(new URL('./fixtures/dsh-headless.mjs', import.meta.url))

function adapter(): DeepSeekAdapter {
  return new DeepSeekAdapter({ executablePath: process.execPath, prefixArgs: [fixture] })
}

describe('DeepSeekAdapter', () => {
  it('detects the executable and completes a headless JSON run', async () => {
    const instance = adapter()
    const detection = await instance.detect()
    expect(detection.runtimes[0]).toMatchObject({
      adapterId: 'deepseek-harness',
      version: 'dsh 0.0.0-fixture',
      health: 'available',
    })

    const report = await exerciseAdapter(instance, {
      runId: 'run:fixture',
      workerSessionId: 'worker:fixture',
      task: 'Complete the fixture',
      cwd: process.cwd(),
      accessMode: 'read_only',
    })
    expect(report.terminalEvent).toBe('worker/completed')
    expect(report.eventCount).toBeGreaterThan(5)
    await instance.dispose()
  })

  it('terminates an active Harness process', async () => {
    const instance = adapter()
    const handle = await instance.start({
      runId: 'run:cancel',
      workerSessionId: 'worker:cancel',
      task: 'HANG',
      cwd: process.cwd(),
      accessMode: 'read_only',
    })
    await instance.cancel(handle.nativeSessionId ?? 'worker:cancel')
    const events = []
    for await (const event of handle.events) events.push(event)
    expect(events.at(-1)?.type).toBe('worker/failed')
    await instance.dispose()
  })
})

