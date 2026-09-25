import { resolve } from 'node:path'
import process from 'node:process'
import { DeepSeekAdapter } from './adapter.js'

const [task, cwdArgument] = process.argv.slice(2)
if (!task) {
  process.stderr.write('Usage: pnpm deepseek:smoke -- "task" [cwd]\n')
  process.exitCode = 2
} else {
  const adapter = new DeepSeekAdapter()
  const detection = await adapter.detect()
  const runtime = detection.runtimes[0]
  if (!runtime) {
    process.stderr.write(`${detection.diagnostics.join('\n')}\n`)
    process.exitCode = 1
  } else {
    process.stderr.write(`Using ${runtime.executablePath} ${runtime.version ?? ''}\n`)
    const handle = await adapter.start({
      runId: `smoke:${Date.now()}`,
      workerSessionId: `smoke-worker:${Date.now()}`,
      task,
      cwd: resolve(cwdArgument ?? process.cwd()),
      accessMode: 'write',
    })
    process.stderr.write(`Session ${handle.nativeSessionId ?? 'unknown'}\n`)
    for await (const event of handle.events) {
      process.stdout.write(`${JSON.stringify(event)}\n`)
      if (event.type === 'worker/failed') process.exitCode = 1
    }
    await adapter.dispose()
  }
}
