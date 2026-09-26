import type { AgentProfile, RelayPolicy, RelayPolicyOverride } from '@relay/protocol'
import {
  adapterCatalogSchema,
  cancelResultSchema,
  eventBatchSchema,
  healthSchema,
  installResultSchema,
  menuViewSchema,
  refreshResultSchema,
  relayConfigSchema,
  runtimeMutationSchema,
  runtimeOptionsViewSchema,
  runtimeProbeSchema,
  snapshotSchema,
  streamMessageSchema,
  type AdapterCatalog,
  type CancelResult,
  type CodexAction,
  type EventBatch,
  type Health,
  type InstallResult,
  type InspectorSnapshot,
  type MenuView,
  type RefreshResult,
  type RelayConfigView,
  type RuntimeMutation,
  type RuntimeOptionsView,
  type RuntimeProbe,
  type StreamMessage,
} from './contract.js'

type FetchLike = (
  input: string,
  init?: { method?: string; headers?: Record<string, string>; body?: string },
) => Promise<{
  ok: boolean
  status: number
  text(): Promise<string>
  json(): Promise<unknown>
}>

export interface RelayClientOptions {
  /** Loopback base URL, e.g. http://127.0.0.1:7352 */
  baseUrl: string
  token?: string
  fetch?: FetchLike
}

export class RelayApiError extends Error {
  readonly status: number
  constructor(message: string, status: number) {
    super(message)
    this.name = 'RelayApiError'
    this.status = status
  }
}

export function normalizeBaseUrl(url: string): string {
  return url.replace(/\/+$/, '')
}

/**
 * Talks to apps/relayd. The daemon binds loopback and requires the run token, so
 * every call carries it; the browser gets it once in the URL fragment and keeps
 * it in local storage (see docs/architecture.md).
 */
export class RelayClient {
  readonly baseUrl: string
  readonly #token: string | undefined
  readonly #fetch: FetchLike

  constructor(options: RelayClientOptions) {
    this.baseUrl = normalizeBaseUrl(options.baseUrl)
    this.#token = options.token
    // The global fetch must keep its global receiver: a browser throws
    // "Illegal invocation" when window.fetch is called as a bare method.
    this.#fetch = options.fetch ?? (globalThis.fetch.bind(globalThis) as unknown as FetchLike)
  }

  /** Absolute URL for an API path, carrying the token as a query parameter. */
  url(path: string, params: Record<string, string | number> = {}): string {
    const search = new URLSearchParams()
    for (const [key, value] of Object.entries(params)) search.set(key, String(value))
    if (this.#token) search.set('token', this.#token)
    const query = search.toString()
    return `${this.baseUrl}${path}${query ? `?${query}` : ''}`
  }

  async #json<T>(path: string, schema: { parse(value: unknown): T }, params?: Record<string, string | number>): Promise<T> {
    const response = await this.#fetch(this.url(path, params), {
      headers: this.#headers(),
    })
    const body = await response.text()
    if (!response.ok) throw new RelayApiError(body || response.status.toString(), response.status)
    return schema.parse(JSON.parse(body))
  }

  #headers(): Record<string, string> {
    return this.#token ? { authorization: `Bearer ${this.#token}` } : {}
  }

  health(): Promise<Health> {
    return this.#json('/api/health', healthSchema)
  }

  snapshot(): Promise<InspectorSnapshot> {
    return this.#json('/api/snapshot', snapshotSchema)
  }

  menu(): Promise<MenuView> {
    return this.#json('/api/menu', menuViewSchema)
  }

  events(runId: string, after = 0): Promise<EventBatch> {
    return this.#json(`/api/runs/${encodeURIComponent(runId)}/events`, eventBatchSchema, { after })
  }

  diagnostics(): Promise<string> {
    return this.#raw('/api/diagnostics')
  }

  async #raw(path: string): Promise<string> {
    const response = await this.#fetch(this.url(path), { headers: this.#headers() })
    const body = await response.text()
    if (!response.ok) throw new RelayApiError(body || response.status.toString(), response.status)
    return body
  }

  async #send<T>(
    method: 'POST' | 'PUT' | 'DELETE',
    path: string,
    schema: { parse(value: unknown): T },
    payload?: unknown,
  ): Promise<T> {
    const response = await this.#fetch(this.url(path), {
      method,
      headers: {
        ...this.#headers(),
        ...(payload === undefined ? {} : { 'content-type': 'application/json' }),
      },
      ...(payload === undefined ? {} : { body: JSON.stringify(payload) }),
    })
    const body = await response.text()
    if (!response.ok) throw new RelayApiError(body || response.status.toString(), response.status)
    return schema.parse(body ? JSON.parse(body) : {})
  }

  /** Relay's own configuration (Agent Profiles + policy) as stored on disk. */
  config(): Promise<RelayConfigView> {
    return this.#json('/api/config', relayConfigSchema)
  }

  saveProfile(profile: AgentProfile): Promise<RelayConfigView> {
    return this.#send('PUT', `/api/config/profiles/${encodeURIComponent(profile.id)}`, relayConfigSchema, profile)
  }

  deleteProfile(profileId: string): Promise<RelayConfigView> {
    return this.#send('DELETE', `/api/config/profiles/${encodeURIComponent(profileId)}`, relayConfigSchema)
  }

  savePolicy(input: {
    policy: RelayPolicy
    workspaceOverrides: Record<string, RelayPolicyOverride>
  }): Promise<RelayConfigView> {
    return this.#send('PUT', '/api/config/policy', relayConfigSchema, input)
  }

  /** Model and reasoning values this runtime's CLI actually advertises. */
  runtimeOptions(runtimeId: string): Promise<RuntimeOptionsView> {
    return this.#json(`/api/runtimes/${encodeURIComponent(runtimeId)}/options`, runtimeOptionsViewSchema)
  }

  /** Registers a runtime by hand; the daemon probes it before saving. */
  saveRuntime(runtime: { id: string; adapterId: string; executablePath: string; label?: string }): Promise<RuntimeMutation> {
    return this.#send('PUT', `/api/config/runtimes/${encodeURIComponent(runtime.id)}`, runtimeMutationSchema, runtime)
  }

  deleteRuntime(runtimeId: string): Promise<RelayConfigView> {
    return this.#send('DELETE', `/api/config/runtimes/${encodeURIComponent(runtimeId)}`, relayConfigSchema)
  }

  /** Every adapter Relay can drive, including CLIs it did not detect. */
  adapters(): Promise<AdapterCatalog> {
    return this.#json('/api/adapters', adapterCatalogSchema)
  }

  /** Checks an executable before it is saved, so the form can report failures. */
  probeRuntime(input: { adapterId: string; executablePath: string }): Promise<RuntimeProbe> {
    return this.#send('POST', '/api/runtimes/probe', runtimeProbeSchema, input)
  }

  /** Re-detects runtimes and re-reads configuration. */
  refresh(): Promise<RefreshResult> {
    return this.#send('POST', '/api/refresh', refreshResultSchema)
  }

  /** Codex integration lifecycle: install, repair, update, remove. */
  codex(action: CodexAction): Promise<InstallResult> {
    return this.#send('POST', `/api/codex/${action}`, installResultSchema)
  }

  cancelWorker(workerSessionId: string): Promise<CancelResult> {
    return this.#send('POST', `/api/workers/${encodeURIComponent(workerSessionId)}/cancel`, cancelResultSchema)
  }

  cancelSession(hostSessionId: string): Promise<CancelResult> {
    return this.#send('POST', `/api/sessions/${encodeURIComponent(hostSessionId)}/cancel`, cancelResultSchema)
  }

  /** SSE endpoint. EventSource cannot set headers, so the token rides the query. */
  streamUrl(): string {
    return this.url('/api/stream')
  }

  /**
   * Subscribes to the live projection. Browser-only by design: Node callers
   * (the tray) poll instead, which keeps one transport per environment.
   */
  stream(handlers: {
    onMessage(message: StreamMessage): void
    onError?(error: unknown): void
    onOpen?(): void
  }): () => void {
    const source = new EventSource(this.streamUrl())
    source.onopen = () => handlers.onOpen?.()
    source.onerror = (error) => handlers.onError?.(error)
    source.onmessage = (event) => {
      try {
        handlers.onMessage(streamMessageSchema.parse(JSON.parse(String(event.data))))
      } catch (error) {
        handlers.onError?.(error)
      }
    }
    return () => source.close()
  }
}
