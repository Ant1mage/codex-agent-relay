import { describe, expect, it } from 'vitest'

describe('DeepSeek smoke command arguments', () => {
  it('documents the pnpm separator before task and cwd', () => {
    const commandArguments = ['--', 'inspect docs', '/workspace']
    if (commandArguments[0] === '--') commandArguments.shift()
    expect(commandArguments).toEqual(['inspect docs', '/workspace'])
  })
})
