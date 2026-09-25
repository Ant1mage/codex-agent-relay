import { accessSync, constants } from 'node:fs'
import { delimiter, join } from 'node:path'
import {
  spawn,
  spawnSync,
  type ChildProcess,
  type ChildProcessWithoutNullStreams,
} from 'node:child_process'
import { createInterface } from 'node:readline'
import type {
  AdapterEvent,
  AgentAdapter,
  DetectionResult,
  ResumeInput,
  WorkerSessionHandle,
} from '@relay/adapter-sdk'
import type { AdapterCapabilities, StartInput } from '@relay/protocol'
import { parseDeepSeekLine } from './parser.js'

class AsyncEventQueue implements AsyncIterable<AdapterEvent> {
  readonly #items: AdapterEvent[] = []
  readonly #waiters: Array<(result: IteratorResult<AdapterEvent>) => void> = []
  #closed = false

  push(event: AdapterEvent): void {
    if (this.#closed) return
    const waiter = this.#waiters.shift()
    if (waiter) waiter({ value: event, done: false })
    else this.#items.push(event)
  }

  close(): void {
    if (this.#closed) return
    this.#closed = true
    for (const waiter of this.#waiters.splice(0)) waiter({ value: undefined, done: true })
  }

  [Symbol.asyncIterator](): AsyncIterator<AdapterEvent> {
    return {
      next: async () => {
        const item = this.#items.shift()
        if (item) return { value: item, done: false }
        if (this.#closed) return { value: undefined, done: true }
        return new Promise<IteratorResult<AdapterEvent>>((resolve) => this.#waiters.push(resolve))
      },
    }
  }
}

export interface DeepSeekAdapterOptions {
  executablePath?: string
  prefixArgs?: string[]
  environment?: NodeJS.ProcessEnv
}

function canExecute(candidate: string): boolean {
  try {
    accessSync(candidate, constants.X_OK)
    return true
  } catch {
    return false
  }
}

function discoverExecutable(name: string): string | undefined {
  const directories = (process.env.PATH ?? '').split(delimiter).filter(Boolean)
  const extensions = process.platform === 'win32'
    ? (process.env.PATHEXT ?? '.EXE;.CMD;.BAT').split(';')
    : ['']
  for (const directory of directories) {
    for (const extension of extensions) {
      const candidate = join(directory, `${name}${extension}`)
      if (canExecute(candidate)) return candidate
    }
  }
  return undefined
}

export class DeepSeekAdapter implements AgentAdapter {
  readonly id = 'deepseek-harness'
  readonly #configuredExecutable: string | undefined
  readonly #prefixArgs: string[]
  readonly #environment: NodeJS.ProcessEnv | undefined
  readonly #processes = new Map<string, ChildProcess>()
  #features = { json: true, resume: true }
  #disposed = false

  constructor(options: DeepSeekAdapterOptions = {}) {
    this.#configuredExecutable = options.executablePath
    this.#prefixArgs = options.prefixArgs ?? []
    this.#environment = options.environment
  }

  capabilities(): AdapterCapabilities {
    return {
      nonInteractive: true,
      structuredEvents: this.#features.json,
      cwd: true,
      resume: this.#features.resume,
      send: false,
      cancel: true,
      childSessions: false,
    }
  }

  async detect(): Promise<DetectionResult> {
    const executablePath = this.#configuredExecutable ?? discoverExecutable('dsh')
    if (!executablePath || !canExecute(executablePath)) {
      return { runtimes: [], diagnostics: ['DeepSeek Harness executable `dsh` was not found'] }
    }
    const result = spawnSync(executablePath, [...this.#prefixArgs, '--version'], {
      encoding: 'utf8',
      timeout: 5_000,
      env: { ...process.env, ...this.#environment },
    })
    this.#features = this.#probeFeatures(executablePath)
    const version = result.status === 0 ? result.stdout.trim() || result.stderr.trim() : undefined
    return {
      runtimes: [
        {
          id: 'runtime:deepseek-harness',
          adapterId: this.id,
          executablePath,
          ...(version ? { version } : {}),
          health: 'available',
          capabilities: this.capabilities(),
        },
      ],
      diagnostics:
        result.status === 0
          ? [
              'Authentication is validated by DeepSeek Harness when a run starts',
              ...(this.#features.json
                ? []
                : ['This dsh version has no --json stream; Relay will use bounded plain-text mode']),
            ]
          : [`DeepSeek Harness version check failed: ${result.stderr.trim() || 'unknown error'}`],
    }
  }

  async start(input: StartInput): Promise<WorkerSessionHandle> {
    return this.#launch(input)
  }

  async resume(input: ResumeInput): Promise<WorkerSessionHandle> {
    return this.#launch(input, input.nativeSessionId)
  }

  async #launch(input: StartInput | ResumeInput, resumeSessionId?: string): Promise<WorkerSessionHandle> {
    if (this.#disposed) throw new Error('DeepSeek adapter is disposed')
    const executablePath = this.#configuredExecutable ?? discoverExecutable('dsh')
    if (!executablePath) throw new Error('DeepSeek Harness executable `dsh` was not found')
    this.#features = this.#probeFeatures(executablePath)
    if (!this.#features.json) return this.#launchPlain(executablePath, input, resumeSessionId)

    const args = [
      ...this.#prefixArgs,
      '--profile',
      'headless',
      '--json',
      ...(resumeSessionId ? ['--session-id', resumeSessionId] : []),
    ]
    const child = spawn(executablePath, args, {
      cwd: input.cwd,
      env: { ...process.env, ...this.#environment, NO_COLOR: '1' },
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    this.#processes.set(input.workerSessionId, child)
    const queue = new AsyncEventQueue()
    let stderr = ''
    let finalText: string | undefined
    let errorMessage: string | undefined
    let turnEndKind: string | undefined
    let sessionId: string | undefined
    let readySettled = false
    let resolveReady!: (id: string) => void
    let rejectReady!: (error: Error) => void
    const ready = new Promise<string>((resolve, reject) => {
      resolveReady = resolve
      rejectReady = reject
    })

    child.stderr.setEncoding('utf8')
    child.stderr.on('data', (chunk: string) => {
      stderr = `${stderr}${chunk}`.slice(-8_192)
    })
    child.on('error', (error) => {
      if (!readySettled) {
        readySettled = true
        rejectReady(error)
      }
    })

    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity })
    lines.on('line', (line) => {
      const parsed = parseDeepSeekLine(line)
      if (parsed.sessionId && !sessionId) {
        sessionId = parsed.sessionId
        this.#processes.set(sessionId, child)
        if (!readySettled) {
          readySettled = true
          resolveReady(sessionId)
        }
      }
      if (parsed.finalText !== undefined) finalText = parsed.finalText
      if (parsed.errorMessage !== undefined) errorMessage = parsed.errorMessage
      if (parsed.turnEndKind !== undefined) turnEndKind = parsed.turnEndKind
      parsed.events.forEach((event) => queue.push(event))
    })

    child.on('close', (code, signal) => {
      this.#processes.delete(input.workerSessionId)
      if (sessionId) this.#processes.delete(sessionId)
      const completed = code === 0 && (!turnEndKind || turnEndKind === 'completed') && !errorMessage
      queue.push(
        completed
          ? { type: 'worker/completed', data: { summary: finalText ?? '', exitCode: code } }
          : {
              type: 'worker/failed',
              data: {
                message: errorMessage ?? (stderr.trim() || 'DeepSeek Harness exited unsuccessfully'),
                exitCode: code,
                signal,
                turnEndKind,
              },
            },
      )
      queue.close()
      if (!readySettled) {
        readySettled = true
        rejectReady(
          new Error(errorMessage ?? (stderr.trim() || 'DeepSeek Harness exited before session start')),
        )
      }
    })

    child.stdin.end(input.task)
    const nativeSessionId = await ready
    return {
      nativeSessionId,
      ...(child.pid === undefined ? {} : { processId: child.pid }),
      events: queue,
    }
  }

  #probeFeatures(executablePath: string): { json: boolean; resume: boolean } {
    const help = spawnSync(
      executablePath,
      [...this.#prefixArgs, '--profile', 'headless', '--help'],
      {
        encoding: 'utf8',
        timeout: 5_000,
        env: { ...process.env, ...this.#environment },
      },
    )
    const output = `${help.stdout ?? ''}\n${help.stderr ?? ''}`
    return { json: output.includes('--json'), resume: output.includes('--session-id') }
  }

  async #launchPlain(
    executablePath: string,
    input: StartInput | ResumeInput,
    resumeSessionId?: string,
  ): Promise<WorkerSessionHandle> {
    if (resumeSessionId && !this.#features.resume) {
      throw new Error('This DeepSeek Harness version does not support --session-id')
    }
    const args = [
      ...this.#prefixArgs,
      '--profile',
      'headless',
      ...(resumeSessionId ? ['--session-id', resumeSessionId] : []),
      input.task,
    ]
    const child = spawn(executablePath, args, {
      cwd: input.cwd,
      env: { ...process.env, ...this.#environment, NO_COLOR: '1' },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    const nativeSessionId = `dsh-process:${input.workerSessionId}`
    this.#processes.set(input.workerSessionId, child)
    this.#processes.set(nativeSessionId, child)
    const queue = new AsyncEventQueue()
    let stdout = ''
    let stderr = ''
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    child.stdout.on('data', (chunk: string) => {
      stdout = `${stdout}${chunk}`.slice(-128_000)
    })
    child.stderr.on('data', (chunk: string) => {
      stderr = `${stderr}${chunk}`.slice(-32_000)
      if (chunk.trim()) {
        queue.push({
          type: 'worker/reasoning',
          data: { text: chunk },
          nativeEvent: { stream: 'stderr', text: chunk },
        })
      }
    })
    child.on('error', (error) => {
      queue.push({ type: 'worker/failed', data: { message: error.message } })
      queue.close()
    })
    child.on('close', (code, signal) => {
      this.#processes.delete(input.workerSessionId)
      this.#processes.delete(nativeSessionId)
      if (stdout.trim()) {
        queue.push({
          type: 'worker/message',
          data: { kind: 'final', text: stdout.trim() },
          nativeEvent: { stream: 'stdout', text: stdout },
        })
      }
      queue.push(
        code === 0
          ? { type: 'worker/completed', data: { summary: stdout.trim(), exitCode: code } }
          : {
              type: 'worker/failed',
              data: {
                message: stderr.trim() || 'DeepSeek Harness exited unsuccessfully',
                exitCode: code,
                signal,
              },
            },
      )
      queue.close()
    })
    return {
      nativeSessionId,
      ...(child.pid === undefined ? {} : { processId: child.pid }),
      events: queue,
    }
  }

  async cancel(sessionId: string): Promise<void> {
    const child = this.#processes.get(sessionId)
    if (!child || child.exitCode !== null) return
    child.kill('SIGTERM')
  }

  async dispose(): Promise<void> {
    this.#disposed = true
    const processes = new Set(this.#processes.values())
    this.#processes.clear()
    for (const child of processes) {
      if (child.exitCode === null) child.kill('SIGTERM')
    }
  }
}
