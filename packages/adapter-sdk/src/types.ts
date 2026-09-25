import type {
  AdapterCapabilities,
  RelayEventType,
  Runtime,
  StartInput,
} from '@relay/protocol'

export interface Disposable {
  dispose(): void | Promise<void>
}

export interface DetectionResult {
  runtimes: Runtime[]
  diagnostics: string[]
}

export interface AdapterEvent {
  type: Exclude<RelayEventType, 'run/created' | 'worker/started'>
  data: unknown
  nativeEvent?: unknown
}

export interface ResumeInput {
  runId: string
  workerSessionId: string
  nativeSessionId: string
  message?: string
}

export interface WorkerSessionHandle {
  nativeSessionId?: string
  processId?: number
  events: AsyncIterable<AdapterEvent>
}

export interface AgentAdapter extends Disposable {
  readonly id: string
  detect(): Promise<DetectionResult>
  capabilities(): AdapterCapabilities
  start(input: StartInput): Promise<WorkerSessionHandle>
  send?(nativeSessionId: string, message: string): Promise<void>
  cancel(nativeSessionId: string): Promise<void>
  resume?(input: ResumeInput): Promise<WorkerSessionHandle>
}

