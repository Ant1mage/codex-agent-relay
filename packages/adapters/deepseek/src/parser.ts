import type { AdapterEvent } from '@relay/adapter-sdk'

interface NativeObject {
  type: string
  [key: string]: unknown
}

export interface ParsedDeepSeekLine {
  events: AdapterEvent[]
  sessionId?: string
  finalText?: string
  errorMessage?: string
  turnEndKind?: string
}

function toolEventType(tool: string): AdapterEvent['type'] {
  const name = tool.toLowerCase()
  if (/search|grep|glob|find/.test(name)) return 'tool/search'
  if (/edit|write|patch|replace|delete|move/.test(name)) return 'tool/edit'
  if (/read|view|open|list/.test(name)) return 'tool/read'
  return 'tool/command'
}

function objectValue(value: unknown): NativeObject | undefined {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined
  const candidate = value as Record<string, unknown>
  return typeof candidate.type === 'string' ? (candidate as NativeObject) : undefined
}

export function parseDeepSeekLine(line: string): ParsedDeepSeekLine {
  let raw: unknown
  try {
    raw = JSON.parse(line)
  } catch {
    return {
      events: [
        {
          type: 'worker/message',
          data: { kind: 'diagnostic', message: 'DeepSeek Harness emitted malformed JSON' },
          nativeEvent: line.slice(0, 8_192),
        },
      ],
    }
  }

  const event = objectValue(raw)
  if (!event) {
    return {
      events: [
        {
          type: 'worker/message',
          data: { kind: 'diagnostic', message: 'DeepSeek Harness emitted an invalid event' },
          nativeEvent: raw,
        },
      ],
    }
  }

  switch (event.type) {
    case 'session':
      return {
        events: [],
        ...(typeof event.sessionId === 'string' ? { sessionId: event.sessionId } : {}),
      }
    case 'thinking':
      return {
        events: [
          { type: 'worker/reasoning', data: { text: event.text ?? '' }, nativeEvent: raw },
        ],
      }
    case 'text':
      return {
        events: [{ type: 'worker/message', data: { text: event.text ?? '' }, nativeEvent: raw }],
      }
    case 'status': {
      const reason =
        event.phase === 'turn_end' && event.reason && typeof event.reason === 'object'
          ? (event.reason as Record<string, unknown>).kind
          : undefined
      return {
        events: [
          {
            type: 'worker/message',
            data: {
              kind: 'status',
              phase: event.phase,
              turn: event.turn,
              step: event.step,
              reason: event.reason,
              usage: event.usage,
            },
            nativeEvent: raw,
          },
        ],
        ...(typeof reason === 'string' ? { turnEndKind: reason } : {}),
      }
    }
    case 'tool_call': {
      const tool = typeof event.tool === 'string' ? event.tool : 'unknown'
      return {
        events: [
          {
            type: toolEventType(tool),
            data: { callId: event.callId, tool, input: event.input },
            nativeEvent: raw,
          },
        ],
      }
    }
    case 'tool_result':
      return {
        events: [
          {
            type: 'tool/result',
            data: {
              callId: event.callId,
              status: event.status,
              result: event.result,
            },
            nativeEvent: raw,
          },
        ],
      }
    case 'final': {
      const finalText = typeof event.text === 'string' ? event.text : ''
      return {
        events: [
          {
            type: 'worker/message',
            data: { kind: 'final', text: finalText },
            nativeEvent: raw,
          },
        ],
        finalText,
      }
    }
    case 'error': {
      const errorMessage = typeof event.message === 'string' ? event.message : 'DeepSeek Harness failed'
      return {
        events: [
          {
            type: 'worker/message',
            data: { kind: 'diagnostic', level: 'error', message: errorMessage },
            nativeEvent: raw,
          },
        ],
        errorMessage,
      }
    }
    default:
      return { events: [] }
  }
}

