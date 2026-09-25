import process from 'node:process'
import readline from 'node:readline'

const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })
for await (const line of lines) {
  const request = JSON.parse(line)
  if (request.method === 'initialize') {
    process.stdout.write(`${JSON.stringify({ id: request.id, result: { userAgent: 'fake' } })}\n`)
  } else if (request.method === 'thread/read') {
    process.stdout.write(`${JSON.stringify({
      id: request.id,
      result: {
        thread: {
          id: request.params.threadId,
          name: 'Codex exact title',
          preview: 'First prompt fallback',
          cwd: '/workspace',
          model: 'gpt-test',
        },
      },
    })}\n`)
  }
}

