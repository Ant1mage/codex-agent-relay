import { z } from 'zod'

export const relayErrorCodeSchema = z.enum([
  'INVALID_REQUEST',
  'HOST_SESSION_NOT_FOUND',
  'SESSION_NAME_UNAVAILABLE',
  'PROFILE_NOT_FOUND',
  'PROFILE_DISABLED',
  'RUNTIME_NOT_FOUND',
  'RUNTIME_UNAVAILABLE',
  'CAPABILITY_DENIED',
  'CONCURRENCY_LIMIT',
  'WORKSPACE_CONFLICT',
  'RUN_NOT_FOUND',
  'WORKER_NOT_FOUND',
  'OPERATION_UNSUPPORTED',
  'ADAPTER_FAILURE',
  'EVENT_SEQUENCE_CONFLICT',
])
export type RelayErrorCode = z.infer<typeof relayErrorCodeSchema>

export class RelayError extends Error {
  readonly code: RelayErrorCode
  readonly details?: unknown

  constructor(code: RelayErrorCode, message: string, details?: unknown) {
    super(message)
    this.name = 'RelayError'
    this.code = code
    this.details = details
  }
}
