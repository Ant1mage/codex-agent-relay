import { z } from 'zod'
import {
  agentProfileSchema,
  hostSessionSchema,
  relayEventSchema,
  runSchema,
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

export const codexCheckSchema = z.object({
  id: z.enum(['codex-cli', 'relay-mcp', 'relay-skill']),
  ok: z.boolean(),
  detail: z.string(),
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
  codex: codexStatusSchema,
})
export type MenuView = z.infer<typeof menuViewSchema>

export const healthSchema = z.object({
  ok: z.literal(true),
  pid: z.number().int().positive(),
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
