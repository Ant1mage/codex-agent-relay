import {
  runSchema,
  workerSessionSchema,
  type RelayEvent,
  type Run,
  type WorkerSession,
} from '@relay/protocol'

export interface RunProjection {
  run: Run
  workers: WorkerSession[]
  lastEvent?: RelayEvent
}

export function projectRun(events: RelayEvent[]): RunProjection {
  const first = events[0]
  if (!first || first.type !== 'run/created') throw new Error('Run projection requires run/created')
  const run = runSchema.parse((first.data as { run?: unknown }).run)
  const workers = new Map<string, WorkerSession>()

  for (const event of events.slice(1)) {
    if (event.type === 'worker/started') {
      const worker = workerSessionSchema.parse((event.data as { worker?: unknown }).worker)
      workers.set(worker.id, worker)
      run.status = 'running'
      continue
    }
    const worker = event.workerSessionId ? workers.get(event.workerSessionId) : undefined
    if (event.type === 'worker/completed') {
      run.status = 'completed'
      if (worker) {
        worker.status = 'completed'
        worker.endedAt = event.timestamp
      }
    } else if (event.type === 'worker/failed') {
      run.status = 'failed'
      if (worker) {
        worker.status = 'failed'
        worker.endedAt = event.timestamp
      }
    } else if (event.type === 'worker/cancelled') {
      run.status = 'cancelled'
      if (worker) {
        worker.status = 'cancelled'
        worker.endedAt = event.timestamp
      }
    }
  }

  const lastEvent = events.at(-1)
  return {
    run,
    workers: [...workers.values()],
    ...(lastEvent ? { lastEvent } : {}),
  }
}
