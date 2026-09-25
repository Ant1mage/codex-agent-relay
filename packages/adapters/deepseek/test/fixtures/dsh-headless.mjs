import process from 'node:process'

if (process.argv.includes('--version')) {
  process.stdout.write('dsh 0.0.0-fixture\n')
  process.exit(0)
}

let task = ''
process.stdin.setEncoding('utf8')
for await (const chunk of process.stdin) task += chunk

const write = (event) => process.stdout.write(`${JSON.stringify(event)}\n`)
write({ type: 'session', sessionId: 'session-fixture', cwd: process.cwd() })

if (task.includes('HANG')) {
  process.on('SIGTERM', () => process.exit(143))
  setInterval(() => {}, 1_000)
} else if (task.includes('FAIL')) {
  write({ type: 'error', code: 'fixture', message: 'fixture failed' })
  process.exit(1)
} else {
  write({ type: 'status', phase: 'turn_start', turn: 1 })
  write({ type: 'thinking', text: 'inspect the workspace' })
  write({ type: 'tool_call', callId: 'call-1', tool: 'read_file', input: { path: 'README.md' } })
  write({ type: 'tool_result', callId: 'call-1', status: 'completed', result: 'contents' })
  write({ type: 'text', text: 'work complete' })
  write({ type: 'status', phase: 'turn_end', turn: 1, reason: { kind: 'completed' } })
  write({ type: 'final', text: 'work complete' })
}

