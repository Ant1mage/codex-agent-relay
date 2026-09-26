import type { RelayEvent } from '@relay/protocol'
import type { RunView } from '@relay/relay-api'

/**
 * Events for one Step. Worker events belong to the step that owns their worker;
 * run-level events (created, awaiting Codex, accepted) have neither a step nor a
 * worker, so they stay visible on every step instead of disappearing.
 */
export function eventsForStep(view: RunView, stepId: string, events: RelayEvent[]): RelayEvent[] {
  const workerIds = new Set(
    view.workers.filter((worker) => worker.stepId === stepId).map((worker) => worker.id),
  )
  return events.filter((event) => {
    if (event.stepId) return event.stepId === stepId
    if (event.workerSessionId) return workerIds.has(event.workerSessionId)
    return true
  })
}
