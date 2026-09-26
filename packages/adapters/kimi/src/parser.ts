import type { AdapterEvent } from '@relay/adapter-sdk'

export interface ParsedKimiLine {
  events: AdapterEvent[]
  sessionId?: string
  finalText?: string
}

function textContent(content: unknown): string {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content
    .map((part) => {
      if (!part || typeof part !== 'object') return ''
      const value = part as Record<string, unknown>
      return typeof value.text === 'string' ? value.text : ''
    })
    .join('')
}

function toolEventType(tool: string): AdapterEvent['type'] {
  const name = tool.toLowerCase()
  if (/search|grep|glob|find|fetch/.test(name)) return 'tool/search'
  if (/edit|write|patch|replace|delete|move/.test(name)) return 'tool/edit'
  if (/read|view|open|list/.test(name)) return 'tool/read'
  if (/test/.test(name)) return 'test/result'
  return 'tool/command'
}

export function parseKimiLine(line: string): ParsedKimiLine {
  let raw: unknown
  try {
    raw = JSON.parse(line)
  } catch {
    return {
      events: [{
        type: 'worker/message',
        data: { kind: 'diagnostic', message: 'Kimi Code emitted malformed JSON' },
        nativeEvent: line.slice(0, 8_192),
      }],
    }
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return { events: [] }
  const message = raw as Record<string, unknown>
  if (message.role === 'meta') {
    const sessionId = typeof message.session_id === 'string'
      ? message.session_id
      : typeof message.sessionId === 'string'
        ? message.sessionId
        : undefined
    return {
      events: [],
      ...(sessionId ? { sessionId } : {}),
    }
  }
  if (message.role === 'assistant') {
    const text = textContent(message.content)
    const events: AdapterEvent[] = []
    if (text) events.push({ type: 'worker/message', data: { text }, nativeEvent: raw })
    if (Array.isArray(message.tool_calls)) {
      for (const entry of message.tool_calls) {
        if (!entry || typeof entry !== 'object') continue
        const call = entry as Record<string, unknown>
        const fn = call.function && typeof call.function === 'object'
          ? call.function as Record<string, unknown>
          : {}
        const tool = typeof fn.name === 'string' ? fn.name : 'unknown'
        events.push({
          type: toolEventType(tool),
          data: { callId: call.id, tool, arguments: fn.arguments },
          nativeEvent: raw,
        })
      }
    }
    return { events, ...(text ? { finalText: text } : {}) }
  }
  if (message.role === 'tool') {
    return {
      events: [{
        type: 'tool/result',
        data: { callId: message.tool_call_id, output: textContent(message.content) },
        nativeEvent: raw,
      }],
    }
  }
  return { events: [] }
}
