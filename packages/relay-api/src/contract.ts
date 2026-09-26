import { z } from 'zod'
import {
  agentProfileSchema,
  hostSessionSchema,
  relayEventSchema,
  relayPolicyOverrideSchema,
  relayPolicySchema,
  runSchema,
  runtimeOptionsSchema,
  runtimeSchema,
  stepSchema,
  workerSessionSchema,
} from '@relay/protocol'

/**
 * The wire contract between the Relay daemon (apps/relayd), the tray
 * (apps/menu-bar) and the web inspector (apps/web). One schema per payload, so
 * both sides validate the same document instead of each keeping its own copy of
 * the shape (packages/protocol stays the domain schema; this is the transport).
 */

export const runViewSchema = z.object({
  run: runSchema,
  steps: stepSchema.array(),
  workers: workerSessionSchema.array(),
})
export type RunView = z.infer<typeof runViewSchema>

export const sessionViewSchema = z.object({
  session: hostSessionSchema,
  runs: runViewSchema.array(),
  /** Workers in starting/running state, so a list can show load at a glance. */
  activeWorkers: z.number().int().nonnegative(),
  awaitingHost: z.number().int().nonnegative(),
})
export type SessionView = z.infer<typeof sessionViewSchema>

/**
 * One thing Codex needs before Relay can be delegated to. `status` separates the
 * shades of "not ok" (never installed, pointing at a stale checkout, older than
 * the shipped copy, left over from a manual install) so repair can be specific
 * instead of a blind reinstall.
 */
export const codexCheckIdSchema = z.enum([
  'codex-cli',
  'relay-mcp',
  'relay-skill',
  'relay-plugin',
  'relay-hooks',
])
export const codexCheckSchema = z.object({
  id: codexCheckIdSchema,
  ok: z.boolean(),
  status: z.enum(['ok', 'missing', 'stale', 'outdated', 'legacy']),
  detail: z.string(),
  /** Suggested next step, shown verbatim in the control panel. */
  hint: z.string().optional(),
})
export type CodexCheck = z.infer<typeof codexCheckSchema>

export const codexStatusSchema = z.object({
  checks: codexCheckSchema.array(),
  configured: z.boolean(),
})
export type CodexStatus = z.infer<typeof codexStatusSchema>

export const snapshotSchema = z.object({
  sessions: sessionViewSchema.array(),
  runtimes: runtimeSchema.array(),
  profiles: agentProfileSchema.array(),
  diagnostics: z.string().array(),
  codex: codexStatusSchema,
  generatedAt: z.string(),
})
export type InspectorSnapshot = z.infer<typeof snapshotSchema>

export const menuWorkerSchema = z.object({
  workerSessionId: z.string().min(1),
  runId: z.string().min(1),
  label: z.string(),
})
export const menuSessionSchema = z.object({
  id: z.string().min(1),
  displayName: z.string(),
  cwd: z.string(),
  activeWorkers: menuWorkerSchema.array(),
})
export const menuAgentSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  /** Absent when the agent can run; otherwise why it cannot. */
  blocked: z.enum(['auth', 'missing', 'disabled']).optional(),
})
export const menuViewSchema = z.object({
  status: z.enum(['ready', 'needsSetup', 'noRuntime']),
  runningWorkers: z.number().int().nonnegative(),
  awaitingHost: z.number().int().nonnegative(),
  sessions: menuSessionSchema.array(),
  agents: menuAgentSchema.array(),
  /** Runtime scan results, so the menu can show the environment itself. */
  runtimes: runtimeSchema.array(),
  codex: codexStatusSchema,
})
export type MenuView = z.infer<typeof menuViewSchema>

/** A runtimes.json entry: a CLI the user registered by hand. */
export const manualRuntimeSchema = z.object({
  id: z.string().min(1),
  adapterId: z.string().min(1),
  executablePath: z.string().min(1),
  label: z.string().min(1).optional(),
})
export type ManualRuntimeView = z.infer<typeof manualRuntimeSchema>

/** Relay's own configuration as it exists on disk right now. */
export const relayConfigSchema = z.object({
  profiles: agentProfileSchema.array(),
  policy: relayPolicySchema,
  workspaceOverrides: z.record(z.string(), relayPolicyOverrideSchema),
  /** Hand-registered runtimes, merged with what the scanner finds. */
  manualRuntimes: manualRuntimeSchema.array(),
  /**
   * Unparseable or invalid configuration files. The daemon keeps running on
   * defaults; the panel shows this instead of pretending nothing happened.
   */
  warnings: z.string().array(),
  /** Changes whenever any configuration file changes. */
  revision: z.string(),
})
export type RelayConfigView = z.infer<typeof relayConfigSchema>

export const adapterCatalogSchema = z.object({ adapters: z.string().array() })
export type AdapterCatalog = z.infer<typeof adapterCatalogSchema>

export const runtimeProbeSchema = z.object({
  ok: z.boolean(),
  version: z.string().optional(),
  error: z.string().optional(),
})
export type RuntimeProbe = z.infer<typeof runtimeProbeSchema>

/** A runtime write answers with the new configuration and what was verified. */
export const runtimeMutationSchema = z.object({
  config: relayConfigSchema,
  probe: runtimeProbeSchema,
})
export type RuntimeMutation = z.infer<typeof runtimeMutationSchema>

export const runtimeOptionsViewSchema = runtimeOptionsSchema
export type RuntimeOptionsView = z.infer<typeof runtimeOptionsViewSchema>

export const codexActionSchema = z.enum(['install', 'repair', 'update', 'remove'])
export type CodexAction = z.infer<typeof codexActionSchema>

export const refreshResultSchema = z.object({
  runtimes: z.number().int().nonnegative(),
  profiles: z.number().int().nonnegative(),
  detectedAt: z.string(),
})
export type RefreshResult = z.infer<typeof refreshResultSchema>

export const healthSchema = z.object({
  ok: z.literal(true),
  pid: z.number().int().positive(),
  /** Identity proof: a reused PID cannot fake this. */
  nonce: z.string(),
  port: z.number().int().positive(),
  startedAt: z.string(),
  version: z.string(),
  database: z.string(),
  sessions: z.number().int().nonnegative(),
  runs: z.number().int().nonnegative(),
})
export type Health = z.infer<typeof healthSchema>

export const eventBatchSchema = z.object({
  runId: z.string().min(1),
  events: relayEventSchema.array(),
})
export type EventBatch = z.infer<typeof eventBatchSchema>

export const streamMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('hello'), port: z.number(), startedAt: z.string() }),
  z.object({ type: z.literal('snapshot'), snapshot: snapshotSchema }),
  z.object({ type: z.literal('events'), batch: eventBatchSchema }),
])
export type StreamMessage = z.infer<typeof streamMessageSchema>

export const cancelResultSchema = z.object({
  accepted: z.boolean(),
  count: z.number().int().nonnegative().optional(),
  message: z.string().optional(),
})
export type CancelResult = z.infer<typeof cancelResultSchema>

export const installResultSchema = z.object({
  status: codexStatusSchema,
  messages: z.string().array(),
})
export type InstallResult = z.infer<typeof installResultSchema>
