import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { createInterface } from 'node:readline'
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
  withModelFallback,
  selectionOf,
  withSelectionArgs,
} from '@relay/adapter-sdk'
import type { AdapterCapabilities, RuntimeOptions, StartInput } from '@relay/protocol'
import { parseKimiLine } from './parser.js'

export interface KimiAdapterOptions {
  executablePath?: string
  prefixArgs?: string[]
  environment?: NodeJS.ProcessEnv
}

export class KimiAdapter implements AgentAdapter {
  readonly id = 'kimi-code'
  readonly #configuredExecutable: string | undefined
  readonly #prefixArgs: string[]
  readonly #environment: NodeJS.ProcessEnv | undefined
  #options: RuntimeOptions | undefined
  readonly #processes = new Map<string, ChildProcess>()
  #disposed = false

  constructor(options: KimiAdapterOptions = {}) {
    this.#configuredExecutable = options.executablePath
    this.#prefixArgs = options.prefixArgs ?? []
    this.#environment = options.environment
  }

  capabilities(): AdapterCapabilities {
    return {
      nonInteractive: true,
      structuredEvents: true,
      cwd: true,
      resume: false,
      send: false,
      cancel: true,
      childSessions: false,
    }
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
    return withModelFallback(cli, 'kimi')
  }

  /** Capabilities with model support set from what the CLI advertises. */
  #probedCapabilities(executablePath: string): AdapterCapabilities {
    const evidence = readHelp(executablePath, this.#prefixArgs, this.#environment)
    return probeRuntimeOptions(this.capabilities(), evidence, 'runtime:kimi-code', this.id).capabilities
  }

  #executable(): string | undefined {
    return this.#configuredExecutable ?? discoverExecutable('kimi', [
      join(homedir(), '.local', 'bin', 'kimi'),
      join(homedir(), '.kimi-code', 'bin', 'kimi'),
    ])
  }

  async detect(): Promise<DetectionResult> {
    const executablePath = this.#executable()
    if (!executablePath || !canExecute(executablePath)) {
      return { runtimes: [], diagnostics: ['Kimi Code executable `kimi` was not found'] }
    }
    const result = spawnSync(executablePath, [...this.#prefixArgs, '--version'], {
      encoding: 'utf8',
      timeout: 5_000,
      env: { ...process.env, ...this.#environment },
    })
    const version = result.status === 0 ? result.stdout.trim() || result.stderr.trim() : undefined
    return {
      runtimes: [{
        id: 'runtime:kimi-code',
        adapterId: this.id,
        executablePath,
        ...(version ? { version } : {}),
        health: 'available',
        capabilities: this.capabilities(),
      }],
      diagnostics: result.status === 0
        ? ['Authentication is validated by Kimi Code when a run starts']
        : [`Kimi Code version check failed: ${result.stderr.trim() || 'unknown error'}`],
    }
  }

  async start(input: StartInput): Promise<WorkerSessionHandle> {
    return this.#launch(input)
  }

  async #launch(input: StartInput): Promise<WorkerSessionHandle> {
    if (this.#disposed) throw new Error('Kimi adapter is disposed')
    const executablePath = input.executablePath ?? this.#executable()
    if (!executablePath) throw new Error('Kimi Code executable `kimi` was not found')
    // Probe the CLI once per launch so model/reasoning flags are only sent
    // when the CLI actually advertises them.
    this.#options = probeRuntimeOptions(
      this.capabilities(),
      readHelp(executablePath, this.#prefixArgs, this.#environment),
      'runtime:kimi-code',
      this.id,
    ).options
    const args = [
      ...this.#prefixArgs,
      '--prompt',
      input.task,
      '--output-format',
      'stream-json',
    ]
    const selected = withSelectionArgs(args, selectionOf(input), this.#options ?? {})
    const child = spawn(executablePath, selected, {
      cwd: input.cwd,
      env: { ...process.env, ...this.#environment, NO_COLOR: '1' },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    const queue = new AsyncEventQueue<AdapterEvent>()
    const syntheticSessionId = `kimi-process:${input.workerSessionId}`
    this.#processes.set(input.workerSessionId, child)
    this.#processes.set(syntheticSessionId, child)
    let nativeSessionId: string | undefined
    let finalText = ''
    let stderr = ''
    let emittedFailure = false

    child.stderr.setEncoding('utf8')
    child.stderr.on('data', (chunk: string) => {
      stderr = `${stderr}${chunk}`.slice(-32_000)
      if (chunk.trim()) {
        queue.push({
          type: 'worker/status',
          data: { message: 'Kimi Code wrote to stderr' },
          nativeEvent: { stream: 'stderr', text: chunk },
        })
      }
    })
    child.on('error', (error) => {
      emittedFailure = true
      queue.push({ type: 'worker/failed', data: { message: error.message } })
      queue.close()
    })
    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity })
    lines.on('line', (line) => {
      const parsed = parseKimiLine(line)
      if (parsed.sessionId && !nativeSessionId) {
        nativeSessionId = parsed.sessionId
        this.#processes.set(nativeSessionId, child)
      }
      if (parsed.finalText !== undefined) finalText = parsed.finalText
      parsed.events.forEach((event) => queue.push(event))
    })
    child.on('close', (code, signal) => {
      this.#processes.delete(input.workerSessionId)
      this.#processes.delete(syntheticSessionId)
      if (nativeSessionId) this.#processes.delete(nativeSessionId)
      if (!emittedFailure) {
        queue.push(code === 0
          ? { type: 'worker/completed', data: { summary: finalText, exitCode: code } }
          : {
              type: 'worker/failed',
              data: {
                message: stderr.trim() || 'Kimi Code exited unsuccessfully',
                exitCode: code,
                signal,
              },
            })
        queue.close()
      }
    })
    return {
      nativeSessionId: syntheticSessionId,
      ...(child.pid === undefined ? {} : { processId: child.pid }),
      events: queue,
    }
  }

  async cancel(sessionId: string): Promise<void> {
    const child = this.#processes.get(sessionId)
    if (child?.exitCode === null) child.kill('SIGTERM')
  }

  async dispose(): Promise<void> {
    this.#disposed = true
    const processes = new Set(this.#processes.values())
    this.#processes.clear()
    for (const child of processes) if (child.exitCode === null) child.kill('SIGTERM')
  }
}
