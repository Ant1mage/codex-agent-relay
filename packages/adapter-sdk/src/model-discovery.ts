import type { ModelOption, ReasoningLevel, RuntimeOptions } from '@relay/protocol'

/**
 * Official-HTTP fallback for model discovery.
 *
 * Some runtime CLIs do not publish their model list (the DeepSeek Harness CLI has
 * no model flag at all). When the CLI cannot answer, Relay asks the provider's
 * official API instead and caches the result - see docs/ui.md 16.1.
 *
 * Two rules this module holds to:
 *  - the API key is read from the environment only. It is never written to disk,
 *    never sent to the renderer, and never included in a diagnostic.
 *  - a provider whose endpoint or response shape is not verified is left out
 *    entirely rather than guessed at.
 */

export interface HttpModelQuery {
  /** Provider display name, used in diagnostics. */
  provider: string
  endpoint: string
  /** Environment variables checked, in order, for a bearer token. */
  keyEnv: string[]
  /** Extra headers some providers require. */
  headers?: Record<string, string>
}

export interface HttpModelsResult {
  models: ModelOption[]
  levels: ReasoningLevel[]
  /** True when the request could not run because no API key was configured. */
  authRequired: boolean
  diagnostics: string[]
}

const HTTP_TIMEOUT_MS = 8_000

/** Endpoint and credential names per provider, from the providers' own docs. */
export const httpModelQueries: Record<string, HttpModelQuery> = {
  deepseek: {
    provider: 'DeepSeek',
    endpoint: 'https://api.deepseek.com/models',
    keyEnv: ['DEEPSEEK_API_KEY'],
  },
  kimi: {
    provider: 'Kimi',
    endpoint: 'https://api.moonshot.ai/v1/models',
    keyEnv: ['MOONSHOT_API_KEY', 'KIMI_API_KEY'],
  },
  gemini: {
    provider: 'Gemini',
    endpoint: 'https://generativelanguage.googleapis.com/v1beta/models',
    keyEnv: ['GEMINI_API_KEY', 'GOOGLE_API_KEY'],
    // Gemini takes the key as a query parameter rather than a bearer token.
    headers: {},
  },
  grok: {
    provider: 'Grok',
    endpoint: 'https://api.x.ai/v1/models',
    keyEnv: ['XAI_API_KEY', 'GROK_API_KEY'],
  },
}

function apiKey(query: HttpModelQuery): { key: string; env: string } | undefined {
  for (const env of query.keyEnv) {
    const value = process.env[env]
    if (value && value.trim()) return { key: value.trim(), env }
  }
  return undefined
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : undefined
}

/**
 * Reads models out of the documented response shapes. DeepSeek, Kimi and Grok
 * return an OpenAI-style `{ data: [...] }`; Gemini returns `{ models: [...] }`
 * with names prefixed `models/`.
 */
export function parseModelPayload(payload: unknown): ModelOption[] {
  const root = asRecord(payload)
  if (!root) return []
  const list = Array.isArray(root.data)
    ? root.data
    : Array.isArray(root.models)
      ? root.models
      : []
  const models: ModelOption[] = []
  for (const entry of list) {
    const record = asRecord(entry)
    if (!record) continue
    const rawId = typeof record.id === 'string' ? record.id : typeof record.name === 'string' ? record.name : undefined
    if (!rawId) continue
    // Gemini reports "models/gemini-2.5-pro"; the CLI wants the bare id.
    const value = rawId.replace(/^models\//, '')
    if (!value) continue
    const label = typeof record.display_name === 'string'
      ? record.display_name
      : typeof record.displayName === 'string'
        ? record.displayName
        : typeof record.name === 'string' && typeof record.id === 'string'
          ? record.name
          : value
    models.push({ value, label })
  }
  // Stable, de-duplicated order.
  const seen = new Set<string>()
  return models.filter((model) => (seen.has(model.value) ? false : seen.add(model.value)))
}

/**
 * DeepSeek is the one provider whose model endpoint declares the reasoning
 * levels a model accepts, so its `effort.supported_levels` becomes the slider.
 */
export function parseReasoningPayload(payload: unknown): ReasoningLevel[] {
  const root = asRecord(payload)
  const list = Array.isArray(root?.data) ? root.data : []
  for (const entry of list) {
    const effort = asRecord(asRecord(entry)?.effort)
    const levels = Array.isArray(effort?.supported_levels) ? effort.supported_levels : []
    const values = levels.filter((value): value is string => typeof value === 'string' && value.length > 0)
    if (values.length) {
      return values.slice(0, 5).map((value, index) => ({
        strength: index + 1,
        label: value.charAt(0).toUpperCase() + value.slice(1),
        value,
      }))
    }
  }
  return []
}

/** Never let a credential reach a diagnostic string. */
function redact(message: string, key: string): string {
  return key ? message.split(key).join('<redacted>') : message
}

export async function listModelsOverHttp(
  key: string,
  query = httpModelQueries[key],
  fetchImpl: typeof fetch = fetch,
): Promise<HttpModelsResult> {
  const empty = { models: [], levels: [], authRequired: false, diagnostics: [] }
  if (!query) return empty

  const credential = apiKey(query)
  if (!credential) {
    return {
      ...empty,
      authRequired: true,
      diagnostics: [
        `${query.provider} publishes no model list on its CLI. Set ${query.keyEnv[0]} to let Relay read the model list from the official API`,
      ],
    }
  }

  const url = new URL(query.endpoint)
  // Gemini authenticates with a query parameter; the rest use a bearer header.
  const headers: Record<string, string> = { accept: 'application/json', ...query.headers }
  if (query.provider === 'Gemini') url.searchParams.set('key', credential.key)
  else headers.authorization = `Bearer ${credential.key}`

  try {
    const response = await fetchImpl(url.toString(), {
      headers,
      signal: AbortSignal.timeout(HTTP_TIMEOUT_MS),
    })
    if (!response.ok) {
      return {
        ...empty,
        diagnostics: [`${query.provider} model list request failed with HTTP ${response.status}`],
      }
    }
    const payload: unknown = await response.json()
    const models = parseModelPayload(payload)
    if (!models.length) {
      return {
        ...empty,
        diagnostics: [`${query.provider} returned no models; Relay will use the runtime default`],
      }
    }
    return { models, levels: parseReasoningPayload(payload), authRequired: false, diagnostics: [] }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    return {
      ...empty,
      diagnostics: [`${query.provider} model list request failed: ${redact(message, credential.key)}`],
    }
  }
}

/**
 * Merges CLI-reported options with the official API. The CLI wins whenever it
 * reports anything; the API is consulted only when the CLI is silent, which is
 * the case for runtimes whose CLI exposes no model flag at all.
 */
export async function withModelFallback(
  cli: RuntimeOptions,
  providerKey: string | undefined,
  fetchImpl: typeof fetch = fetch,
): Promise<RuntimeOptions> {
  const needsModels = cli.models.length === 0
  const needsLevels = cli.levels.length === 0
  if (!providerKey || (!needsModels && !needsLevels)) return cli
  const http = await listModelsOverHttp(providerKey, undefined, fetchImpl)
  const usedApi = (needsModels && http.models.length > 0) || (needsLevels && http.levels.length > 0)
  return {
    ...cli,
    models: needsModels ? http.models : cli.models,
    levels: needsLevels ? http.levels : cli.levels,
    source: usedApi ? 'api' : cli.source,
    // CLI-derived notes stay first: they explain why the fallback ran.
    diagnostics: [...cli.diagnostics, ...http.diagnostics],
  }
}
