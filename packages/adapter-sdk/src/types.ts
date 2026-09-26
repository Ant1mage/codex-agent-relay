import type {
  AdapterCapabilities,
  RelayEventType,
  Runtime,
  RuntimeOptions,
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
  type: Exclude<
    RelayEventType,
    | 'run/created'
    | 'run/awaiting_host'
    | 'run/accepted'
    | 'step/created'
    | 'step/iteration_started'
    | 'worker/started'
  >
  data: unknown
  nativeEvent?: unknown
}

export interface ResumeInput {
  runId: string
  workerSessionId: string
  nativeSessionId: string
  task: string
  cwd: string
  accessMode: StartInput['accessMode']
  executablePath?: string
  instructions?: string
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
  /**
   * Model and reasoning choices as reported by the CLI itself. Optional so an
   * adapter can omit it; omitting means "this runtime exposes no choices", and
   * the UI then shows the CLI default instead of a picker.
   */
  reportOptions?(runtimeId: string): Promise<RuntimeOptions>
  start(input: StartInput): Promise<WorkerSessionHandle>
  send?(nativeSessionId: string, message: string): Promise<void>
  cancel(nativeSessionId: string): Promise<void>
  resume?(input: ResumeInput): Promise<WorkerSessionHandle>
}
