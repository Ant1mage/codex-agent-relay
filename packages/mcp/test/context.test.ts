import { describe, expect, it } from 'vitest'
import { codexInvocationContext } from '../src/index.js'

describe('Codex invocation identity', () => {
  it('prefers host-provided environment identity over request-shaped data', () => {
    expect(
      codexInvocationContext(
        { _meta: { thread_id: 'untrusted-request-thread', turn_id: 'turn-1' } },
        { CODEX_THREAD_ID: 'host-thread' },
      ),
    ).toEqual({ threadId: 'host-thread', turnId: 'turn-1' })
  })

  it('reads encoded Codex request metadata when environment identity is absent', () => {
    expect(
      codexInvocationContext(
        { requestInfo: { meta: { 'x-codex-turn-metadata': '{"session_id":"thread-2"}' } } },
        {},
      ).threadId,
    ).toBe('thread-2')
  })
})

