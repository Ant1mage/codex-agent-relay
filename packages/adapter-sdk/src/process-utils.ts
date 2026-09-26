import { accessSync, constants } from 'node:fs'
import { delimiter, join } from 'node:path'

export class AsyncEventQueue<T> implements AsyncIterable<T> {
  readonly #items: T[] = []
  readonly #waiters: Array<(result: IteratorResult<T>) => void> = []
  #closed = false

  push(item: T): void {
    if (this.#closed) return
    const waiter = this.#waiters.shift()
    if (waiter) waiter({ value: item, done: false })
    else this.#items.push(item)
  }

  close(): void {
    if (this.#closed) return
    this.#closed = true
    for (const waiter of this.#waiters.splice(0)) waiter({ value: undefined, done: true })
  }

  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: async () => {
        const item = this.#items.shift()
        if (item !== undefined) return { value: item, done: false }
        if (this.#closed) return { value: undefined, done: true }
        return new Promise<IteratorResult<T>>((resolve) => this.#waiters.push(resolve))
      },
    }
  }
}

export function canExecute(candidate: string): boolean {
  try {
    accessSync(candidate, constants.X_OK)
    return true
  } catch {
    return false
  }
}

export function discoverExecutable(name: string, additionalCandidates: string[] = []): string | undefined {
  const directories = (process.env.PATH ?? '').split(delimiter).filter(Boolean)
  const extensions = process.platform === 'win32'
    ? (process.env.PATHEXT ?? '.EXE;.CMD;.BAT').split(';')
    : ['']
  for (const directory of directories) {
    for (const extension of extensions) {
      const candidate = join(directory, `${name}${extension}`)
      if (canExecute(candidate)) return candidate
    }
  }
  return additionalCandidates.find(canExecute)
}
