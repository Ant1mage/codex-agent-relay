import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { createInterface } from 'node:readline'
import { RelayError } from '@relay/protocol'

/**
 * The CLI is not always on PATH: the VS Code extension and the desktop app ship
 * their own copy, which is the normal install for most users. Relay resolves the
 * same candidates the daemon's checks use, so session sync works either way.
 */
function codexCandidates(): string[] {
  const home = homedir()
  const codexHome = process.env.CODEX_HOME ?? join(home, '.codex')
  const candidates = [
    process.env.CODEX_PATH,
    'codex',
    join(codexHome, 'bin', 'codex'),
    join(home, '.local', 'bin', 'codex'),
    join(home, '.npm-global', 'bin', 'codex'),
    '/usr/local/bin/codex',
    '/opt/homebrew/bin/codex',
  ].filter((candidate): candidate is string => Boolean(candidate))
  for (const root of [join(home, '.vscode', 'extensions'), join(home, '.vscode-insiders', 'extensions')]) {
    try {
      const versions = readdirSync(root)
        .filter((entry) => entry.startsWith('openai.chatgpt-'))
        .sort()
        .reverse()
      for (const entry of versions) {
        for (const target of ['macos-aarch64', 'macos-x86_64', 'linux-x86_64', 'linux-aarch64']) {
          const candidate = join(root, entry, 'bin', target, 'codex')
          if (existsSync(candidate)) candidates.push(candidate)
        }
      }
    } catch {
      // VS Code is optional.
    }
  }
  return candidates
}

export interface CodexThreadMetadata {
  id: string
  displayName: string
  cwd: string
  model?: string
}

export interface CodexThreadMetadataResolver {
  resolve(threadId: string): Promise<CodexThreadMetadata>
}

export interface CodexAppServerOptions {
  executablePath?: string
  prefixArgs?: string[]
  timeoutMs?: number
  environment?: NodeJS.ProcessEnv
}

interface JsonRpcResponse {
  id?: number
  result?: unknown
  error?: { message?: string }
}

interface ThreadResponse {
  thread?: {
    id?: unknown
    name?: unknown
    preview?: unknown
    cwd?: unknown
    model?: unknown
  }
}

function send(child: ChildProcessWithoutNullStreams, message: unknown): void {
  child.stdin.write(`${JSON.stringify(message)}\n`)
}

export class CodexAppServerThreadResolver implements CodexThreadMetadataResolver {
  readonly #executablePath: string
  readonly #prefixArgs: string[]
  readonly #timeoutMs: number
  readonly #environment: NodeJS.ProcessEnv | undefined

  readonly #explicit: boolean

  constructor(options: CodexAppServerOptions = {}) {
    this.#explicit = Boolean(options.executablePath)
    this.#executablePath = options.executablePath ?? 'codex'
    this.#prefixArgs = options.prefixArgs ?? []
    this.#timeoutMs = options.timeoutMs ?? 5_000
    this.#environment = options.environment
  }

  /**
   * Tries the configured path first, then the locations Codex installs itself
   * into. A missing executable must not surface as "spawn codex ENOENT" to the
   * model: it either finds a working CLI or reports a clear reason.
   */
  async resolve(threadId: string): Promise<CodexThreadMetadata> {
    const candidates = this.#explicit ? [this.#executablePath] : [this.#executablePath, ...codexCandidates()]
    let lastError: Error | undefined
    for (const candidate of [...new Set(candidates)]) {
      if (candidate !== 'codex' && !existsSync(candidate)) continue
      try {
        return await this.#resolveWith(candidate, threadId)
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error))
        // Only a missing binary is worth retrying with the next candidate.
        if (!/ENOENT|not found/i.test(lastError.message)) throw lastError
      }
    }
    throw lastError ?? new RelayError('RUNTIME_NOT_FOUND', 'Codex CLI was not found')
  }

  async #resolveWith(executablePath: string, threadId: string): Promise<CodexThreadMetadata> {
    const child = spawn(executablePath, [...this.#prefixArgs, 'app-server', '--stdio'], {
      env: { ...process.env, ...this.#environment },
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity })
    let stderr = ''
    child.stderr.setEncoding('utf8')
    child.stderr.on('data', (chunk: string) => {
      stderr = `${stderr}${chunk}`.slice(-8_192)
    })

    return new Promise<CodexThreadMetadata>((resolve, reject) => {
      let settled = false
      const finish = (error?: Error, metadata?: CodexThreadMetadata): void => {
        if (settled) return
        settled = true
        clearTimeout(timeout)
        lines.close()
        if (child.exitCode === null) child.kill('SIGTERM')
        if (error) reject(error)
        else if (metadata) resolve(metadata)
      }
      const timeout = setTimeout(
        () => finish(new Error(`Timed out reading Codex thread ${threadId}`)),
        this.#timeoutMs,
      )

      child.on('error', (error) => finish(error))
      child.on('close', () => {
        if (!settled) finish(new Error(stderr.trim() || 'Codex app-server exited unexpectedly'))
      })
      lines.on('line', (line) => {
        let response: JsonRpcResponse
        try {
          response = JSON.parse(line) as JsonRpcResponse
        } catch {
          return
        }
        if (response.id === 1) {
          send(child, { method: 'initialized' })
          send(child, {
            method: 'thread/read',
            id: 2,
            params: { threadId, includeTurns: false },
          })
          return
        }
        if (response.id !== 2) return
        if (response.error) {
          finish(new Error(response.error.message ?? `Codex thread ${threadId} was not found`))
          return
        }
        const result = response.result as ThreadResponse | undefined
        const thread = result?.thread
        const name = typeof thread?.name === 'string' && thread.name.trim() ? thread.name : undefined
        const preview =
          typeof thread?.preview === 'string' && thread.preview.trim() ? thread.preview : undefined
        const displayName = name ?? preview
        if (!displayName) {
          finish(
            new RelayError(
              'SESSION_NAME_UNAVAILABLE',
              `Codex did not provide a display name for thread ${threadId}`,
            ),
          )
          return
        }
        if (typeof thread?.id !== 'string' || typeof thread.cwd !== 'string') {
          finish(new Error(`Codex returned invalid metadata for thread ${threadId}`))
          return
        }
        finish(undefined, {
          id: thread.id,
          displayName,
          cwd: thread.cwd,
          ...(typeof thread.model === 'string' ? { model: thread.model } : {}),
        })
      })

      send(child, {
        method: 'initialize',
        id: 1,
        params: {
          clientInfo: { name: 'relay', title: 'Relay', version: '0.0.0' },
          capabilities: null,
        },
      })
    })
  }
}

