import { execFile } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { promisify } from 'node:util'
import { AntigravityAdapter } from '@relay/adapter-antigravity'
import { DeepSeekAdapter } from '@relay/adapter-deepseek'
import { GeminiAdapter } from '@relay/adapter-gemini'
import { KimiAdapter } from '@relay/adapter-kimi'
import { ZaiAdapter } from '@relay/adapter-zai'
import type { AgentAdapter } from '@relay/adapter-sdk'
import { defaultProfiles } from '@relay/core/default-profiles'
import { agentProfileSchema, type AgentProfile, type Runtime } from '@relay/protocol'
import type { CodexCheck, CodexStatus, InstallResult } from '@relay/relay-api'
import { profilesPath } from '@relay/relay-api/server-info'

/**
 * Everything the daemon knows about this machine that is not in the event log:
 * which runtimes exist, which profiles Codex sees, and whether Codex is wired up
 * at all. Detection spawns CLIs, so it runs once when the daemon starts.
 */

const execFileAsync = promisify(execFile)
const PROBE_TIMEOUT_MS = 4_000

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

export async function detectEnvironment(): Promise<Environment> {
  const detected = await Promise.all(adapters().map((adapter) => adapter.detect()))
  const runtimes = detected.flatMap((result) => result.runtimes)
  const diagnostics = detected.flatMap((result) => result.diagnostics)
  const defaults = defaultProfiles(runtimes)

  const path = profilesPath()
  if (!existsSync(path)) return { runtimes, profiles: defaults, diagnostics }
  try {
    const stored = agentProfileSchema.array().parse(JSON.parse(readFileSync(path, 'utf8')))
    const available = new Set(runtimes.map((runtime) => runtime.id))
    const storedById = new Map(
      stored.filter((profile) => available.has(profile.runtimeId)).map((profile) => [profile.id, profile]),
    )
    const profiles = defaults.map((profile) => storedById.get(profile.id) ?? profile)
    for (const profile of storedById.values()) {
      if (!profiles.some((candidate) => candidate.id === profile.id)) profiles.push(profile)
    }
    return { runtimes, profiles, diagnostics }
  } catch {
    diagnostics.push('Invalid profiles.json; using built-in profiles.')
    return { runtimes, profiles: defaults, diagnostics }
  }
}

function codexHome(): string {
  const configured = process.env.CODEX_HOME
  return configured && configured.length > 0 ? configured : join(homedir(), '.codex')
}

/**
 * Candidate codex executables in the order Codex itself installs them. The
 * CLI is not always on PATH: the VS Code extension and the desktop app bundle
 * their own copy, which is the normal install for most users.
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

export interface CodexExecutable {
  path: string
  version: string
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

async function probeCodexCli(): Promise<CodexCheck> {
  const executable = await findCodexCli()
  if (executable) {
    return {
      id: 'codex-cli',
      ok: true,
      detail:
        executable.path === 'codex'
          ? `codex · ${executable.version}`
          : `${executable.path} · ${executable.version}`,
    }
  }
  return { id: 'codex-cli', ok: false, detail: 'codex not found on PATH or in a known Codex location' }
}

function probeRelayMcp(): CodexCheck {
  const candidates = [join(codexHome(), 'config.toml'), join(codexHome(), 'mcp.json')]
  for (const candidate of candidates) {
    if (!existsSync(candidate)) continue
    try {
      if (/mcp_servers\.relay|"relay"\s*:/i.test(readFileSync(candidate, 'utf8'))) {
        return { id: 'relay-mcp', ok: true, detail: candidate }
      }
    } catch {
      // Unreadable config is reported as not configured below.
    }
  }
  return { id: 'relay-mcp', ok: false, detail: join(codexHome(), 'config.toml') }
}

function probeRelaySkill(): CodexCheck {
  const primary = join(codexHome(), 'skills', 'relay', 'SKILL.md')
  const candidates = [primary, join(homedir(), '.config', 'codex', 'skills', 'relay', 'SKILL.md')]
  for (const candidate of candidates) {
    if (existsSync(candidate)) return { id: 'relay-skill', ok: true, detail: candidate }
  }
  return { id: 'relay-skill', ok: false, detail: primary }
}

export async function codexStatus(): Promise<CodexStatus> {
  const checks = [await probeCodexCli(), probeRelayMcp(), probeRelaySkill()]
  return { checks, configured: checks.every((check) => check.ok) }
}

/**
 * Finds the source skill and workspace command. Walks up from this file so the
 * daemon can be started from any working directory.
 */
export function relayInstallation(): { workspaceRoot: string; skillSource: string } | undefined {
  let directory = import.meta.dirname
  for (let depth = 0; depth < 5; depth += 1) {
    const skillSource = join(directory, 'integrations', 'codex', 'skills', 'relay', 'SKILL.md')
    if (existsSync(skillSource) && existsSync(join(directory, 'package.json'))) {
      return { workspaceRoot: directory, skillSource }
    }
    const parent = dirname(directory)
    if (parent === directory) break
    directory = parent
  }
  return undefined
}

/**
 * Installs the two parts Codex needs to discover Relay: the stdio MCP server
 * through Codex's own CLI, and the shipped skill in CODEX_HOME. Both steps are
 * idempotent, so running it twice never touches a working configuration.
 */
export async function installCodexIntegration(): Promise<InstallResult> {
  const messages: string[] = []
  const installation = relayInstallation()
  const executable = await findCodexCli()

  if (!installation) {
    return { status: await codexStatus(), messages: ['Relay installation files are not available in this build'] }
  }
  if (!executable) {
    return {
      status: await codexStatus(),
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
      await execFileAsync(
        executable.path,
        ['mcp', 'add', 'relay', '--', 'corepack', 'pnpm', '--dir', installation.workspaceRoot, 'mcp:dev'],
        { timeout: PROBE_TIMEOUT_MS },
      )
      messages.push('Configured the Relay MCP server for Codex')
    } catch (error) {
      messages.push(`Could not configure Relay MCP: ${error instanceof Error ? error.message : String(error)}`)
    }
  } else {
    messages.push('Relay MCP is already configured')
  }

  return { status: await codexStatus(), messages }
}
