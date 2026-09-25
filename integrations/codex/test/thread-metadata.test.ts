import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { CodexAppServerThreadResolver } from '../src/index.js'

const fixture = fileURLToPath(new URL('./fixtures/fake-codex-app-server.mjs', import.meta.url))

describe('CodexAppServerThreadResolver', () => {
  it('uses the exact Codex thread name as the Relay display name', async () => {
    const resolver = new CodexAppServerThreadResolver({
      executablePath: process.execPath,
      prefixArgs: [fixture],
    })
    await expect(resolver.resolve('thread-1')).resolves.toEqual({
      id: 'thread-1',
      displayName: 'Codex exact title',
      cwd: '/workspace',
      model: 'gpt-test',
    })
  })
})

