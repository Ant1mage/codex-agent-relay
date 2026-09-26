import { describe, expect, it } from 'vitest'
import { buildPath, parsePath } from '../src/lib/router.js'
import { parseToken, resolveToken, withoutToken } from '../src/lib/token.js'

describe('routes', () => {
  it('reads the two shapes the inspector uses', () => {
    expect(parsePath('/')).toEqual({})
    expect(parsePath('/s/codex%3Aone')).toEqual({ sessionId: 'codex:one' })
    expect(parsePath('/s/codex%3Aone/r/run-1')).toEqual({ sessionId: 'codex:one', runId: 'run-1' })
    expect(parsePath('/nonsense')).toEqual({})
  })

  it('round-trips through the URL', () => {
    expect(buildPath({})).toBe('/')
    expect(buildPath({ sessionId: 'codex:one' })).toBe('/s/codex%3Aone')
    expect(buildPath({ sessionId: 'codex:one', runId: 'run-1' })).toBe('/s/codex%3Aone/r/run-1')
    expect(parsePath(buildPath({ sessionId: 'a b', runId: 'c/d' }))).toEqual({ sessionId: 'a b', runId: 'c/d' })
  })
})

describe('token bootstrap', () => {
  it('takes the token from the fragment and stores it', () => {
    const storage = new Map<string, string>()
    const result = resolveToken('http://127.0.0.1:7352/s/codex%3Aone#t=abc123', {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => void storage.set(key, value),
    })
    expect(result).toEqual({ token: 'abc123', fromUrl: true })
    expect(storage.get('relay.token')).toBe('abc123')
  })

  it('falls back to storage, and reports when there is nothing', () => {
    const storage = new Map<string, string>([['relay.token', 'stored']])
    const read = {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => void storage.set(key, value),
    }
    expect(resolveToken('http://127.0.0.1:7352/', read)).toEqual({ token: 'stored', fromUrl: false })
    expect(resolveToken('http://127.0.0.1:7352/', { getItem: () => null, setItem: () => {} }).token).toBeUndefined()
  })

  it('strips the token from the address bar but keeps the route', () => {
    expect(withoutToken('http://127.0.0.1:7352/s/codex%3Aone#t=abc123')).toBe('http://127.0.0.1:7352/s/codex%3Aone')
    expect(withoutToken('http://127.0.0.1:7352/?t=abc123')).toBe('http://127.0.0.1:7352/')
    expect(parseToken('http://127.0.0.1:7352/#t=')).toBeUndefined()
  })
})
