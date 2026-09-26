import { describe, expect, it } from 'vitest'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { RelayClient, normalizeBaseUrl } from '../src/client.js'
import { menuViewSchema, snapshotSchema } from '../src/contract.js'
import {
  clearServerInfo,
  inspectorUrl,
  readServerInfo,
  writeServerInfo,
  type ServerInfo,
} from '../src/server-info.js'

const info: ServerInfo = {
  pid: 4242,
  port: 7352,
  url: 'http://127.0.0.1:7352',
  token: 'secret',
  startedAt: '2026-01-01T10:00:00.000Z',
  version: '0.0.0',
  database: '/tmp/relay.sqlite',
}

describe('server info', () => {
  it('round-trips through the file and clears only its own entry', () => {
    const directory = mkdtempSync(join(tmpdir(), 'relay-info-'))
    const path = join(directory, 'server.json')
    writeServerInfo(info, path)
    expect(readServerInfo(path)).toEqual(info)

    // A different process must not delete the live daemon's record.
    clearServerInfo(999, path)
    expect(readServerInfo(path)).toBeDefined()
    clearServerInfo(info.pid, path)
    expect(readServerInfo(path)).toBeUndefined()
    rmSync(directory, { recursive: true, force: true })
  })

  it('treats a hand-edited file as no daemon instead of throwing', () => {
    const directory = mkdtempSync(join(tmpdir(), 'relay-info-'))
    const path = join(directory, 'server.json')
    writeServerInfo({ ...info, port: 'nope' as unknown as number }, path)
    expect(readServerInfo(path)).toBeUndefined()
    rmSync(directory, { recursive: true, force: true })
  })

  it('puts the token in the fragment so a browser never sends it as a referrer', () => {
    expect(inspectorUrl(info)).toBe('http://127.0.0.1:7352/#t=secret')
    expect(inspectorUrl(info, 'codex:one', 'run-1')).toBe('http://127.0.0.1:7352/s/codex%3Aone/r/run-1#t=secret')
  })
})

describe('RelayClient', () => {
  const calls: string[] = []
  const fakeFetch = async (url: string, init?: { method?: string; headers?: Record<string, string> }) => {
    calls.push(`${init?.method ?? 'GET'} ${url} ${init?.headers?.authorization ?? ''}`.trim())
    return {
      ok: true,
      status: 200,
      text: async () => JSON.stringify(url.includes('/api/menu')
        ? { status: 'ready', runningWorkers: 0, awaitingHost: 0, sessions: [], agents: [], codex: { checks: [], configured: true } }
        : {}),
      json: async () => ({}),
    }
  }

  it('carries the token on every request and normalizes the base URL', () => {
    calls.length = 0
    const client = new RelayClient({ baseUrl: 'http://127.0.0.1:7352/', token: 'secret', fetch: fakeFetch as never })
    expect(client.baseUrl).toBe('http://127.0.0.1:7352')
    expect(client.url('/api/health')).toBe('http://127.0.0.1:7352/api/health?token=secret')
    expect(client.url('/api/runs/a/b/events', { after: 12 })).toBe(
      'http://127.0.0.1:7352/api/runs/a/b/events?after=12&token=secret',
    )
    expect(client.streamUrl()).toBe('http://127.0.0.1:7352/api/stream?token=secret')
  })

  it('validates what the daemon returns', async () => {
    const client = new RelayClient({ baseUrl: 'http://127.0.0.1:7352', token: 'secret', fetch: fakeFetch as never })
    await expect(client.menu()).resolves.toMatchObject({ status: 'ready' })
    await expect(client.snapshot()).rejects.toThrow()
  })

  it('surfaces an error status with the daemon message', async () => {
    const failing = async () => ({ ok: false, status: 401, text: async () => 'Missing or invalid Relay token', json: async () => ({}) })
    const client = new RelayClient({ baseUrl: 'http://127.0.0.1:7352', fetch: failing as never })
    await expect(client.health()).rejects.toThrow('Missing or invalid Relay token')
  })

  it('normalizes trailing slashes', () => {
    expect(normalizeBaseUrl('http://127.0.0.1:7352///')).toBe('http://127.0.0.1:7352')
  })

  it('keeps the global receiver when it falls back to the global fetch', async () => {
    const original = globalThis.fetch
    const receivers: unknown[] = []
    globalThis.fetch = function (this: unknown) {
      receivers.push(this)
      return Promise.resolve({
        ok: true,
        status: 200,
        text: async () =>
          JSON.stringify({
            ok: true,
            pid: 1,
            port: 7352,
            startedAt: '2026-01-01T10:00:00.000Z',
            version: '0.0.0',
            database: '/tmp/relay.sqlite',
            sessions: 0,
            runs: 0,
          }),
        json: async () => ({}),
      })
    } as unknown as typeof fetch
    try {
      const client = new RelayClient({ baseUrl: 'http://127.0.0.1:7352' })
      await expect(client.health()).resolves.toMatchObject({ ok: true })
    } finally {
      globalThis.fetch = original
    }
    // A browser throws "Illegal invocation" when window.fetch loses its receiver.
    expect(receivers[0]).toBe(globalThis)
  })
})

describe('contract', () => {
  it('rejects a menu payload without the codex block', () => {
    expect(() => menuViewSchema.parse({ status: 'ready', runningWorkers: 0, awaitingHost: 0, sessions: [], agents: [] })).toThrow()
  })

  it('accepts an empty snapshot', () => {
    const parsed = snapshotSchema.parse({
      sessions: [],
      runtimes: [],
      profiles: [],
      diagnostics: [],
      codex: { checks: [], configured: false },
      generatedAt: '2026-01-01T10:00:00.000Z',
    })
    expect(parsed.sessions).toEqual([])
  })
})
