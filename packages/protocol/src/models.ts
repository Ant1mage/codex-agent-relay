import { z } from 'zod'

export const isoTimestampSchema = z.iso.datetime({ offset: true })
export const identifierSchema = z.string().trim().min(1).max(256)
export const localeSchema = z.enum(['en', 'zh-CN'])
export type Locale = z.infer<typeof localeSchema>
/**
 * Reasoning is a value the runtime's CLI defines and reports, not a fixed Relay
 * scale, so it is stored as an opaque token. Relay only ever
 * writes a value the CLI itself listed.
 */
export const reasoningEffortSchema = z.string().trim().min(1).max(64)
export type ReasoningEffort = z.infer<typeof reasoningEffortSchema>

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
  /**
   * True only when the CLI itself advertises a model flag, i.e. when Relay can
   * actually hand a model to it. Declared capabilities stay false until the
   * probe in @relay/adapter-sdk finds the flag, so the UI never offers a picker
   * that cannot affect a run.
   */
  modelSelection: z.boolean().optional(),
})
export type AdapterCapabilities = z.infer<typeof adapterCapabilitiesSchema>

/**
 * One selectable model, as reported by the CLI itself. Relay never invents
 * model names: `value` is what the CLI accepts on its model flag.
 */
export const modelOptionSchema = z.object({
  value: z.string().trim().min(1).max(128),
  label: z.string().trim().min(1).max(200).optional(),
})
export type ModelOption = z.infer<typeof modelOptionSchema>

/**
 * One selectable reasoning strength. `strength` gives the slider its order, so
 * a CLI reporting four levels yields four stops rather than three.
 */
export const reasoningLevelSchema = z.object({
  strength: z.number().int().min(1).max(5),
  label: z.string().trim().min(1).max(200),
  value: z.string().trim().min(1).max(128),
})
export type ReasoningLevel = z.infer<typeof reasoningLevelSchema>

/** What a runtime's CLI reports about its own model and reasoning choices. */
export const runtimeOptionsSchema = z.object({
  runtimeId: identifierSchema,
  adapterId: identifierSchema,
  models: modelOptionSchema.array(),
  levels: reasoningLevelSchema.array(),
  /** Flags Relay would use, empty when the CLI did not advertise them. */
  modelFlag: z.string().trim().min(1).optional(),
  reasoningFlag: z.string().trim().min(1).optional(),
  /** Where the reported choices came from, so the UI can say so. */
  source: z.enum(['cli', 'api', 'default']),
  diagnostics: z.string().array(),
})
export type RuntimeOptions = z.infer<typeof runtimeOptionsSchema>

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
  model: z.string().trim().min(1).max(128).optional(),
  reasoning: reasoningEffortSchema.optional(),
  capabilities: capabilitySetSchema,
  enabled: z.boolean(),
})
export type AgentProfile = z.infer<typeof agentProfileSchema>

export const hostSessionStatusSchema = z.enum(['active', 'offline', 'ended'])
export const hostSessionSchema = z.object({
  id: identifierSchema,
  host: z.literal('codex'),
  nativeSessionId: identifierSchema,
  displayName: z.string().trim().min(1).max(500),
  nameSource: z.literal('codex'),
  cwd: z.string().min(1),
  model: z.string().min(1).optional(),
  status: hostSessionStatusSchema,
  startedAt: isoTimestampSchema,
  updatedAt: isoTimestampSchema,
  endedAt: isoTimestampSchema.optional(),
})
export type HostSession = z.infer<typeof hostSessionSchema>

export const hostSessionUpsertSchema = hostSessionSchema.pick({
  nativeSessionId: true,
  displayName: true,
  cwd: true,
  model: true,
}).extend({ status: hostSessionStatusSchema.default('active') })
export type HostSessionUpsert = z.infer<typeof hostSessionUpsertSchema>

export const accessModeSchema = z.enum(['read_only', 'propose', 'write'])
export const isolationSchema = z.enum(['shared', 'worktree'])
export const runStatusSchema = z.enum([
  'queued',
  'starting',
  'running',
  'awaiting_host',
  'completed',
  'failed',
  'cancelled',
  'interrupted',
  'orphaned',
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
  updatedAt: isoTimestampSchema,
})
export type Run = z.infer<typeof runSchema>
export type RunStatus = z.infer<typeof runStatusSchema>

export const stepStatusSchema = z.enum([
  'queued',
  'starting',
  'running',
  'awaiting_host',
  'completed',
  'failed',
  'cancelled',
  'interrupted',
  'orphaned',
])
export const stepSchema = z.object({
  id: identifierSchema,
  runId: identifierSchema,
  profileId: identifierSchema,
  task: z.string().trim().min(1).max(100_000),
  accessMode: accessModeSchema,
  isolation: isolationSchema,
  status: stepStatusSchema,
  iteration: z.number().int().positive(),
  createdAt: isoTimestampSchema,
  updatedAt: isoTimestampSchema,
})
export type Step = z.infer<typeof stepSchema>
export type StepStatus = z.infer<typeof stepStatusSchema>

export const workerStatusSchema = z.enum([
  'starting',
  'running',
  'completed',
  'failed',
  'cancelled',
  'interrupted',
  'orphaned',
])
export const workerSessionSchema = z.object({
  id: identifierSchema,
  runId: identifierSchema,
  stepId: identifierSchema,
  iteration: z.number().int().positive(),
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
  'run/awaiting_host',
  'run/accepted',
  'step/created',
  'step/iteration_started',
  'worker/started',
  'worker/message',
  'worker/status',
  // Kept for provider compatibility. The desktop console intentionally hides it.
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
  'worker/interrupted',
  'worker/orphaned',
])
export type RelayEventType = z.infer<typeof relayEventTypeSchema>

export const relayEventSchema = z.object({
  id: identifierSchema,
  runId: identifierSchema,
  stepId: identifierSchema.optional(),
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
  /** Exact executable selected by RuntimeRegistry, including manual entries. */
  executablePath: z.string().min(1).optional(),
  model: z.string().trim().min(1).max(128).optional(),
  reasoning: reasoningEffortSchema.optional(),
  instructions: z.string().optional(),
})
export type StartInput = z.infer<typeof startInputSchema>
