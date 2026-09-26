import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import {
  AsyncEventQueue,
  canExecute,
  discoverExecutable,
  type AdapterEvent,
  type AgentAdapter,
  type DetectionResult,
  type WorkerSessionHandle,
} from '@relay/adapter-sdk'
import type { AdapterCapabilities, StartInput } from '@relay/protocol'
import { parseZaiOutput } from './parser.js'

export interface ZaiAdapterOptions {
  executablePath?: string
  prefixArgs?: string[]
  environment?: NodeJS.ProcessEnv
}

export class ZaiAdapter implements AgentAdapter {
  readonly id = 'zai-cli'
  readonly #configuredExecutable: string | undefined
  readonly #prefixArgs: string[]
  readonly #environment: NodeJS.ProcessEnv | undefined
  readonly #processes = new Map<string, ChildProcess>()
  #disposed = false

  constructor(options: ZaiAdapterOptions = {}) {
    this.#configuredExecutable = options.executablePath
    this.#prefixArgs = options.prefixArgs ?? []
    this.#environment = options.environment
  }

  capabilities(): AdapterCapabilities {
    return { nonInteractive: true, structuredEvents: true, cwd: true, resume: false, send: false, cancel: true, childSessions: false }
  }

  #executable(): string | undefined { return this.#configuredExecutable ?? discoverExecutable('zai-cli') }

  async detect(): Promise<DetectionResult> {
    const executablePath = this.#executable()
    if (!executablePath || !canExecute(executablePath)) return { runtimes: [], diagnostics: ['GLM / Z.ai CLI executable `zai-cli` was not found'] }
    const result = spawnSync(executablePath, [...this.#prefixArgs, '--version'], { encoding: 'utf8', timeout: 5_000, env: { ...process.env, ...this.#environment } })
    const version = result.status === 0 ? (result.stdout.trim() || result.stderr.trim()) : undefined
    return {
      runtimes: [{ id: 'runtime:zai-cli', adapterId: this.id, executablePath, ...(version ? { version } : {}), health: 'available', capabilities: this.capabilities() }],
      diagnostics: result.status === 0 ? ['Authentication is validated by GLM / Z.ai CLI when a run starts'] : [`GLM / Z.ai CLI version check failed: ${result.stderr.trim() || 'unknown error'}`],
    }
  }

  async start(input: StartInput): Promise<WorkerSessionHandle> {
    if (this.#disposed) throw new Error('GLM / Z.ai adapter is disposed')
    const executablePath = this.#executable()
    if (!executablePath) throw new Error('GLM / Z.ai CLI executable `zai-cli` was not found')
    const child = spawn(executablePath, [...this.#prefixArgs, 'chat', input.task, '--output', 'json', '--quiet'], {
      cwd: input.cwd, env: { ...process.env, ...this.#environment, NO_COLOR: '1' }, stdio: ['ignore', 'pipe', 'pipe'],
    })
    const queue = new AsyncEventQueue<AdapterEvent>()
    const nativeSessionId = `zai-process:${input.workerSessionId}`
    this.#processes.set(input.workerSessionId, child)
    this.#processes.set(nativeSessionId, child)
    let stdout = ''
    let stderr = ''
    let spawnedError: string | undefined
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    child.stdout.on('data', (chunk: string) => { stdout = `${stdout}${chunk}`.slice(-128 * 1024) })
    child.stderr.on('data', (chunk: string) => { stderr = `${stderr}${chunk}`.slice(-32_000); if (chunk.trim()) queue.push({ type: 'worker/status', data: { message: 'GLM / Z.ai CLI wrote to stderr' }, nativeEvent: { stream: 'stderr', text: chunk } }) })
    child.on('error', (error) => { spawnedError = error.message })
    child.on('close', (code, signal) => {
      this.#processes.delete(input.workerSessionId)
      this.#processes.delete(nativeSessionId)
      const parsed = parseZaiOutput(stdout)
      parsed.events.forEach((event) => queue.push(event))
      if (code === 0 && !spawnedError && !parsed.errorMessage) queue.push({ type: 'worker/completed', data: { summary: parsed.finalText ?? '', exitCode: code } })
      else queue.push({ type: 'worker/failed', data: { message: spawnedError ?? parsed.errorMessage ?? (stderr.trim() || 'GLM / Z.ai CLI exited unsuccessfully'), exitCode: code, signal } })
      queue.close()
    })
    return { nativeSessionId, ...(child.pid === undefined ? {} : { processId: child.pid }), events: queue }
  }

  async cancel(sessionId: string): Promise<void> { const child = this.#processes.get(sessionId); if (child?.exitCode === null) child.kill('SIGTERM') }
  async dispose(): Promise<void> { this.#disposed = true; const processes = new Set(this.#processes.values()); this.#processes.clear(); for (const child of processes) if (child.exitCode === null) child.kill('SIGTERM') }
}
