import type { RunStatus, WorkerStatus } from './models.js'

const runTransitions: Readonly<Record<RunStatus, readonly RunStatus[]>> = {
  queued: ['starting', 'cancelled'],
  starting: ['running', 'failed', 'cancelled'],
  running: ['completed', 'failed', 'cancelled', 'handed_off'],
  completed: [],
  failed: [],
  cancelled: [],
  handed_off: [],
}

const workerTransitions: Readonly<Record<WorkerStatus, readonly WorkerStatus[]>> = {
  starting: ['running', 'failed', 'cancelled'],
  running: ['completed', 'failed', 'cancelled'],
  completed: [],
  failed: [],
  cancelled: [],
}

export function canTransitionRun(from: RunStatus, to: RunStatus): boolean {
  return runTransitions[from].includes(to)
}

export function canTransitionWorker(from: WorkerStatus, to: WorkerStatus): boolean {
  return workerTransitions[from].includes(to)
}

export function assertRunTransition(from: RunStatus, to: RunStatus): void {
  if (!canTransitionRun(from, to)) {
    throw new Error(`Invalid run transition: ${from} -> ${to}`)
  }
}

export function assertWorkerTransition(from: WorkerStatus, to: WorkerStatus): void {
  if (!canTransitionWorker(from, to)) {
    throw new Error(`Invalid worker transition: ${from} -> ${to}`)
  }
}

