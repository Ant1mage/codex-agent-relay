import {
  RelayError,
  hostSessionSchema,
  hostSessionUpsertSchema,
  type HostSession,
  type HostSessionUpsert,
} from '@relay/protocol'

export class HostSessionRegistry {
  readonly #sessions = new Map<string, HostSession>()

  upsertCodex(input: HostSessionUpsert): HostSession {
    const update = hostSessionUpsertSchema.parse(input)
    const id = `codex:${update.nativeSessionId}`
    const existing = this.#sessions.get(id)
    const now = new Date().toISOString()
    const session = hostSessionSchema.parse({
      id,
      host: 'codex',
      nativeSessionId: update.nativeSessionId,
      displayName: update.displayName,
      nameSource: 'codex',
      cwd: update.cwd,
      ...(update.model ? { model: update.model } : existing?.model ? { model: existing.model } : {}),
      status: update.status,
      startedAt: existing?.startedAt ?? now,
      updatedAt: now,
      ...(update.status === 'ended' ? { endedAt: now } : {}),
    })
    this.#sessions.set(id, session)
    return session
  }

  renameFromCodex(nativeSessionId: string, displayName: string): HostSession {
    const id = `codex:${nativeSessionId}`
    const existing = this.#sessions.get(id)
    if (!existing) throw new RelayError('HOST_SESSION_NOT_FOUND', `Unknown host session ${id}`)
    return this.upsertCodex({
      nativeSessionId,
      displayName,
      cwd: existing.cwd,
      ...(existing.model ? { model: existing.model } : {}),
      status: existing.status,
    })
  }

  get(id: string): HostSession | undefined {
    const session = this.#sessions.get(id)
    return session ? structuredClone(session) : undefined
  }

  list(): HostSession[] {
    return [...this.#sessions.values()]
      .map((session) => structuredClone(session))
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))
  }
}

