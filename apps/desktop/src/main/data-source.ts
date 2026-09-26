import { execFile } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { promisify } from 'node:util'
import type {
  AgentProfile,
  RelayPolicy,
  RelayPolicyOverride,
  Runtime,
  RuntimeOptions,
} from '@relay/protocol'
import {
  hostSessionSchema,
  agentProfileSchema,
  relayEventSchema,
  relayPolicyOverrideSchema,
  relayPolicySchema,
} from '@relay/protocol'
import type { AgentAdapter } from '@relay/adapter-sdk'
import { AntigravityAdapter } from '@relay/adapter-antigravity'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { GeminiAdapter } from '@relay/adapter-gemini'
import { KimiAdapter } from '@relay/adapter-kimi'
import { ZaiAdapter } from '@relay/adapter-zai'
import { defaultPolicy, defaultProfiles, projectRun } from '@relay/core'
import { httpModelQueries } from '@relay/adapter-sdk'
import type {
  CodexIntegrationCheck,
  CodexIntegrationInstallResult,
  CodexIntegrationStatus,
  DesktopMenuBarPrefs,
  DesktopRunView,
  DesktopSettings,
  DesktopSnapshot,
} from '../shared/api.js'

const execFileAsync = promisify(execFile)

/**
 * Real detection for the three checks in docs/ui.md 22.2 "Connect Codex".
 * The integration ships as repo content (integrations/codex); these probe the
 * user's Codex home so onboarding reports actual status rather than an
 * optimistic checklist.
 */
const CODEX_PROBE_TIMEOUT_MS = 4_000

/** Runtime option probes spawn the CLI, so reuse a recent result. */
const OPTIONS_CACHE_MS = 30_000

/** An API-derived model list is stable; keep it across restarts for a day. */
const OPTIONS_DISK_TTL_MS = 24 * 60 * 60 * 1_000

function codexHome(): string {
  const configured = process.env.CODEX_HOME
  return configured && configured.length > 0 ? configured : join(homedir(), '.codex')
}

/**
 * Candidate `codex` executables, in the order Codex itself installs them.
 *
 * The CLI is still named `codex`. It just is not always on PATH: the VS Code
 * extension and the desktop app both bundle their own copy, which is the normal
 * install for most users, so a PATH-only check reports "not found" on a machine
 * that plainly has Codex running.
 */
function codexCandidates(): string[] {
  const home = homedir()
  const candidates = [
    join(codexHome(), 'bin', 'codex'),
    join(home, '.local', 'bin', 'codex'),
    join(home, '.npm-global', 'bin', 'codex'),
    '/usr/local/bin/codex',
    '/opt/homebrew/bin/codex',
  ]
  // The VS Code extension ships a per-platform binary under a versioned folder.
  for (const root of [join(home, '.vscode', 'extensions'), join(home, '.vscode-insiders', 'extensions')]) {
    try {
      const versions = readdirSync(root)
        .filter((entry) => entry.startsWith('openai.chatgpt-'))
        .sort()
        .reverse()
      for (const entry of versions) {
        for (const target of ['macos-aarch64', 'macos-x86_64', 'linux-x86_64', 'linux-aarch64']) {
          candidates.push(join(root, entry, 'bin', target, 'codex'))
        }
      }
    } catch {
      // VS Code is optional.
    }
  }
  return candidates
}

interface CodexExecutable {
  path: string
  version: string
}

async function findCodexCli(): Promise<CodexExecutable | undefined> {
  const attempts = ['codex', ...codexCandidates()]
  for (const candidate of attempts) {
    try {
      const { stdout } = await execFileAsync(candidate, ['--version'], {
        timeout: CODEX_PROBE_TIMEOUT_MS,
      })
      return { path: candidate, version: stdout.trim() || 'unknown version' }
    } catch {
      // Try the next known location.
    }
  }
  return undefined
}

async function probeCodexCli(): Promise<CodexIntegrationCheck> {
  const executable = await findCodexCli()
  if (executable) {
    return {
      id: 'codex-cli',
      ok: true,
      detail: executable.path === 'codex' ? `codex · ${executable.version}` : `${executable.path} · ${executable.version}`,
    }
  }
  return {
    id: 'codex-cli',
    ok: false,
    detail: `codex not found on PATH or in a known Codex location`,
  }
}

/** Finds the source skill and workspace command used by the local developer app. */
function relayInstallation(): { workspaceRoot: string; skillSource: string } | undefined {
  const roots = [
    process.cwd(),
    // electron-vite dev emits main code under apps/desktop/out/main.
    join(import.meta.dirname, '../../../../'),
  ]
  for (const workspaceRoot of roots) {
    const skillSource = join(workspaceRoot, 'integrations', 'codex', 'skills', 'relay', 'SKILL.md')
    if (existsSync(skillSource) && existsSync(join(workspaceRoot, 'package.json'))) {
      return { workspaceRoot, skillSource }
    }
  }
  return undefined
}

function probeRelayMcp(): CodexIntegrationCheck {
  const candidates = [join(codexHome(), 'config.toml'), join(codexHome(), 'mcp.json')]
  for (const candidate of candidates) {
    if (!existsSync(candidate)) continue
    try {
      const contents = readFileSync(candidate, 'utf8')
      // Either a TOML [mcp_servers.relay] table or a JSON "relay" key.
      if (/mcp_servers\.relay|"relay"\s*:/i.test(contents)) {
        return { id: 'relay-mcp', ok: true, detail: candidate }
      }
    } catch {
      // Unreadable config is reported as not configured below.
    }
  }
  return { id: 'relay-mcp', ok: false, detail: join(codexHome(), 'config.toml') }
}

function probeRelaySkill(): CodexIntegrationCheck {
  const primary = join(codexHome(), 'skills', 'relay', 'SKILL.md')
  const candidates = [primary, join(homedir(), '.config', 'codex', 'skills', 'relay', 'SKILL.md')]
  for (const candidate of candidates) {
    if (existsSync(candidate)) return { id: 'relay-skill', ok: true, detail: candidate }
  }
  return { id: 'relay-skill', ok: false, detail: primary }
}

interface EventRow {
  id: string
  run_id: string
  step_id: string | null
  worker_session_id: string | null
  seq: number
  timestamp: string
  type: string
  data_json: string
  native_event_json: string | null
}

export class DesktopDataSource {
  readonly #database: DatabaseSync
  readonly #settingsPath: string
  readonly #profilesPath: string
  readonly #adapters: AgentAdapter[] = [
    new DeepSeekAdapter(),
    new AntigravityAdapter(),
    new KimiAdapter(),
    new GeminiAdapter(),
    new ZaiAdapter(),
  ]
  readonly #optionsCache = new Map<string, { at: number; authRequired: boolean; value: RuntimeOptions }>()
  readonly #optionsCachePath: string
  #runtimes: Runtime[] = []
  #profiles: AgentProfile[] = []
  #diagnostics: string[] = []
  #snapshotCache: { at: number; value: DesktopSnapshot } | undefined
  #codexCache: { at: number; value: CodexIntegrationStatus } | undefined

  constructor(databasePath: string, settingsPath: string) {
    this.#database = new DatabaseSync(databasePath)
    this.#database.exec('PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;')
    this.#database.exec(`
      CREATE TABLE IF NOT EXISTS relay_events (
        id TEXT NOT NULL UNIQUE,
        run_id TEXT NOT NULL,
        step_id TEXT,
        worker_session_id TEXT,
        seq INTEGER NOT NULL CHECK (seq > 0),
        timestamp TEXT NOT NULL,
        type TEXT NOT NULL,
        data_json TEXT NOT NULL,
        native_event_json TEXT,
        PRIMARY KEY (run_id, seq)
      );
      CREATE INDEX IF NOT EXISTS relay_events_timestamp_idx ON relay_events(timestamp);
      CREATE TABLE IF NOT EXISTS host_sessions (
        id TEXT PRIMARY KEY,
        native_session_id TEXT NOT NULL UNIQUE,
        display_name TEXT NOT NULL,
        data_json TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS host_sessions_updated_idx ON host_sessions(updated_at DESC);
      CREATE TABLE IF NOT EXISTS relay_control_commands (
        id TEXT PRIMARY KEY,
        type TEXT NOT NULL,
        worker_session_id TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        error TEXT
      );
      CREATE INDEX IF NOT EXISTS relay_control_pending_idx
        ON relay_control_commands(status, created_at);
    `)
    const eventColumns = this.#database
      .prepare('PRAGMA table_info(relay_events)')
      .all() as Array<{ name: string }>
    if (!eventColumns.some((column) => column.name === 'step_id')) {
      this.#database.exec('ALTER TABLE relay_events ADD COLUMN step_id TEXT;')
    }
    this.#settingsPath = settingsPath
    this.#profilesPath = join(dirname(settingsPath), 'profiles.json')
    this.#optionsCachePath = join(dirname(settingsPath), 'runtime-options.json')
  }

  async initialize(): Promise<void> {
    const detections = await Promise.all(this.#adapters.map((adapter) => adapter.detect()))
    this.#runtimes = detections.flatMap((detection) => detection.runtimes)
    this.#diagnostics = detections.flatMap((detection) => detection.diagnostics)
    const defaults = defaultProfiles(this.#runtimes)
    if (!existsSync(this.#profilesPath)) {
      this.#profiles = defaults
      return
    }
    try {
      const stored = agentProfileSchema.array().parse(
        JSON.parse(readFileSync(this.#profilesPath, 'utf8')),
      )
      const availableRuntimeIds = new Set(this.#runtimes.map((runtime) => runtime.id))
      const storedById = new Map(
        stored.filter((profile) => availableRuntimeIds.has(profile.runtimeId)).map((profile) => [profile.id, profile]),
      )
      this.#profiles = defaults.map((profile) => storedById.get(profile.id) ?? profile)
      for (const profile of storedById.values()) {
        if (!this.#profiles.some((candidate) => candidate.id === profile.id)) this.#profiles.push(profile)
      }
    } catch {
      this.#profiles = defaults
      this.#diagnostics.push('Invalid profiles.json; using built-in profiles.')
    }
  }

  /**
   * Menu bar switches are booleans the user set through the tray. Anything else
   * in the file is ignored rather than trusted, so a hand-edited settings file
   * cannot put the app into a state the UI has no way to show.
   */
  #menuBarPrefs(value: unknown): DesktopMenuBarPrefs {
    const record = value && typeof value === 'object' ? (value as Record<string, unknown>) : {}
    return { hideDockIcon: record.hideDockIcon === true }
  }

  settings(): DesktopSettings {
    if (existsSync(this.#settingsPath)) {
      try {
        const parsed = JSON.parse(readFileSync(this.#settingsPath, 'utf8')) as {
          policy?: unknown
          workspaceOverrides?: Record<string, unknown>
          menuBar?: unknown
          onboardingCompletedAt?: unknown
        }
        const workspaceOverrides: Record<string, RelayPolicyOverride> = {}
        for (const [workspace, override] of Object.entries(parsed.workspaceOverrides ?? {})) {
          workspaceOverrides[workspace] = relayPolicyOverrideSchema.parse(override)
        }
        return {
          policy: relayPolicySchema.parse(parsed.policy),
          workspaceOverrides,
          menuBar: this.#menuBarPrefs(parsed.menuBar),
          ...(typeof parsed.onboardingCompletedAt === 'string'
            ? { onboardingCompletedAt: parsed.onboardingCompletedAt }
            : {}),
        }
      } catch {
        // Invalid user settings fall back to conservative defaults.
      }
    }
    return { policy: defaultPolicy, workspaceOverrides: {}, menuBar: { hideDockIcon: false } }
  }

  /** Called by the menu bar; the window picks the change up on its next poll. */
  saveMenuBarPrefs(prefs: DesktopMenuBarPrefs): DesktopSettings {
    return this.saveSettings({ ...this.settings(), menuBar: prefs })
  }

  saveSettings(input: DesktopSettings): DesktopSettings {
    const workspaceOverrides: Record<string, RelayPolicyOverride> = {}
    for (const [workspace, override] of Object.entries(input.workspaceOverrides)) {
      workspaceOverrides[workspace] = relayPolicyOverrideSchema.parse(override)
    }
    const settings: DesktopSettings = {
      policy: relayPolicySchema.parse(input.policy),
      workspaceOverrides,
      menuBar: this.#menuBarPrefs(input.menuBar),
      ...(input.onboardingCompletedAt ? { onboardingCompletedAt: input.onboardingCompletedAt } : {}),
    }
    writeFileSync(this.#settingsPath, `${JSON.stringify(settings, null, 2)}\n`, 'utf8')
    return settings
  }

  /** Marks first-run onboarding finished so later launches skip it (docs/ui.md 22.3). */
  completeOnboarding(): DesktopSettings {
    return this.saveSettings({ ...this.settings(), onboardingCompletedAt: new Date().toISOString() })
  }

  /**
   * Model and reasoning choices for a runtime, as reported by its CLI. Cached
   * briefly because the probe spawns the CLI with --help, and the renderer asks
   * whenever the user opens an agent editor.
   */

  #readOptionsCache(): Record<string, { at: number; authRequired: boolean; value: RuntimeOptions }> {
    try {
      if (!existsSync(this.#optionsCachePath)) return {}
      const parsed: unknown = JSON.parse(readFileSync(this.#optionsCachePath, 'utf8'))
      return parsed && typeof parsed === 'object'
        ? (parsed as Record<string, { at: number; authRequired: boolean; value: RuntimeOptions }>)
        : {}
    } catch {
      // A corrupt cache is not worth failing over; the probe runs again.
      return {}
    }
  }

  #writeOptionsCache(
    runtimeId: string,
    entry: { at: number; authRequired: boolean; value: RuntimeOptions },
  ): void {
    try {
      const next = { ...this.#readOptionsCache(), [runtimeId]: entry }
      writeFileSync(this.#optionsCachePath, `${JSON.stringify(next, null, 2)}\n`, 'utf8')
    } catch {
      // Caching is best-effort; a read-only home directory must not break the UI.
    }
  }

  /**
   * A result produced without an API key is only valid while there is still no
   * key: once one appears the provider may now answer over HTTP.
   */
  #cacheStillValid(entry: { authRequired: boolean }): boolean {
    if (!entry.authRequired) return true
    return !Object.values(httpModelQueries).some((query) =>
      query.keyEnv.some((name) => {
        const value = process.env[name]
        return Boolean(value && value.trim())
      }),
    )
  }

  async runtimeOptions(runtimeId: string): Promise<RuntimeOptions> {
    const cached = this.#optionsCache.get(runtimeId)
    if (cached && Date.now() - cached.at < OPTIONS_CACHE_MS && this.#cacheStillValid(cached)) {
      return cached.value
    }
    // Fall back to the on-disk cache: an HTTP-derived list is slow and
    // key-gated, so it should outlive the process.
    const stored = this.#readOptionsCache()[runtimeId]
    if (stored && Date.now() - stored.at < OPTIONS_DISK_TTL_MS && this.#cacheStillValid(stored)) {
      this.#optionsCache.set(runtimeId, stored)
      return stored.value
    }
    const runtime = this.#runtimes.find((candidate) => candidate.id === runtimeId)
    const adapter = runtime
      ? this.#adapters.find((candidate) => candidate.id === runtime.adapterId)
      : undefined
    if (!runtime || !adapter) {
      return {
        runtimeId,
        adapterId: runtime?.adapterId ?? 'unknown',
        models: [],
        levels: [],
        source: 'default',
        diagnostics: ['Unknown runtime; Relay cannot read its options'],
      }
    }
    let value: RuntimeOptions
    if (adapter.reportOptions) {
      value = await adapter.reportOptions(runtimeId)
    } else {
      value = {
        runtimeId,
        adapterId: adapter.id,
        models: [],
        levels: [],
        source: 'default',
        diagnostics: [`${adapter.id} exposes no model or reasoning options`],
      }
    }
    const entry = {
      at: Date.now(),
      // Remember if this ran without a credential, so adding a key re-probes.
      authRequired: value.diagnostics.some((line) => line.includes('official API')),
      value,
    }
    this.#optionsCache.set(runtimeId, entry)
    this.#writeOptionsCache(runtimeId, entry)
    return value
  }

  /**
   * Probing spawns the Codex CLI, so the menu bar reuses a recent result. The
   * window and the explicit "re-check"/install paths pass 0 and always probe.
   */
  async codexIntegration(maxAgeMs = 0): Promise<CodexIntegrationStatus> {
    const now = Date.now()
    if (this.#codexCache && now - this.#codexCache.at <= maxAgeMs) return this.#codexCache.value
    const checks = [await probeCodexCli(), probeRelayMcp(), probeRelaySkill()]
    const value: CodexIntegrationStatus = {
      checks,
      configured: checks.every((check) => check.ok),
    }
    this.#codexCache = { at: now, value }
    return value
  }

  /**
   * Installs the two parts Codex needs to discover Relay: the stdio MCP server
   * through Codex's own CLI and the shipped Relay skill in CODEX_HOME. Both
   * operations are idempotent: an existing user configuration is left intact.
   */
  async installCodexIntegration(): Promise<CodexIntegrationInstallResult> {
    const messages: string[] = []
    const installation = relayInstallation()
    const executable = await findCodexCli()

    if (!installation) {
      return {
        status: await this.codexIntegration(),
        messages: ['Relay installation files are not available in this build'],
      }
    }
    if (!executable) {
      return {
        status: await this.codexIntegration(),
        messages: ['Codex executable was not found, so Relay cannot configure its MCP server'],
      }
    }

    const skillTarget = join(codexHome(), 'skills', 'relay', 'SKILL.md')
    if (!existsSync(skillTarget)) {
      try {
        mkdirSync(dirname(skillTarget), { recursive: true })
        copyFileSync(installation.skillSource, skillTarget)
        messages.push('Installed the Relay skill for Codex')
      } catch (error) {
        messages.push(`Could not install the Relay skill: ${error instanceof Error ? error.message : String(error)}`)
      }
    } else {
      messages.push('Relay skill is already installed')
    }

    if (!probeRelayMcp().ok) {
      try {
        await execFileAsync(executable.path, [
          'mcp',
          'add',
          'relay',
          '--',
          'corepack',
          'pnpm',
          '--dir',
          installation.workspaceRoot,
          'mcp:dev',
        ], { timeout: CODEX_PROBE_TIMEOUT_MS })
        messages.push('Configured the Relay MCP server for Codex')
      } catch (error) {
        messages.push(`Could not configure Relay MCP: ${error instanceof Error ? error.message : String(error)}`)
      }
    } else {
      messages.push('Relay MCP is already configured')
    }

    return { status: await this.codexIntegration(), messages }
  }

  saveProfile(input: AgentProfile): AgentProfile {
    const profile = agentProfileSchema.parse(input)
    if (!this.#runtimes.some((runtime) => runtime.id === profile.runtimeId)) {
      throw new Error(`Unknown runtime ${profile.runtimeId}`)
    }
    const index = this.#profiles.findIndex((candidate) => candidate.id === profile.id)
    if (index === -1) this.#profiles.push(profile)
    else this.#profiles[index] = profile
    writeFileSync(this.#profilesPath, `${JSON.stringify(this.#profiles, null, 2)}\n`, 'utf8')
    return profile
  }

  /** Projects every stored run; shared by snapshot and session-level controls. */
  #projectedRuns(): DesktopRunView[] {
    const runIds = this.#database
      .prepare('SELECT DISTINCT run_id FROM relay_events ORDER BY timestamp ASC')
      .all()
      .map((row) => String(row.run_id))
    const eventQuery = this.#database.prepare(
      'SELECT * FROM relay_events WHERE run_id = ? ORDER BY seq ASC',
    )
    return runIds.map((runId) => {
      const events = (eventQuery.all(runId) as unknown as EventRow[]).map((row) =>
        relayEventSchema.parse({
          id: row.id,
          runId: row.run_id,
          ...(row.step_id ? { stepId: row.step_id } : {}),
          ...(row.worker_session_id ? { workerSessionId: row.worker_session_id } : {}),
          seq: row.seq,
          timestamp: row.timestamp,
          type: row.type,
          data: JSON.parse(row.data_json) as unknown,
          ...(row.native_event_json
            ? { nativeEvent: JSON.parse(row.native_event_json) as unknown }
            : {}),
        }),
      )
      const projection = projectRun(events)
      return {
        run: projection.run,
        steps: projection.steps,
        workers: projection.workers,
        events,
      }
    })
  }

  /**
   * The window polls every two seconds and the menu bar refreshes on its own
   * cadence. A short shared cache keeps both readers from re-projecting every
   * stored event independently; callers that need a guaranteed-fresh projection
   * pass 0.
   */
  snapshot(maxAgeMs = 0): DesktopSnapshot {
    const now = Date.now()
    if (this.#snapshotCache && now - this.#snapshotCache.at <= maxAgeMs) {
      return this.#snapshotCache.value
    }
    const value = this.#projectSnapshot()
    this.#snapshotCache = { at: now, value }
    return value
  }

  #projectSnapshot(): DesktopSnapshot {
    const sessions = this.#database
      .prepare('SELECT data_json FROM host_sessions ORDER BY updated_at DESC')
      .all()
      .map((row) => hostSessionSchema.parse(JSON.parse(String(row.data_json))))
    const runs = this.#projectedRuns()
    return {
      sessions,
      runs: runs.sort((left, right) => right.run.createdAt.localeCompare(left.run.createdAt)),
      runtimes: this.#runtimes,
      profiles: this.#profiles,
      diagnostics: this.#diagnostics,
      settings: this.settings(),
      refreshedAt: new Date().toISOString(),
    }
  }

  cancelWorker(workerSessionId: string): { accepted: boolean; message: string } {
    this.#database
      .prepare(`
        INSERT INTO relay_control_commands (
          id, type, worker_session_id, status, created_at, error
        ) VALUES (?, 'cancel-worker', ?, 'pending', ?, NULL)
      `)
      .run(randomUUID(), workerSessionId, new Date().toISOString())
    return { accepted: true, message: 'Cancellation requested' }
  }

  /** "Stop all workers" from the session contextual menu (docs/ui.md 19). */
  cancelSessionWorkers(hostSessionId: string): { accepted: boolean; count: number } {
    const active = new Set<string>()
    for (const view of this.#projectedRuns()) {
      if (view.run.hostSessionId !== hostSessionId) continue
      for (const worker of view.workers) {
        if (worker.status === 'running' || worker.status === 'starting') active.add(worker.id)
      }
    }
    for (const workerSessionId of active) this.cancelWorker(workerSessionId)
    return { accepted: true, count: active.size }
  }

  close(): void {
    this.#database.close()
    for (const adapter of this.#adapters) void adapter.dispose()
  }
}
