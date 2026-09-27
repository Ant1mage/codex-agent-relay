import {
  runSchema,
  stepSchema,
  workerSessionSchema,
  type RelayEvent,
  type Run,
  type Step,
  type WorkerSession,
} from '@relay/protocol'

export interface RunProjection {
  run: Run
  steps: Step[]
  workers: WorkerSession[]
  /** Data carried by the latest terminal worker event, for host review. */
  result?: unknown
  lastEvent?: RelayEvent
}

export function projectRun(events: RelayEvent[]): RunProjection {
  const first = events[0]
  if (!first || first.type !== 'run/created') throw new Error('Run projection requires run/created')
  const rawRun = (first.data as { run?: Record<string, unknown> }).run
  const run = runSchema.parse({ ...rawRun, updatedAt: rawRun?.updatedAt ?? rawRun?.createdAt })
  const steps = new Map<string, Step>()
  const workers = new Map<string, WorkerSession>()
  const usesStepLifecycle = events.some((event) => event.type === 'step/created')

  const legacyStep = (): Step => {
    const existing = steps.values().next().value as Step | undefined
    if (existing) return existing
    const step = stepSchema.parse({
      id: `legacy-step:${run.id}`,
      runId: run.id,
      profileId: run.profileId,
      task: run.task,
      accessMode: run.accessMode,
      isolation: run.isolation,
      status: run.status === 'completed' ? 'completed' : run.status,
      iteration: 1,
      createdAt: run.createdAt,
      updatedAt: run.updatedAt,
    })
    steps.set(step.id, step)
    return step
  }

  for (const event of events.slice(1)) {
    run.updatedAt = event.timestamp
    if (event.type === 'step/created') {
      const step = stepSchema.parse((event.data as { step?: unknown }).step)
      steps.set(step.id, step)
      continue
    }
    if (event.type === 'step/iteration_started') {
      const step = stepSchema.parse((event.data as { step?: unknown }).step)
      steps.set(step.id, step)
      run.status = 'starting'
      continue
    }
    if (event.type === 'worker/started') {
      const rawWorker = (event.data as { worker?: Record<string, unknown> }).worker
      const step = event.stepId ? steps.get(event.stepId) : legacyStep()
      const worker = workerSessionSchema.parse({
        ...rawWorker,
        stepId: rawWorker?.stepId ?? step?.id ?? legacyStep().id,
        iteration: rawWorker?.iteration ?? step?.iteration ?? 1,
      })
      workers.set(worker.id, worker)
      run.status = 'running'
      if (step) {
        step.status = 'running'
        step.updatedAt = event.timestamp
      }
      continue
    }
    const worker = event.workerSessionId ? workers.get(event.workerSessionId) : undefined
    const step = event.stepId
      ? steps.get(event.stepId)
      : worker
        ? steps.get(worker.stepId)
        : undefined
    if (event.type === 'worker/completed') {
      run.status = usesStepLifecycle ? 'awaiting_host' : 'completed'
      if (worker) {
        worker.status = 'completed'
        worker.endedAt = event.timestamp
      }
      if (step) {
        step.status = usesStepLifecycle ? 'awaiting_host' : 'completed'
        step.updatedAt = event.timestamp
      }
    } else if (event.type === 'worker/failed') {
      run.status = 'failed'
      if (worker) {
        worker.status = 'failed'
        worker.endedAt = event.timestamp
      }
      if (step) {
        step.status = 'failed'
        step.updatedAt = event.timestamp
      }
    } else if (event.type === 'worker/cancelled') {
      run.status = 'cancelled'
      if (worker) {
        worker.status = 'cancelled'
        worker.endedAt = event.timestamp
      }
      if (step) {
        step.status = 'cancelled'
        step.updatedAt = event.timestamp
      }
    } else if (event.type === 'worker/interrupted') {
      run.status = 'interrupted'
      if (worker) worker.status = 'interrupted'
      if (step) step.status = 'interrupted'
    } else if (event.type === 'worker/orphaned') {
      run.status = 'orphaned'
      if (worker) worker.status = 'orphaned'
      if (step) step.status = 'orphaned'
    } else if (event.type === 'run/awaiting_host') {
      run.status = 'awaiting_host'
    } else if (event.type === 'run/accepted') {
      run.status = 'completed'
      for (const candidate of steps.values()) {
        if (candidate.status === 'awaiting_host') {
          candidate.status = 'completed'
          candidate.updatedAt = event.timestamp
        }
      }
    }
  }

  if (steps.size === 0) legacyStep()

  const lastEvent = events.at(-1)
  const terminalEvent = events.findLast((event) =>
    event.type === 'worker/completed' ||
    event.type === 'worker/failed' ||
    event.type === 'worker/cancelled',
  )
  return {
    run,
    steps: [...steps.values()],
    workers: [...workers.values()],
    ...(terminalEvent ? { result: terminalEvent.data } : {}),
    ...(lastEvent ? { lastEvent } : {}),
  }
}
