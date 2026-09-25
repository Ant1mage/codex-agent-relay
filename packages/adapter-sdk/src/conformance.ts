import { adapterCapabilitiesSchema, runtimeSchema, type StartInput } from '@relay/protocol'
import type { AgentAdapter } from './types.js'

export interface AdapterConformanceReport {
  adapterId: string
  detectedRuntimeCount: number
  eventCount: number
  terminalEvent: 'worker/completed' | 'worker/failed' | 'worker/cancelled'
}

export async function exerciseAdapter(
  adapter: AgentAdapter,
  input: StartInput,
): Promise<AdapterConformanceReport> {
  const detection = await adapter.detect()
  detection.runtimes.forEach((runtime) => runtimeSchema.parse(runtime))
  adapterCapabilitiesSchema.parse(adapter.capabilities())

  const handle = await adapter.start(input)
  let eventCount = 0
  let terminalEvent: AdapterConformanceReport['terminalEvent'] | undefined

  for await (const event of handle.events) {
    eventCount += 1
    if (
      event.type === 'worker/completed' ||
      event.type === 'worker/failed' ||
      event.type === 'worker/cancelled'
    ) {
      terminalEvent = event.type
    }
  }

  if (!terminalEvent) {
    throw new Error(`Adapter ${adapter.id} ended without a terminal event`)
  }

  return {
    adapterId: adapter.id,
    detectedRuntimeCount: detection.runtimes.length,
    eventCount,
    terminalEvent,
  }
}

