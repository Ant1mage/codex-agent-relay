import { readFile } from 'node:fs/promises'
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import { extname, join, resolve, sep } from 'node:path'
import type { CodexStatus, StreamMessage } from '@relay/relay-api'
import { buildDiagnosticsReport } from './diagnostics.js'
import type { RelayStore } from './store.js'
import type { Environment } from './environment.js'

/**
 * The daemon's only listener: loopback HTTP that serves the built inspector and
 * the projection it renders. It is deliberately a plain node:http server — the
 * surface is a dozen routes and an SSE stream, which does not need a framework.
 */

export interface RelayServerOptions {
  store: RelayStore
  environment(): Environment
  codex(): Promise<CodexStatus>
  refreshEnvironment(): Promise<Environment>
  installCodex(): Promise<{ status: CodexStatus; messages: string[] }>
  webRoot: string
  token: string
  /** Read after binding: the daemon may fall back to the next free port. */
  port: () => number
  version: string
  databasePath: string
  startedAt: string
  /** How often the projection is re-read for SSE clients. */
  tickMs?: number
}

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
}

interface StreamClient {
  response: ServerResponse
  cursors: Map<string, number>
}

function send(response: ServerResponse, message: StreamMessage): void {
  response.write(`data: ${JSON.stringify(message)}\n\n`)
}

export function createRelayServer(options: RelayServerOptions): Server {
  const startedAt = options.startedAt
  const tickMs = options.tickMs ?? 400
  const clients = new Set<StreamClient>()
  let lastRevision = ''
  let ticking = false

  /** Rebuilt per request: the port is only known once the socket is bound. */
  function hostNames(): Set<string> {
    return new Set([
      `127.0.0.1:${options.port()}`,
      `localhost:${options.port()}`,
      '127.0.0.1',
      'localhost',
    ])
  }

  /**
   * Two local-only guards. The Host check stops DNS rebinding (a page on a
   * public name resolving to loopback), and the Origin check stops other sites
   * from reading the log through the user's browser. The token then covers
   * everything that is not the browser.
   */
  function requestAllowed(request: IncomingMessage): boolean {
    const host = request.headers.host
    if (!host || !hostNames().has(host)) return false
    const origin = request.headers.origin
    if (!origin) return true
    try {
      const url = new URL(origin)
      return hostNames().has(url.host)
    } catch {
      return false
    }
  }

  function authorized(request: IncomingMessage, url: URL): boolean {
    if (url.searchParams.get('token') === options.token) return true
    const header = request.headers.authorization
    return header === `Bearer ${options.token}`
  }

  function json(response: ServerResponse, status: number, value: unknown): void {
    const body = JSON.stringify(value)
    response.writeHead(status, {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'no-store',
      'content-length': Buffer.byteLength(body),
    })
    response.end(body)
  }

  function text(response: ServerResponse, status: number, body: string): void {
    response.writeHead(status, {
      'content-type': 'text/plain; charset=utf-8',
      'cache-control': 'no-store',
    })
    response.end(body)
  }

  async function serveStatic(response: ServerResponse, pathname: string): Promise<void> {
    const webRoot = resolve(options.webRoot)
    const relative = pathname === '/' ? 'index.html' : pathname.replace(/^\/+/, '')
    const candidate = resolve(join(webRoot, relative))
    // Path traversal guard: never read outside the built web directory.
    const inside = candidate === webRoot || candidate.startsWith(`${webRoot}${sep}`)
    const target = inside ? candidate : join(webRoot, 'index.html')
    try {
      const body = await readFile(target)
      response.writeHead(200, {
        'content-type': MIME[extname(target)] ?? 'application/octet-stream',
        // The inspector is a local, always-current build; never cache it.
        'cache-control': 'no-store',
      })
      response.end(body)
    } catch {
      if (pathname !== '/' && !extname(pathname)) {
        try {
          const fallback = await readFile(join(webRoot, 'index.html'))
          response.writeHead(200, { 'content-type': MIME['.html'] ?? 'text/html', 'cache-control': 'no-store' })
          response.end(fallback)
          return
        } catch {
          // Fall through to the missing-build message below.
        }
      }
      text(
        response,
        200,
        [
          'Relay inspector is not built yet.',
          '',
          'Build it with:  pnpm web:build',
          `API is available at http://127.0.0.1:${options.port()}/api/health?token=…`,
        ].join('\n'),
      )
    }
  }

  async function handleApi(
    request: IncomingMessage,
    response: ServerResponse,
    url: URL,
  ): Promise<boolean> {
    const path = url.pathname

    if (path === '/api/health') {
      const snapshot = options.store.snapshot({
        ...options.environment(),
        codex: await options.codex(),
      })
      json(response, 200, {
        ok: true,
        pid: process.pid,
        port: options.port(),
        startedAt,
        version: options.version,
        database: options.databasePath,
        sessions: snapshot.sessions.length,
        runs: snapshot.sessions.reduce((total, session) => total + session.runs.length, 0),
      })
      return true
    }

    if (path === '/api/snapshot') {
      json(
        response,
        200,
        options.store.snapshot({ ...options.environment(), codex: await options.codex() }),
      )
      return true
    }

    if (path === '/api/menu') {
      json(response, 200, options.store.menu({ codex: await options.codex() }))
      return true
    }

    if (path === '/api/diagnostics') {
      const snapshot = options.store.snapshot({ ...options.environment(), codex: await options.codex() })
      text(
        response,
        200,
        buildDiagnosticsReport({
          appVersion: options.version,
          nodeVersion: process.versions.node,
          platform: process.platform,
          arch: process.arch,
          port: options.port(),
          databasePath: options.databasePath,
          snapshot,
        }),
      )
      return true
    }

    const events = /^\/api\/runs\/([^/]+)\/events$/.exec(path)
    if (events) {
      const runId = decodeURIComponent(events[1] ?? '')
      const after = Number(url.searchParams.get('after') ?? '0')
      json(response, 200, options.store.eventsFor(runId, Number.isFinite(after) ? after : 0))
      return true
    }

    const cancelWorker = /^\/api\/workers\/([^/]+)\/cancel$/.exec(path)
    if (cancelWorker && request.method === 'POST') {
      json(response, 200, options.store.cancelWorker(decodeURIComponent(cancelWorker[1] ?? '')))
      return true
    }

    const cancelSession = /^\/api\/sessions\/([^/]+)\/cancel$/.exec(path)
    if (cancelSession && request.method === 'POST') {
      json(response, 200, options.store.cancelSession(decodeURIComponent(cancelSession[1] ?? '')))
      return true
    }

    if (path === '/api/codex/install' && request.method === 'POST') {
      json(response, 200, await options.installCodex())
      return true
    }

    if (path === '/api/refresh' && request.method === 'POST') {
      const environment = await options.refreshEnvironment()
      json(response, 200, {
        runtimes: environment.runtimes.length,
        profiles: environment.profiles.length,
      })
      return true
    }

    return false
  }

  function openStream(request: IncomingMessage, response: ServerResponse): void {
    response.writeHead(200, {
      'content-type': 'text/event-stream; charset=utf-8',
      'cache-control': 'no-store',
      connection: 'keep-alive',
      'x-accel-buffering': 'no',
    })
    response.write(': relay stream\n\n')
    const client: StreamClient = {
      response,
      // Start from "now": the client pulls history per run and de-duplicates by
      // sequence, so the stream only has to deliver what happens next.
      cursors: options.store.cursors(),
    }
    clients.add(client)
    void (async () => {
      send(response, { type: 'hello', port: options.port(), startedAt })
      send(response, {
        type: 'snapshot',
        snapshot: options.store.snapshot({ ...options.environment(), codex: await options.codex() }),
      })
    })()
    const heartbeat = setInterval(() => response.write(': ping\n\n'), 15_000)
    request.on('close', () => {
      clearInterval(heartbeat)
      clients.delete(client)
    })
  }

  async function tick(): Promise<void> {
    if (ticking || clients.size === 0) return
    ticking = true
    try {
      const revision = options.store.revision()
      if (revision === lastRevision) return
      lastRevision = revision
      const codex = await options.codex()
      const snapshot = options.store.snapshot({ ...options.environment(), codex })
      const current = options.store.cursors()
      for (const client of clients) {
        for (const [runId, seq] of current) {
          const sent = client.cursors.get(runId) ?? 0
          if (seq <= sent) continue
          const batch = options.store.eventsFor(runId, sent)
          client.cursors.set(runId, seq)
          if (batch.events.length > 0) send(client.response, { type: 'events', batch })
        }
        send(client.response, { type: 'snapshot', snapshot })
      }
    } finally {
      ticking = false
    }
  }

  const timer = setInterval(() => void tick(), tickMs)
  const server = createServer((request, response) => {
    void (async () => {
      const url = new URL(request.url ?? '/', `http://127.0.0.1:${options.port()}`)
      if (!requestAllowed(request)) {
        text(response, 403, 'Relay only answers loopback requests from its own origin')
        return
      }
      if (url.pathname.startsWith('/api/')) {
        if (!authorized(request, url)) {
          json(response, 401, { error: 'Missing or invalid Relay token' })
          return
        }
        if (url.pathname === '/api/stream') {
          openStream(request, response)
          return
        }
        if (await handleApi(request, response, url)) return
        json(response, 404, { error: `Unknown Relay API route: ${url.pathname}` })
        return
      }
      await serveStatic(response, url.pathname)
    })().catch((error: unknown) => {
      if (!response.headersSent) json(response, 500, { error: error instanceof Error ? error.message : String(error) })
      else response.end()
    })
  })
  server.on('close', () => clearInterval(timer))
  return server
}
