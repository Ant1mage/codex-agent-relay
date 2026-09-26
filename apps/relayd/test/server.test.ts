import { mkdtempSync, rmSync } from 'node:fs'
import { request } from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { Server } from 'node:http'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createRelayServer } from '../src/server.js'
import { RelayStore } from '../src/store.js'
import { profiles, runtimes, seedDatabase } from './fixtures.js'

const TOKEN = 'test-token'

let directory: string
let store: RelayStore
let server: Server
let port: number

function call(path: string, options: { token?: string; host?: string; method?: string } = {}) {
  const query =
    options.token === undefined ? '' : `${path.includes('?') ? '&' : '?'}token=${options.token}`
  return new Promise<{ status: number; body: string }>((resolve, reject) => {
    const req = request(
      {
        host: '127.0.0.1',
        port,
        path: `${path}${query}`,
        method: options.method ?? 'GET',
        headers: { host: options.host ?? `127.0.0.1:${port}` },
      },
      (res) => {
        let body = ''
        res.setEncoding('utf8')
        res.on('data', (chunk) => (body += chunk))
        res.on('end', () => resolve({ status: res.statusCode ?? 0, body }))
      },
    )
    req.on('error', reject)
    req.end()
  })
}

beforeEach(async () => {
  directory = mkdtempSync(join(tmpdir(), 'relay-server-'))
  const database = join(directory, 'relay.sqlite')
  seedDatabase(database)
  store = new RelayStore(database)
  store.setEnvironment({ runtimes, profiles })
  const portRef = { current: 0 }
  server = createRelayServer({
    store,
    environment: () => store.runtimesAndProfiles(),
    codex: async () => ({ checks: [], configured: true }),
    refreshEnvironment: async () => store.runtimesAndProfiles(),
    installCodex: async () => ({ status: { checks: [], configured: true }, messages: [] }),
    webRoot: join(directory, 'missing-web'),
    token: TOKEN,
    port: () => portRef.current,
    version: '0.0.0',
    databasePath: database,
    startedAt: '2026-01-01T10:00:00.000Z',
    tickMs: 20,
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  port = (server.address() as AddressInfo).port
  portRef.current = port
})

afterEach(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()))
  store.close()
  rmSync(directory, { recursive: true, force: true })
})

describe('relay daemon', () => {
  it('refuses API calls without the token', async () => {
    const response = await call('/api/health')
    expect(response.status).toBe(401)
  })

  it('answers the projection with the token', async () => {
    const health = await call('/api/health', { token: TOKEN })
    expect(health.status).toBe(200)
    expect(JSON.parse(health.body)).toMatchObject({ ok: true, sessions: 1, runs: 1 })

    const menu = await call('/api/menu', { token: TOKEN })
    const parsed = JSON.parse(menu.body) as { runningWorkers: number; sessions: Array<{ id: string }> }
    expect(parsed.runningWorkers).toBe(1)
    expect(parsed.sessions[0]?.id).toBe('codex:one')

    const events = await call('/api/runs/run-1/events?after=3', { token: TOKEN })
    expect((JSON.parse(events.body) as { events: unknown[] }).events).toHaveLength(2)
  })

  it('rejects a foreign Host so a public name cannot resolve to loopback', async () => {
    const response = await call('/api/health', { token: TOKEN, host: 'evil.example' })
    expect(response.status).toBe(403)
  })

  it('tells the user how to build the inspector when there is no bundle', async () => {
    const response = await call('/', { token: TOKEN })
    expect(response.status).toBe(200)
    expect(response.body).toContain('pnpm web:build')
  })

  it('queues cancellation through the API', async () => {
    const response = await call('/api/workers/worker-1/cancel', { token: TOKEN, method: 'POST' })
    expect(response.status).toBe(200)
    expect(JSON.parse(response.body)).toMatchObject({ accepted: true })
  })

  it('streams the first snapshot to an SSE client', async () => {
    const body = await new Promise<string>((resolve, reject) => {
      const req = request(
        {
          host: '127.0.0.1',
          port,
          path: `/api/stream?token=${TOKEN}`,
          headers: { host: `127.0.0.1:${port}` },
        },
        (res) => {
          let received = ''
          res.setEncoding('utf8')
          res.on('data', (chunk) => {
            received += chunk
            if (received.includes('"snapshot"')) {
              req.destroy()
              resolve(received)
            }
          })
        },
      )
      req.on('error', reject)
      req.end()
      setTimeout(() => reject(new Error('no stream message')), 4_000).unref()
    })
    expect(body).toContain('"type":"hello"')
    expect(body).toContain('"snapshot"')
    expect(body).toContain('Refine the inspector')
  })
})
