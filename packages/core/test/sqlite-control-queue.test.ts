import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { SqliteControlQueue } from '../src/sqlite-control-queue.js'

describe('SqliteControlQueue', () => {
  it('claims and completes a cancellation request once', () => {
    const directory = mkdtempSync(join(tmpdir(), 'relay-control-'))
    const queue = new SqliteControlQueue(join(directory, 'relay.sqlite'))
    const created = queue.enqueueCancel('worker-1')

    expect(queue.claimNext()).toMatchObject({
      id: created.id,
      type: 'cancel-worker',
      workerSessionId: 'worker-1',
      status: 'processing',
    })
    expect(queue.claimNext()).toBeUndefined()

    queue.complete(created.id)
    expect(queue.get(created.id)?.status).toBe('completed')
    queue.close()
  })
})
