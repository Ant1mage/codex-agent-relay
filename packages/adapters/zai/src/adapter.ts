import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import {
  AsyncEventQueue,
  canExecute,
  discoverExecutable,
  type AdapterEvent,
  type AgentAdapter,
  type DetectionResult,
  type WorkerSessionHandle,
  probeRuntimeOptions,
  readHelp,
  selectionOf,
  withSelectionArgs,
} from '@relay/adapter-sdk'
import type { AdapterCapabilities, RuntimeOptions, StartInput } from '@relay/protocol'
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
  #options: RuntimeOptions | undefined
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

  /**
   * Model and reasoning choices, read from the CLI's own --help. Relay reports
   * only what the CLI advertises; a CLI with no model flag yields no models and
   * a diagnostic, so the UI shows the CLI default rather than a guessed list.
   */
  async reportOptions(runtimeId: string): Promise<RuntimeOptions> {
    const executablePath = this.#executable()
    if (!executablePath) {
      return {
        runtimeId,
        adapterId: this.id,
        models: [],
        levels: [],
        source: 'default',
        diagnostics: ['Runtime executable was not found, so Relay cannot read its model options'],
      }
    }
    const evidence = readHelp(executablePath, this.#prefixArgs, this.#environment)
    return probeRuntimeOptions(this.capabilities(), evidence, runtimeId, this.id).options
  }

  /** Capabilities with model support set from what the CLI advertises. */
  #probedCapabilities(executablePath: string): AdapterCapabilities {
    const evidence = readHelp(executablePath, this.#prefixArgs, this.#environment)
    return probeRuntimeOptions(this.capabilities(), evidence, 'runtime:zai-cli', this.id).capabilities
  }

  #executable(): string | undefined { return this.#configuredExecutable ?? discoverExecutable('zai-cli') }

  async detect(): Promise<DetectionResult> {
    const executablePath = this.#executable()
    if (!executablePath || !canExecute(executablePath)) return { runtimes: [], diagnostics: ['GLM / Z.ai CLI executable `zai-cli` was not found'] }
    const result = spawnSync(executablePath, [...this.#prefixArgs, '--version'], { encoding: 'utf8', timeout: 5_000, env: { ...process.env, ...this.#environment } })
    const version = result.status === 0 ? (result.stdout.trim() || result.stderr.trim()) : undefined
    return {
      runtimes: [{ id: 'runtime:zai-cli', adapterId: this.id, executablePath, ...(version ? { version } : {}), health: 'available', capabilities: this.#probedCapabilities(executablePath) }],
      diagnostics: result.status === 0 ? ['Authentication is validated by GLM / Z.ai CLI when a run starts'] : [`GLM / Z.ai CLI version check failed: ${result.stderr.trim() || 'unknown error'}`],
    }
  }

  async start(input: StartInput): Promise<WorkerSessionHandle> {
    if (this.#disposed) throw new Error('GLM / Z.ai adapter is disposed')
    const executablePath = this.#executable()
    if (!executablePath) throw new Error('GLM / Z.ai CLI executable `zai-cli` was not found')
    // Probe the CLI once per launch so model/reasoning flags are only sent
    // when the CLI actually advertises them.
    this.#options = probeRuntimeOptions(
      this.capabilities(),
      readHelp(executablePath, this.#prefixArgs, this.#environment),
      'runtime:zai-cli',
      this.id,
    ).options
    const selected = withSelectionArgs(
      [...this.#prefixArgs, 'chat', input.task, '--output', 'json', '--quiet'],
      selectionOf(input),
      this.#options ?? {},
    )
    const child = spawn(executablePath, selected, {
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
