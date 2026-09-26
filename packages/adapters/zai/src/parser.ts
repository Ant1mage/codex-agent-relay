import type { AdapterEvent } from '@relay/adapter-sdk'

export interface ParsedZaiOutput {
  events: AdapterEvent[]
  finalText?: string
  errorMessage?: string
}

function findText(value: unknown): string | undefined {
  if (typeof value === 'string') return value
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  for (const key of ['response', 'content', 'message', 'text', 'output']) {
    if (typeof record[key] === 'string') return record[key]
  }
  if (record.data) return findText(record.data)
  return undefined
}

export function parseZaiOutput(output: string): ParsedZaiOutput {
  let raw: unknown
  try {
    raw = JSON.parse(output)
  } catch {
    return {
      events: [{
        type: 'worker/status',
        data: { message: 'GLM / Z.ai CLI returned non-JSON output' },
        nativeEvent: output.slice(0, 32_768),
      }],
      finalText: output.trim(),
    }
  }
  const record = raw && typeof raw === 'object' ? raw as Record<string, unknown> : undefined
  const error = record?.error
  const errorMessage = typeof error === 'string'
    ? error
    : error && typeof error === 'object' && typeof (error as Record<string, unknown>).message === 'string'
      ? String((error as Record<string, unknown>).message)
      : undefined
  const finalText = findText(raw)
  return {
    events: finalText ? [{ type: 'worker/message', data: { kind: 'final', text: finalText }, nativeEvent: raw }] : [],
    ...(finalText ? { finalText } : {}),
    ...(errorMessage ? { errorMessage } : {}),
  }
}
