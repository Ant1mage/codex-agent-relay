import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import { createInterface } from 'node:readline'
import {
  AsyncEventQueue,
  canExecute,
  discoverExecutable,
  type AdapterEvent,
  type AgentAdapter,
  type DetectionResult,
  type ResumeInput,
  type WorkerSessionHandle,
  probeRuntimeOptions,
  readHelp,
  withModelFallback,
  selectionOf,
  withSelectionArgs,
} from '@relay/adapter-sdk'
import type { AdapterCapabilities, RuntimeOptions, StartInput } from '@relay/protocol'
import { parseGeminiLine } from './parser.js'

export interface GeminiAdapterOptions {
  executablePath?: string
  prefixArgs?: string[]
  environment?: NodeJS.ProcessEnv
}

export class GeminiAdapter implements AgentAdapter {
  readonly id = 'gemini-cli'
  readonly #configuredExecutable: string | undefined
  readonly #prefixArgs: string[]
  readonly #environment: NodeJS.ProcessEnv | undefined
  #options: RuntimeOptions | undefined
  readonly #processes = new Map<string, ChildProcess>()
  #disposed = false

  constructor(options: GeminiAdapterOptions = {}) {
    this.#configuredExecutable = options.executablePath
    this.#prefixArgs = options.prefixArgs ?? []
    this.#environment = options.environment
  }

  capabilities(): AdapterCapabilities {
    return { nonInteractive: true, structuredEvents: true, cwd: true, resume: true, send: false, cancel: true, childSessions: false }
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
    const cli = probeRuntimeOptions(this.capabilities(), evidence, runtimeId, this.id).options
    // CLI first; fall back to the official API when the CLI exposes no list.
    return withModelFallback(cli, 'gemini')
  }

  /** Capabilities with model support set from what the CLI advertises. */
  #probedCapabilities(executablePath: string): AdapterCapabilities {
    const evidence = readHelp(executablePath, this.#prefixArgs, this.#environment)
    return probeRuntimeOptions(this.capabilities(), evidence, 'runtime:gemini-cli', this.id).capabilities
  }

  #executable(): string | undefined {
    return this.#configuredExecutable ?? discoverExecutable('gemini')
  }

  async detect(): Promise<DetectionResult> {
    const executablePath = this.#executable()
    if (!executablePath || !canExecute(executablePath)) return { runtimes: [], diagnostics: ['Gemini CLI executable `gemini` was not found'] }
    const result = spawnSync(executablePath, [...this.#prefixArgs, '--version'], { encoding: 'utf8', timeout: 5_000, env: { ...process.env, ...this.#environment } })
    const version = result.status === 0 ? (result.stdout.trim() || result.stderr.trim()) : undefined
    return {
      runtimes: [{ id: 'runtime:gemini-cli', adapterId: this.id, executablePath, ...(version ? { version } : {}), health: 'available', capabilities: this.#probedCapabilities(executablePath) }],
      diagnostics: result.status === 0 ? ['Authentication is validated by Gemini CLI when a run starts'] : [`Gemini CLI version check failed: ${result.stderr.trim() || 'unknown error'}`],
    }
  }

  async start(input: StartInput): Promise<WorkerSessionHandle> { return this.#launch(input) }
  async resume(input: ResumeInput): Promise<WorkerSessionHandle> { return this.#launch(input, input.nativeSessionId) }

  async #launch(input: StartInput | ResumeInput, resumeId?: string): Promise<WorkerSessionHandle> {
    if (this.#disposed) throw new Error('Gemini adapter is disposed')
    const executablePath = input.executablePath ?? this.#executable()
    if (!executablePath) throw new Error('Gemini CLI executable `gemini` was not found')
    // Probe the CLI once per launch so model/reasoning flags are only sent
    // when the CLI actually advertises them.
    this.#options = probeRuntimeOptions(
      this.capabilities(),
      readHelp(executablePath, this.#prefixArgs, this.#environment),
      'runtime:gemini-cli',
      this.id,
    ).options
    const selected = withSelectionArgs(
      [...this.#prefixArgs, '-p', input.task, '--output-format', 'stream-json', ...(resumeId ? ['--resume', resumeId] : [])],
      selectionOf(input),
      this.#options ?? {},
    )
    const child = spawn(executablePath, selected, {
      cwd: input.cwd, env: { ...process.env, ...this.#environment, NO_COLOR: '1' }, stdio: ['ignore', 'pipe', 'pipe'],
    })
    const queue = new AsyncEventQueue<AdapterEvent>()
    this.#processes.set(input.workerSessionId, child)
    let nativeSessionId = resumeId
    let finalText = ''
    let errorMessage: string | undefined
    let stderr = ''
    let settled = false
    let resolveReady!: (id: string) => void
    let rejectReady!: (error: Error) => void
    const ready = new Promise<string>((resolve, reject) => { resolveReady = resolve; rejectReady = reject })
    if (resumeId) { settled = true; resolveReady(resumeId) }

    child.stderr.setEncoding('utf8')
    child.stderr.on('data', (chunk: string) => {
      stderr = `${stderr}${chunk}`.slice(-32_000)
      if (chunk.trim()) queue.push({ type: 'worker/status', data: { message: 'Gemini CLI wrote to stderr' }, nativeEvent: { stream: 'stderr', text: chunk } })
    })
    child.on('error', (error) => { if (!settled) { settled = true; rejectReady(error) } })
    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity })
    lines.on('line', (line) => {
      const parsed = parseGeminiLine(line)
      if (parsed.sessionId && !nativeSessionId) { nativeSessionId = parsed.sessionId; this.#processes.set(nativeSessionId, child); if (!settled) { settled = true; resolveReady(nativeSessionId) } }
      if (parsed.finalText !== undefined) finalText = parsed.finalText
      if (parsed.errorMessage !== undefined) errorMessage = parsed.errorMessage
      parsed.events.forEach((event) => queue.push(event))
    })
    child.on('close', (code, signal) => {
      this.#processes.delete(input.workerSessionId)
      if (nativeSessionId) this.#processes.delete(nativeSessionId)
      if (code === 0 && !errorMessage) queue.push({ type: 'worker/completed', data: { summary: finalText, exitCode: code } })
      else queue.push({ type: 'worker/failed', data: { message: errorMessage ?? (stderr.trim() || 'Gemini CLI exited unsuccessfully'), exitCode: code, signal } })
      queue.close()
      if (!settled) { settled = true; rejectReady(new Error(errorMessage ?? (stderr.trim() || 'Gemini CLI exited before initialization'))) }
    })
    const sessionId = await ready
    return { nativeSessionId: sessionId, ...(child.pid === undefined ? {} : { processId: child.pid }), events: queue }
  }

  async cancel(sessionId: string): Promise<void> { const child = this.#processes.get(sessionId); if (child?.exitCode === null) child.kill('SIGTERM') }
  async dispose(): Promise<void> { this.#disposed = true; const processes = new Set(this.#processes.values()); this.#processes.clear(); for (const child of processes) if (child.exitCode === null) child.kill('SIGTERM') }
}
