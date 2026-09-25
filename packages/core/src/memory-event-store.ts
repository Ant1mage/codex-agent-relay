import { RelayError, relayEventSchema, type RelayEvent } from '@relay/protocol'

export interface EventStore {
  append(event: RelayEvent): void | Promise<void>
  list(runId: string): RelayEvent[] | Promise<RelayEvent[]>
  findRunIdByWorker(workerSessionId: string): string | undefined | Promise<string | undefined>
}

export class MemoryEventStore implements EventStore {
  readonly #events = new Map<string, RelayEvent[]>()

  append(input: RelayEvent): void {
    const event = structuredClone(relayEventSchema.parse(input))
    const events = this.#events.get(event.runId) ?? []
    const expected = events.length + 1
    if (event.seq !== expected) {
      throw new RelayError(
        'EVENT_SEQUENCE_CONFLICT',
        `Expected sequence ${expected} for run ${event.runId}, received ${event.seq}`,
      )
    }
    events.push(event)
    this.#events.set(event.runId, events)
  }

  list(runId: string): RelayEvent[] {
    return [...(this.#events.get(runId) ?? [])]
  }

  findRunIdByWorker(workerSessionId: string): string | undefined {
    for (const [runId, events] of this.#events) {
      if (events.some((event) => event.workerSessionId === workerSessionId)) return runId
    }
    return undefined
  }
}
