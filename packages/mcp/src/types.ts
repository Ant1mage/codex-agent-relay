import type { AgentProfile as ProtocolAgentProfile } from '@relay/protocol'

export type AccessMode = 'read_only' | 'propose' | 'write'
export type Isolation = 'shared' | 'worktree'
export type AgentProfile = ProtocolAgentProfile

