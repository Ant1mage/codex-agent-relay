import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { promisify } from 'node:util'
import { AntigravityAdapter } from '@relay/adapter-antigravity'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { GeminiAdapter } from '@relay/adapter-gemini'
import { KimiAdapter } from '@relay/adapter-kimi'
import { ZaiAdapter } from '@relay/adapter-zai'
import type { AgentAdapter } from '@relay/adapter-sdk'
import { RelayConfigStore, relayHome } from '@relay/config'
import type { AgentProfile, Runtime } from '@relay/protocol'
import type { CodexAction, CodexCheck, CodexStatus, InstallResult } from '@relay/relay-api'

/**
 * Everything the daemon knows about this machine that is not in the event log:
 * which runtimes exist, which Agent Profiles Codex sees, and whether the Codex
 * side of the integration is present, current and removable.
 *
 * The Codex integration is a real lifecycle (docs/codex-integration.md):
 * detect, install, update, repair and remove over the plugin marketplace plus
 * the MCP server entry, never a one-shot copy.
 */

const execFileAsync = promisify(execFile)
const PROBE_TIMEOUT_MS = 10_000
const MARKETPLACE_NAME = 'relay'
const PLUGIN_NAME = 'relay'

export interface Environment {
  runtimes: Runtime[]
  profiles: AgentProfile[]
  diagnostics: string[]
}

function adapters(): AgentAdapter[] {
  return [
    new DeepSeekAdapter(),
    new AntigravityAdapter(),
    new KimiAdapter(),
    new GeminiAdapter(),
    new ZaiAdapter(),
  ]
}

export async function detectEnvironment(
  config: RelayConfigStore = new RelayConfigStore(),
): Promise<Environment> {
  const detected = await Promise.all(adapters().map((adapter) => adapter.detect()))
  const runtimes = detected.flatMap((result) => result.runtimes)
  const diagnostics = detected.flatMap((result) => result.diagnostics)
  const profiles = config.read({ runtimes }).profiles
  return { runtimes, profiles, diagnostics }
}

/** Model and reasoning values a runtime's CLI advertises, for the profile editor. */
export async function runtimeOptions(runtimeId: string, runtimes: Runtime[]) {
  const runtime = runtimes.find((candidate) => candidate.id === runtimeId)
  const adapter = runtime ? adapters().find((candidate) => candidate.id === runtime.adapterId) : undefined
  if (!adapter) {
    return {
      runtimeId,
      adapterId: runtime?.adapterId ?? 'unknown',
      models: [],
      levels: [],
      source: 'default' as const,
      diagnostics: ['Unknown runtime; Relay cannot read its options'],
    }
  }
  if (!adapter.reportOptions) {
    return {
      runtimeId,
      adapterId: adapter.id,
      models: [],
      levels: [],
      source: 'default' as const,
      diagnostics: [`${adapter.id} exposes no model or reasoning options`],
    }
  }
  return adapter.reportOptions(runtimeId)
}

/* ------------------------------------------------------------------ */
/* Codex integration                                                   */
/* ------------------------------------------------------------------ */

export function codexHome(): string {
  const configured = process.env.CODEX_HOME
  return configured && configured.length > 0 ? configured : join(homedir(), '.codex')
}

/** Where Relay materialises the plugin it asks Codex to install. */
export function marketplaceRoot(): string {
  return process.env.RELAY_CODEX_PLUGIN_ROOT ?? join(relayHome(), 'codex-plugin')
}

export interface RelaySources {
  root: string
  pluginManifest: string
  hooks: string
  skill: string
}

/**
 * Where the shipped integration lives. Packaged builds place the same tree next
 * to the app bundle; source checkouts are found by walking up from this file.
 */
export function relaySources(): RelaySources | undefined {
  const bases: string[] = []
  const override = process.env.RELAY_INSTALL_ROOT
  if (override) bases.push(join(override, 'integrations', 'codex'), override)
  bases.push(join(import.meta.dirname, 'codex'))
  let directory = import.meta.dirname
  for (let depth = 0; depth < 6; depth += 1) {
    bases.push(join(directory, 'integrations', 'codex'))
    const parent = dirname(directory)
    if (parent === directory) break
    directory = parent
  }
  for (const base of bases) {
    const pluginManifest = join(base, 'plugin.json')
    const hooks = join(base, 'hooks', 'hooks.json')
    const skill = join(base, 'skills', 'relay', 'SKILL.md')
    if (existsSync(pluginManifest) && existsSync(hooks) && existsSync(skill)) {
      return { root: dirname(base), pluginManifest, hooks, skill }
    }
  }
  return undefined
}

export interface CodexExecutable {
  path: string
  version: string
}

function codexCandidates(): string[] {
  const home = homedir()
  const candidates = [
    join(codexHome(), 'bin', 'codex'),
    join(home, '.local', 'bin', 'codex'),
    join(home, '.npm-global', 'bin', 'codex'),
    '/usr/local/bin/codex',
    '/opt/homebrew/bin/codex',
  ]
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

export async function findCodexCli(): Promise<CodexExecutable | undefined> {
  for (const candidate of ['codex', ...codexCandidates()]) {
    try {
      const { stdout } = await execFileAsync(candidate, ['--version'], { timeout: PROBE_TIMEOUT_MS })
      return { path: candidate, version: stdout.trim() || 'unknown version' }
    } catch {
      // Try the next known location.
    }
  }
  return undefined
}

async function codex(executable: CodexExecutable, args: string[]): Promise<{ ok: boolean; out: string }> {
  try {
    const { stdout, stderr } = await execFileAsync(executable.path, args, { timeout: PROBE_TIMEOUT_MS * 6 })
    return { ok: true, out: `${stdout}${stderr}` }
  } catch (error) {
    const failure = error as { stdout?: string; stderr?: string }
    const message = error instanceof Error ? error.message : String(error)
    return { ok: false, out: `${failure.stdout ?? ''}${failure.stderr ?? message}` }
  }
}

/**
 * The MCP entry Codex should run.
 *
 * A bundled daemon (built artifact) points Codex at the bundled MCP server, so a
 * release install never depends on corepack, pnpm or tsx. A source checkout runs
 * the workspace command instead, which keeps edits live during development.
 */
export function desiredMcpCommand(): { command: string; args: string[] } {
  const bundled = process.env.RELAY_MCP_ENTRY ?? join(import.meta.dirname, '..', 'mcp', 'stdio.js')
  const fromSource = import.meta.dirname.endsWith('/src')
  if ((!fromSource || process.env.RELAY_MCP_ENTRY) && existsSync(bundled)) {
    return { command: process.execPath, args: [bundled] }
  }
  const sources = relaySources()
  return { command: 'corepack', args: ['pnpm', '--dir', sources?.root ?? process.cwd(), 'mcp:dev'] }
}

function mcpEntryFile(): string | undefined {
  const { command, args } = desiredMcpCommand()
  return command === process.execPath && args[0] ? args[0] : undefined
}

/** Reads just the relay table out of config.toml; enough for a status check. */
export function readMcpEntry(contents: string): { command?: string; args: string[] } | undefined {
  const lines = contents.split('\n')
  const start = lines.findIndex((line) => line.trim() === '[mcp_servers.relay]')
  if (start === -1) return undefined
  const entry: { command?: string; args: string[] } = { args: [] }
  for (const line of lines.slice(start + 1)) {
    const trimmed = line.trim()
    if (trimmed.startsWith('[')) break
    const command = /^command\s*=\s*"(.*)"$/.exec(trimmed)
    if (command?.[1]) entry.command = command[1]
    const args = /^args\s*=\s*\[(.*)\]$/.exec(trimmed)
    if (args?.[1] !== undefined) {
      entry.args = args[1]
        .split(',')
        .map((part) => part.trim().replace(/^"|"$/g, ''))
        .filter((part) => part.length > 0)
    }
  }
  return entry
}

export function relayPluginVersion(): string {
  const sources = relaySources()
  if (!sources) return '0.0.0'
  const hash = createHash('sha256')
  hash.update(readFileSync(sources.skill, 'utf8'))
  hash.update(readFileSync(sources.hooks, 'utf8'))
  return `0.0.0+${hash.digest('hex').slice(0, 8)}`
}

/** Parses `codex plugin list` output for our own plugin. */
export function parseInstalledPlugin(
  output: string,
): { status?: string | undefined; source?: string | undefined } | undefined {
  for (const line of output.split('\n')) {
    const fields = line.split(/\s{2,}/).map((field) => field.trim())
    if (fields[0] !== `${PLUGIN_NAME}@${MARKETPLACE_NAME}`) continue
    return { status: fields[1], source: fields[fields.length - 1] }
  }
  return undefined
}

/** Parses `codex plugin marketplace list` output for our marketplace's root. */
export function parseMarketplaceRoot(output: string): string | undefined {
  for (const line of output.split('\n')) {
    const fields = line.split(/\s{2,}/).map((field) => field.trim())
    if (fields[0] === MARKETPLACE_NAME && fields[1]) return fields[1]
  }
  return undefined
}

async function probeCodexCli(executable: CodexExecutable | undefined): Promise<CodexCheck> {
  if (!executable) {
    return {
      id: 'codex-cli',
      ok: false,
      status: 'missing',
      detail: 'codex not found on PATH or in a known Codex location',
      hint: '安装 Codex CLI 或 VS Code 扩展后重新检测',
    }
  }
  const detail =
    executable.path === 'codex' ? `codex · ${executable.version}` : `${executable.path} · ${executable.version}`
  return { id: 'codex-cli', ok: true, status: 'ok', detail }
}

function probeRelayMcp(): CodexCheck {
  const configFile = join(codexHome(), 'config.toml')
  if (!existsSync(configFile)) {
    return { id: 'relay-mcp', ok: false, status: 'missing', detail: configFile, hint: '运行"安装到 Codex"' }
  }
  const entry = readMcpEntry(readFileSync(configFile, 'utf8'))
  if (!entry?.command) {
    return { id: 'relay-mcp', ok: false, status: 'missing', detail: configFile, hint: '运行"安装到 Codex"' }
  }
  const desired = desiredMcpCommand()
  const sameArgs =
    entry.args.length === desired.args.length && entry.args.every((arg, index) => arg === desired.args[index])
  const entryFile = mcpEntryFile()
  const fileExists = entryFile ? existsSync(entryFile) : true
  if (entry.command === desired.command && sameArgs && fileExists) {
    return { id: 'relay-mcp', ok: true, status: 'ok', detail: `${entry.command} ${entry.args.join(' ')}` }
  }
  return {
    id: 'relay-mcp',
    ok: false,
    status: 'stale',
    detail: `已配置: ${entry.command} ${entry.args.join(' ')}`,
    hint: fileExists
      ? `期望: ${desired.command} ${desired.args.join(' ')} — 运行"修复"`
      : `入口不存在: ${entryFile} — 运行"修复"`,
  }
}

function probeRelaySkill(): CodexCheck {
  const installed = join(marketplaceRoot(), 'plugins', PLUGIN_NAME, 'skills', PLUGIN_NAME, 'SKILL.md')
  const legacy = join(codexHome(), 'skills', PLUGIN_NAME, 'SKILL.md')
  const sources = relaySources()

  if (existsSync(legacy) && !existsSync(installed)) {
    return {
      id: 'relay-skill',
      ok: false,
      status: 'legacy',
      detail: `旧版手工复制: ${legacy}`,
      hint: '运行"修复"迁移到插件安装',
    }
  }
  if (!existsSync(installed)) {
    return { id: 'relay-skill', ok: false, status: 'missing', detail: installed, hint: '运行"安装到 Codex"' }
  }
  if (!sources) return { id: 'relay-skill', ok: true, status: 'ok', detail: installed }
  if (readFileSync(sources.skill, 'utf8') !== readFileSync(installed, 'utf8')) {
    return {
      id: 'relay-skill',
      ok: false,
      status: 'outdated',
      detail: '已安装的 skill 与当前 Relay 版本不一致',
      hint: '运行"更新"',
    }
  }
  return { id: 'relay-skill', ok: true, status: 'ok', detail: installed }
}

async function probeRelayPlugin(executable: CodexExecutable | undefined): Promise<CodexCheck> {
  if (!executable) {
    return { id: 'relay-plugin', ok: false, status: 'missing', detail: '需要 codex CLI 才能检测插件' }
  }
  const listed = await codex(executable, ['plugin', 'list'])
  if (!listed.ok) {
    return {
      id: 'relay-plugin',
      ok: false,
      status: 'missing',
      detail: 'codex plugin list 失败',
      hint: listed.out.slice(0, 200),
    }
  }
  const installed = parseInstalledPlugin(listed.out)
  if (!installed?.status?.includes('installed')) {
    return {
      id: 'relay-plugin',
      ok: false,
      status: 'missing',
      detail: `${PLUGIN_NAME}@${MARKETPLACE_NAME} 未安装`,
      hint: '运行"安装到 Codex"',
    }
  }
  if (!installed.status.includes('enabled')) {
    return {
      id: 'relay-plugin',
      ok: false,
      status: 'stale',
      detail: '插件已安装但被禁用',
      hint: '在 Codex 中启用 relay 插件',
    }
  }
  return {
    id: 'relay-plugin',
    ok: true,
    status: 'ok',
    detail: installed.source ?? `${PLUGIN_NAME}@${MARKETPLACE_NAME}`,
  }
}

function probeRelayHooks(): CodexCheck {
  const hooksFile = join(marketplaceRoot(), 'plugins', PLUGIN_NAME, 'hooks', 'hooks.json')
  const sources = relaySources()
  if (!existsSync(hooksFile)) {
    return {
      id: 'relay-hooks',
      ok: false,
      status: 'missing',
      detail: hooksFile,
      hint: 'hooks 随插件安装；运行"安装到 Codex"',
    }
  }
  let events: string[] = []
  try {
    const parsed = JSON.parse(readFileSync(hooksFile, 'utf8')) as { hooks?: Record<string, unknown> }
    events = Object.keys(parsed.hooks ?? {})
  } catch {
    return { id: 'relay-hooks', ok: false, status: 'stale', detail: 'hooks.json 无法解析', hint: '运行"修复"' }
  }
  if (sources && readFileSync(sources.hooks, 'utf8') !== readFileSync(hooksFile, 'utf8')) {
    return { id: 'relay-hooks', ok: false, status: 'outdated', detail: 'hooks 与当前 Relay 版本不一致', hint: '运行"更新"' }
  }
  return {
    id: 'relay-hooks',
    ok: true,
    status: 'ok',
    detail: `${events.join(' / ')} · 首次使用需在 Codex 中信任`,
  }
}

export async function codexStatus(): Promise<CodexStatus> {
  const executable = await findCodexCli()
  const checks = [
    await probeCodexCli(executable),
    probeRelayMcp(),
    probeRelaySkill(),
    await probeRelayPlugin(executable),
    probeRelayHooks(),
  ]
  return { checks, configured: checks.every((check) => check.ok) }
}

/** Writes the plugin tree Relay asks Codex to install from. */
function materialisePlugin(): { root: string; version: string } {
  const sources = relaySources()
  if (!sources) throw new Error('这个构建里没有 Relay 集成源文件，无法安装到 Codex')
  const root = marketplaceRoot()
  const plugin = join(root, 'plugins', PLUGIN_NAME)
  const version = relayPluginVersion()

  rmSync(root, { recursive: true, force: true })
  mkdirSync(join(root, '.agents', 'plugins'), { recursive: true })
  mkdirSync(join(plugin, '.codex-plugin'), { recursive: true })

  writeFileSync(
    join(root, '.agents', 'plugins', 'marketplace.json'),
    `${JSON.stringify(
      {
        name: MARKETPLACE_NAME,
        interface: { displayName: 'Relay (local)' },
        plugins: [
          {
            name: PLUGIN_NAME,
            source: { source: 'local', path: `./plugins/${PLUGIN_NAME}` },
            policy: { installation: 'AVAILABLE', authentication: 'ON_INSTALL' },
            category: 'Developer Tools',
          },
        ],
      },
      null,
      2,
    )}\n`,
    'utf8',
  )

  const manifest = JSON.parse(readFileSync(sources.pluginManifest, 'utf8')) as { description?: string }
  writeFileSync(
    join(plugin, '.codex-plugin', 'plugin.json'),
    `${JSON.stringify(
      {
        name: PLUGIN_NAME,
        version,
        description: manifest.description ?? 'Delegate bounded Codex tasks to local coding agents.',
        skills: './skills/',
        hooks: './hooks/hooks.json',
        interface: {
          displayName: 'Relay',
          shortDescription: 'Delegate tasks to local coding agents',
          developerName: 'Relay',
          category: 'Developer Tools',
          capabilities: ['Read', 'Write'],
        },
      },
      null,
      2,
    )}\n`,
    'utf8',
  )
  cpSync(join(sources.hooks, '..'), join(plugin, 'hooks'), { recursive: true })
  cpSync(join(sources.skill, '..', '..'), join(plugin, 'skills'), { recursive: true })
  return { root, version }
}

/**
 * Installs (or repairs/updates) the Codex side: a local plugin marketplace, the
 * plugin itself, and the MCP entry that both the hooks and the tools call.
 * Every step is idempotent and reported individually.
 */
export async function installCodex(): Promise<InstallResult> {
  const messages: string[] = []
  const executable = await findCodexCli()
  if (!executable) {
    return { status: await codexStatus(), messages: ['未找到 codex CLI，无法配置 Codex 集成'] }
  }
  let materialised: { root: string; version: string }
  try {
    materialised = materialisePlugin()
    messages.push(`已生成插件 ${materialised.version} → ${materialised.root}`)
  } catch (error) {
    return { status: await codexStatus(), messages: [error instanceof Error ? error.message : String(error)] }
  }

  const marketplaces = await codex(executable, ['plugin', 'marketplace', 'list'])
  const currentRoot = marketplaces.ok ? parseMarketplaceRoot(marketplaces.out) : undefined
  if (currentRoot && currentRoot !== materialised.root) {
    await codex(executable, ['plugin', 'marketplace', 'remove', MARKETPLACE_NAME])
    messages.push(`已移除指向旧路径的 marketplace: ${currentRoot}`)
  }
  if (currentRoot !== materialised.root) {
    const added = await codex(executable, ['plugin', 'marketplace', 'add', materialised.root])
    messages.push(added.ok ? '已注册本地 marketplace' : `marketplace 注册失败: ${added.out.slice(0, 200)}`)
  }

  const listed = await codex(executable, ['plugin', 'list'])
  const installed = listed.ok ? parseInstalledPlugin(listed.out) : undefined
  if (installed?.status?.includes('installed')) {
    // Removing first refreshes the cached copy, so a newer plugin version lands.
    await codex(executable, ['plugin', 'remove', `${PLUGIN_NAME}@${MARKETPLACE_NAME}`])
  }
  const added = await codex(executable, ['plugin', 'add', `${PLUGIN_NAME}@${MARKETPLACE_NAME}`])
  messages.push(added.ok ? '已安装 relay 插件（skill + hooks）' : `插件安装失败: ${added.out.slice(0, 200)}`)

  const desired = desiredMcpCommand()
  const configFile = join(codexHome(), 'config.toml')
  const entry = existsSync(configFile) ? readMcpEntry(readFileSync(configFile, 'utf8')) : undefined
  const sameArgs =
    entry?.args.length === desired.args.length && entry.args.every((arg, index) => arg === desired.args[index])
  if (!entry || entry.command !== desired.command || !sameArgs) {
    if (entry) await codex(executable, ['mcp', 'remove', PLUGIN_NAME])
    const mcpAdded = await codex(executable, ['mcp', 'add', PLUGIN_NAME, '--', desired.command, ...desired.args])
    messages.push(mcpAdded.ok ? '已配置 relay MCP server' : `MCP 配置失败: ${mcpAdded.out.slice(0, 200)}`)
  } else {
    messages.push('relay MCP server 已是当前配置')
  }

  const legacy = join(codexHome(), 'skills', PLUGIN_NAME)
  if (existsSync(legacy)) {
    rmSync(legacy, { recursive: true, force: true })
    messages.push('已清理旧版手工复制的 skill')
  }

  return { status: await codexStatus(), messages }
}

/** Undoes everything Relay installed into Codex. */
export async function removeCodex(): Promise<InstallResult> {
  const messages: string[] = []
  const executable = await findCodexCli()
  if (executable) {
    const plugin = await codex(executable, ['plugin', 'remove', `${PLUGIN_NAME}@${MARKETPLACE_NAME}`])
    messages.push(plugin.ok ? '已卸载 relay 插件' : 'relay 插件未安装或已卸载')
    const marketplace = await codex(executable, ['plugin', 'marketplace', 'remove', MARKETPLACE_NAME])
    messages.push(marketplace.ok ? '已移除 relay marketplace' : 'relay marketplace 未注册或已移除')
    const mcp = await codex(executable, ['mcp', 'remove', PLUGIN_NAME])
    messages.push(mcp.ok ? '已移除 relay MCP server' : 'relay MCP server 未配置或已移除')
  } else {
    messages.push('未找到 codex CLI；只能清理本地文件')
  }
  rmSync(marketplaceRoot(), { recursive: true, force: true })
  const legacy = join(codexHome(), 'skills', PLUGIN_NAME)
  if (existsSync(legacy)) rmSync(legacy, { recursive: true, force: true })
  messages.push('已删除本地插件与旧 skill 副本')
  return { status: await codexStatus(), messages }
}

export async function runCodexAction(action: CodexAction): Promise<InstallResult> {
  return action === 'remove' ? removeCodex() : installCodex()
}
