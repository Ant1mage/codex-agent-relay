import { z } from 'zod'

export const isoTimestampSchema = z.iso.datetime({ offset: true })
export const identifierSchema = z.string().trim().min(1).max(256)
export const localeSchema = z.enum(['en', 'zh-CN'])
export type Locale = z.infer<typeof localeSchema>

export const capabilitySetSchema = z.object({
  readWorkspace: z.boolean(),
  writeWorkspace: z.boolean(),
  executeCommands: z.boolean(),
  networkAccess: z.boolean(),
})
export type CapabilitySet = z.infer<typeof capabilitySetSchema>

export const adapterCapabilitiesSchema = z.object({
  nonInteractive: z.boolean(),
  structuredEvents: z.boolean(),
  cwd: z.boolean(),
  resume: z.boolean(),
  send: z.boolean(),
  cancel: z.boolean(),
  childSessions: z.boolean(),
})
export type AdapterCapabilities = z.infer<typeof adapterCapabilitiesSchema>

export const runtimeSchema = z.object({
  id: identifierSchema,
  adapterId: identifierSchema,
  executablePath: z.string().min(1),
  version: z.string().min(1).optional(),
  health: z.enum(['available', 'authentication_required', 'unavailable']),
  capabilities: adapterCapabilitiesSchema,
})
export type Runtime = z.infer<typeof runtimeSchema>

export const agentProfileSchema = z.object({
  id: identifierSchema,
  name: z.string().trim().min(1).max(128),
  runtimeId: identifierSchema,
  description: z.string().trim().min(1).max(2_000),
  instructions: z.string().trim().min(1).max(20_000).optional(),
  capabilities: capabilitySetSchema,
  enabled: z.boolean(),
})
export type AgentProfile = z.infer<typeof agentProfileSchema>

export const hostSessionStatusSchema = z.enum(['active', 'offline', 'ended'])
export const hostSessionSchema = z.object({
  id: identifierSchema,
  host: z.literal('codex'),
  nativeSessionId: identifierSchema,
  cwd: z.string().min(1),
  model: z.string().min(1).optional(),
  status: hostSessionStatusSchema,
  startedAt: isoTimestampSchema,
  endedAt: isoTimestampSchema.optional(),
})
export type HostSession = z.infer<typeof hostSessionSchema>

export const accessModeSchema = z.enum(['read_only', 'propose', 'write'])
export const isolationSchema = z.enum(['shared', 'worktree'])
export const runStatusSchema = z.enum([
  'queued',
  'starting',
  'running',
  'completed',
  'failed',
  'cancelled',
  'handed_off',
])
export const runSchema = z.object({
  id: identifierSchema,
  hostSessionId: identifierSchema,
  profileId: identifierSchema,
  task: z.string().trim().min(1).max(100_000),
  cwd: z.string().min(1),
  accessMode: accessModeSchema,
  isolation: isolationSchema,
  status: runStatusSchema,
  createdAt: isoTimestampSchema,
})
export type Run = z.infer<typeof runSchema>
export type RunStatus = z.infer<typeof runStatusSchema>

export const workerStatusSchema = z.enum([
  'starting',
  'running',
  'completed',
  'failed',
  'cancelled',
])
export const workerSessionSchema = z.object({
  id: identifierSchema,
  runId: identifierSchema,
  runtimeId: identifierSchema,
  nativeSessionId: identifierSchema.optional(),
  parentWorkerSessionId: identifierSchema.optional(),
  processId: z.number().int().positive().optional(),
  status: workerStatusSchema,
  startedAt: isoTimestampSchema,
  endedAt: isoTimestampSchema.optional(),
})
export type WorkerSession = z.infer<typeof workerSessionSchema>
export type WorkerStatus = z.infer<typeof workerStatusSchema>

export const relayEventTypeSchema = z.enum([
  'run/created',
  'worker/started',
  'worker/message',
  'worker/reasoning',
  'tool/read',
  'tool/search',
  'tool/edit',
  'tool/command',
  'tool/result',
  'test/result',
  'child/started',
  'child/completed',
  'worker/completed',
  'worker/failed',
  'worker/cancelled',
])
export type RelayEventType = z.infer<typeof relayEventTypeSchema>

export const relayEventSchema = z.object({
  id: identifierSchema,
  runId: identifierSchema,
  workerSessionId: identifierSchema.optional(),
  seq: z.number().int().positive(),
  timestamp: isoTimestampSchema,
  type: relayEventTypeSchema,
  data: z.unknown(),
  nativeEvent: z.unknown().optional(),
})
export type RelayEvent = z.infer<typeof relayEventSchema>

export const runRequestSchema = z.object({
  hostSessionId: identifierSchema,
  profileId: identifierSchema,
  task: z.string().trim().min(1).max(100_000),
  cwd: z.string().min(1),
  accessMode: accessModeSchema,
  isolation: isolationSchema.default('shared'),
})
export type RunRequest = z.infer<typeof runRequestSchema>

export const relayPolicySchema = z.object({
  maxConcurrentRuns: z.number().int().positive(),
  maxConcurrentWriters: z.number().int().positive(),
  requireWorktreeForParallelWriters: z.boolean(),
  allowWrite: z.boolean(),
  allowCommands: z.boolean(),
  allowNetwork: z.boolean(),
})
export type RelayPolicy = z.infer<typeof relayPolicySchema>

export const relayPolicyOverrideSchema = relayPolicySchema.partial()
export type RelayPolicyOverride = z.infer<typeof relayPolicyOverrideSchema>

export const startInputSchema = z.object({
  runId: identifierSchema,
  workerSessionId: identifierSchema,
  task: z.string().trim().min(1).max(100_000),
  cwd: z.string().min(1),
  accessMode: accessModeSchema,
  instructions: z.string().optional(),
})
export type StartInput = z.infer<typeof startInputSchema>
