import type { AdapterEvent } from '@relay/adapter-sdk'

export interface ParsedGeminiLine {
  events: AdapterEvent[]
  sessionId?: string
  finalText?: string
  errorMessage?: string
}

function textValue(value: unknown): string {
  if (typeof value === 'string') return value
  if (!Array.isArray(value)) return ''
  return value.map((part) => {
    if (!part || typeof part !== 'object') return ''
    const record = part as Record<string, unknown>
    return typeof record.text === 'string' ? record.text : ''
  }).join('')
}

function toolEventType(name: string): AdapterEvent['type'] {
  const lower = name.toLowerCase()
  if (/search|grep|glob|find|fetch/.test(lower)) return 'tool/search'
  if (/edit|write|patch|replace|delete|move/.test(lower)) return 'tool/edit'
  if (/read|view|open|list/.test(lower)) return 'tool/read'
  if (/test/.test(lower)) return 'test/result'
  return 'tool/command'
}

export function parseGeminiLine(line: string): ParsedGeminiLine {
  let raw: unknown
  try {
    raw = JSON.parse(line)
  } catch {
    return {
      events: [{
        type: 'worker/status',
        data: { message: 'Gemini CLI emitted malformed JSON' },
        nativeEvent: line.slice(0, 8_192),
      }],
    }
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return { events: [] }
  const event = raw as Record<string, unknown>
  const sessionId = typeof event.session_id === 'string'
    ? event.session_id
    : typeof event.sessionId === 'string'
      ? event.sessionId
      : undefined
  const type = typeof event.type === 'string' ? event.type : ''
  if (type === 'init') {
    return {
      events: [{ type: 'worker/status', data: { phase: 'initialized' }, nativeEvent: raw }],
      ...(sessionId ? { sessionId } : {}),
    }
  }
  if (type === 'message') {
    const text = textValue(event.content ?? event.text ?? event.delta)
    return {
      events: text ? [{ type: 'worker/message', data: { text }, nativeEvent: raw }] : [],
      ...(sessionId ? { sessionId } : {}),
      ...(text ? { finalText: text } : {}),
    }
  }
  if (type === 'tool_use') {
    const tool = typeof event.tool_name === 'string'
      ? event.tool_name
      : typeof event.name === 'string'
        ? event.name
        : 'unknown'
    return {
      events: [{
        type: toolEventType(tool),
        data: { tool, parameters: event.parameters ?? event.arguments },
        nativeEvent: raw,
      }],
      ...(sessionId ? { sessionId } : {}),
    }
  }
  if (type === 'tool_result') {
    return {
      events: [{
        type: 'tool/result',
        data: { tool: event.tool_name ?? event.name, output: event.output ?? event.result },
        nativeEvent: raw,
      }],
      ...(sessionId ? { sessionId } : {}),
    }
  }
  if (type === 'error') {
    const message = typeof event.message === 'string' ? event.message : 'Gemini CLI reported an error'
    return {
      events: [{ type: 'worker/status', data: { message }, nativeEvent: raw }],
      ...(sessionId ? { sessionId } : {}),
      errorMessage: message,
    }
  }
  if (type === 'result') {
    const text = textValue(event.response ?? event.text)
    const status = typeof event.status === 'string' ? event.status.toLowerCase() : 'success'
    return {
      events: text ? [{ type: 'worker/message', data: { kind: 'final', text }, nativeEvent: raw }] : [],
      ...(sessionId ? { sessionId } : {}),
      ...(text ? { finalText: text } : {}),
      ...(status === 'success' ? {} : { errorMessage: String(event.error ?? status) }),
    }
  }
  return { events: [], ...(sessionId ? { sessionId } : {}) }
}
