import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RuntimeOptions } from '@relay/protocol'
import {
  httpModelQueries,
  listModelsOverHttp,
  parseModelPayload,
  parseReasoningPayload,
  withModelFallback,
} from '../src/model-discovery.js'

const originalEnv = { ...process.env }

afterEach(() => {
  process.env = { ...originalEnv }
  vi.restoreAllMocks()
})

describe('parseModelPayload', () => {
  it('reads the OpenAI-shaped list DeepSeek, Kimi and Grok return', () => {
    const models = parseModelPayload({
      object: 'list',
      data: [
        { id: 'deepseek-chat', object: 'model', owned_by: 'deepseek' },
        { id: 'deepseek-reasoner', object: 'model', owned_by: 'deepseek' },
      ],
    })
    expect(models.map((model) => model.value)).toEqual(['deepseek-chat', 'deepseek-reasoner'])
  })

  it('uses the display name when the provider supplies one', () => {
    const models = parseModelPayload({
      object: 'list',
      data: [{ id: 'deepseek-chat', name: 'DeepSeek Chat' }],
    })
    expect(models[0]).toEqual({ value: 'deepseek-chat', label: 'DeepSeek Chat' })
  })

  it('strips the models/ prefix Gemini uses', () => {
    const models = parseModelPayload({
      models: [
        { name: 'models/gemini-2.5-pro', displayName: 'Gemini 2.5 Pro' },
        { name: 'models/gemini-2.5-flash', displayName: 'Gemini 2.5 Flash' },
      ],
    })
    expect(models.map((model) => model.value)).toEqual(['gemini-2.5-pro', 'gemini-2.5-flash'])
    expect(models[0]?.label).toBe('Gemini 2.5 Pro')
  })

  it('de-duplicates repeated ids', () => {
    const models = parseModelPayload({ data: [{ id: 'a' }, { id: 'a' }, { id: 'b' }] })
    expect(models.map((model) => model.value)).toEqual(['a', 'b'])
  })

  it('returns nothing for an unexpected shape instead of guessing', () => {
    expect(parseModelPayload({ error: 'nope' })).toEqual([])
    expect(parseModelPayload(null)).toEqual([])
    expect(parseModelPayload({ data: [{ object: 'model' }] })).toEqual([])
  })
})

describe('parseReasoningPayload', () => {
  it('uses the levels DeepSeek declares for a model', () => {
    const levels = parseReasoningPayload({
      data: [
        { id: 'deepseek-chat' },
        { id: 'deepseek-reasoner', effort: { supported_levels: ['low', 'medium', 'high'] } },
      ],
    })
    expect(levels.map((level) => level.value)).toEqual(['low', 'medium', 'high'])
    expect(levels.map((level) => level.strength)).toEqual([1, 2, 3])
    expect(levels[0]?.label).toBe('Low')
  })

  it('supports a two-stop scale rather than forcing three', () => {
    const levels = parseReasoningPayload({ data: [{ effort: { supported_levels: ['high', 'max'] } }] })
    expect(levels.map((level) => level.value)).toEqual(['high', 'max'])
  })

  it('returns nothing when no model declares levels', () => {
    expect(parseReasoningPayload({ data: [{ id: 'x' }] })).toEqual([])
    expect(parseReasoningPayload({ models: [] })).toEqual([])
  })
})

describe('listModelsOverHttp', () => {
  it('reports a missing API key instead of calling the network', async () => {
    delete process.env.DEEPSEEK_API_KEY
    const fetchSpy = vi.fn()
    const result = await listModelsOverHttp('deepseek', undefined, fetchSpy as unknown as typeof fetch)
    expect(fetchSpy).not.toHaveBeenCalled()
    expect(result.authRequired).toBe(true)
    expect(result.models).toEqual([])
    expect(result.diagnostics[0]).toContain('DEEPSEEK_API_KEY')
  })

  it('sends a bearer token to the documented endpoint', async () => {
    process.env.DEEPSEEK_API_KEY = 'sk-test-123'
    const fetchSpy = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ data: [{ id: 'deepseek-chat' }] }),
    })
    const result = await listModelsOverHttp('deepseek', undefined, fetchSpy as unknown as typeof fetch)
    expect(fetchSpy).toHaveBeenCalledOnce()
    const [url, init] = fetchSpy.mock.calls[0] as [string, { headers: Record<string, string> }]
    expect(url).toBe(httpModelQueries.deepseek?.endpoint)
    expect(init.headers.authorization).toBe('Bearer sk-test-123')
    expect(result.models.map((model) => model.value)).toEqual(['deepseek-chat'])
    expect(result.authRequired).toBe(false)
  })

  it('passes the Gemini key as a query parameter, as that API requires', async () => {
    process.env.GEMINI_API_KEY = 'gm-test-456'
    const fetchSpy = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ models: [{ name: 'models/gemini-2.5-pro' }] }),
    })
    await listModelsOverHttp('gemini', undefined, fetchSpy as unknown as typeof fetch)
    const [url, init] = fetchSpy.mock.calls[0] as [string, { headers: Record<string, string> }]
    expect(url).toContain('key=gm-test-456')
    expect(init.headers.authorization).toBeUndefined()
  })

  it('reports an HTTP failure without leaking the key', async () => {
    process.env.KIMI_API_KEY = 'secret-key-value'
    const fetchSpy = vi.fn().mockResolvedValue({ ok: false, status: 401 })
    const result = await listModelsOverHttp('kimi', undefined, fetchSpy as unknown as typeof fetch)
    expect(result.models).toEqual([])
    expect(result.diagnostics[0]).toContain('HTTP 401')
    expect(result.diagnostics.join(' ')).not.toContain('secret-key-value')
  })

  it('redacts the key if a thrown error embeds it', async () => {
    process.env.DEEPSEEK_API_KEY = 'leaky-key'
    const fetchSpy = vi.fn().mockRejectedValue(new Error('failed for header Bearer leaky-key'))
    const result = await listModelsOverHttp('deepseek', undefined, fetchSpy as unknown as typeof fetch)
    expect(result.diagnostics.join(' ')).not.toContain('leaky-key')
    expect(result.diagnostics.join(' ')).toContain('<redacted>')
  })

  it('reports a provider with no known endpoint as empty', async () => {
    const result = await listModelsOverHttp('unknown-provider')
    expect(result).toEqual({ models: [], levels: [], authRequired: false, diagnostics: [] })
  })
})

describe('withModelFallback', () => {
  const cliEmpty: RuntimeOptions = {
    runtimeId: 'r',
    adapterId: 'a',
    models: [],
    levels: [],
    source: 'default',
    diagnostics: ['cli said nothing'],
  }
  const cliFull: RuntimeOptions = {
    ...cliEmpty,
    models: [{ value: 'cli-model' }],
    levels: [{ strength: 1, label: 'Low', value: 'low' }],
    source: 'cli',
    diagnostics: [],
  }

  it('does not fall back when the CLI already reported a list', async () => {
    const fetchSpy = vi.fn()
    const result = await withModelFallback(cliFull, 'deepseek', fetchSpy as unknown as typeof fetch)
    expect(fetchSpy).not.toHaveBeenCalled()
    expect(result).toBe(cliFull)
    expect(result.source).toBe('cli')
  })

  it('uses the API when the CLI reported nothing, and marks the source', async () => {
    process.env.DEEPSEEK_API_KEY = 'sk-test'
    const fetchSpy = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ data: [{ id: 'deepseek-chat', effort: { supported_levels: ['low', 'high'] } }] }),
    })
    const result = await withModelFallback(cliEmpty, 'deepseek', fetchSpy as unknown as typeof fetch)
    expect(result.models.map((model) => model.value)).toEqual(['deepseek-chat'])
    expect(result.levels.map((level) => level.value)).toEqual(['low', 'high'])
    expect(result.source).toBe('api')
    // The CLI's explanation is kept so the reason for the fallback stays visible.
    expect(result.diagnostics).toContain('cli said nothing')
  })

  it('keeps the CLI list and only fills the gaps', async () => {
    process.env.DEEPSEEK_API_KEY = 'sk-test'
    const cliModelsOnly: RuntimeOptions = { ...cliFull, levels: [], source: 'cli' }
    const fetchSpy = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ data: [{ id: 'api-model', effort: { supported_levels: ['low'] } }] }),
    })
    const result = await withModelFallback(cliModelsOnly, 'deepseek', fetchSpy as unknown as typeof fetch)
    expect(result.models.map((model) => model.value)).toEqual(['cli-model'])
    expect(result.levels.map((level) => level.value)).toEqual(['low'])
  })

  it('stays silent when no provider endpoint is known', async () => {
    const fetchSpy = vi.fn()
    const result = await withModelFallback(cliEmpty, undefined, fetchSpy as unknown as typeof fetch)
    expect(fetchSpy).not.toHaveBeenCalled()
    expect(result).toBe(cliEmpty)
  })
})
