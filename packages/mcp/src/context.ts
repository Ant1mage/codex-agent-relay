export interface CodexInvocationContext {
  threadId: string
  turnId?: string
}

function findString(value: unknown, keys: Set<string>, depth = 0): string | undefined {
  if (depth > 5 || value === null || typeof value !== 'object') return undefined
  for (const [key, item] of Object.entries(value)) {
    if (keys.has(key) && typeof item === 'string' && item.trim()) return item
    if (typeof item === 'string' && item.startsWith('{')) {
      try {
        const nested = findString(JSON.parse(item), keys, depth + 1)
        if (nested) return nested
      } catch {
        // It is an ordinary string rather than encoded metadata.
      }
    }
    const nested = findString(item, keys, depth + 1)
    if (nested) return nested
  }
  return undefined
}

export function codexInvocationContext(
  requestContext: unknown,
  environment: NodeJS.ProcessEnv = process.env,
): CodexInvocationContext {
  const threadId =
    environment.CODEX_THREAD_ID ??
    environment.CODEX_SESSION_ID ??
    findString(requestContext, new Set(['thread_id', 'threadId', 'session_id', 'sessionId']))
  if (!threadId) throw new Error('Codex thread identity is unavailable')
  const turnId = findString(requestContext, new Set(['turn_id', 'turnId']))
  return { threadId, ...(turnId ? { turnId } : {}) }
}

