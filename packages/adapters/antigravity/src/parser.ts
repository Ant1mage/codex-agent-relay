import type { AdapterEvent } from '@relay/adapter-sdk'

export interface ParsedAntigravityLine {
  events: AdapterEvent[]
  sessionId?: string
  finalText?: string
  resultStatus?: string
  errorMessage?: string
}

function toolEventType(tool: string): AdapterEvent['type'] {
  const name = tool.toLowerCase()
  if (/search|grep|glob|find/.test(name)) return 'tool/search'
  if (/edit|write|patch|replace|delete|move/.test(name)) return 'tool/edit'
  if (/read|view|open|list/.test(name)) return 'tool/read'
  if (/test/.test(name)) return 'test/result'
  return 'tool/command'
}

export function parseAntigravityLine(line: string): ParsedAntigravityLine {
  let raw: unknown
  try {
    raw = JSON.parse(line)
  } catch {
    return {
      events: [{
        type: 'worker/message',
        data: { kind: 'diagnostic', message: 'Antigravity emitted malformed JSON' },
        nativeEvent: line.slice(0, 8_192),
      }],
    }
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return { events: [] }
  const event = raw as Record<string, unknown>
  if (event.event === 'init') {
    return {
      events: [{ type: 'worker/message', data: { kind: 'status', phase: 'initialized' }, nativeEvent: raw }],
      ...(typeof event.conversation_id === 'string' ? { sessionId: event.conversation_id } : {}),
    }
  }
  if (event.event === 'step_update' && event.step_update && typeof event.step_update === 'object') {
    const step = event.step_update as Record<string, unknown>
    const conversationId = typeof step.conversation_id === 'string' ? step.conversation_id : undefined
    if (step.subagent_info && typeof step.subagent_info === 'object') {
      return {
        events: [{
          type: step.state === 'DONE' ? 'child/completed' : 'child/started',
          data: step.subagent_info,
          nativeEvent: raw,
        }],
        ...(conversationId ? { sessionId: conversationId } : {}),
      }
    }
    if (step.step_type === 'agent_response' && typeof step.text_delta === 'string') {
      return {
        events: [{
          type: 'worker/message',
          data: { kind: 'delta', text: step.text_delta, state: step.state },
          nativeEvent: raw,
        }],
        ...(conversationId ? { sessionId: conversationId } : {}),
      }
    }
    if (step.step_type === 'tool') {
      const tool = typeof step.tool_name === 'string' ? step.tool_name : 'unknown'
      const toolInfo = step.tool_info && typeof step.tool_info === 'object'
        ? step.tool_info as Record<string, unknown>
        : {}
      const events: AdapterEvent[] = [{
        type: toolEventType(tool),
        data: { tool, state: step.state, parameters: toolInfo.parameters },
        nativeEvent: raw,
      }]
      if (step.state === 'DONE') {
        events.push({
          type: 'tool/result',
          data: { tool, output: toolInfo.output, error: toolInfo.error },
          nativeEvent: raw,
        })
      }
      return { events, ...(conversationId ? { sessionId: conversationId } : {}) }
    }
    return {
      events: [{ type: 'worker/message', data: { kind: 'status', ...step }, nativeEvent: raw }],
      ...(conversationId ? { sessionId: conversationId } : {}),
    }
  }
  if (event.event === 'result' && event.result && typeof event.result === 'object') {
    const result = event.result as Record<string, unknown>
    const finalText = typeof result.response === 'string' ? result.response : ''
    return {
      events: finalText
        ? [{ type: 'worker/message', data: { kind: 'final', text: finalText }, nativeEvent: raw }]
        : [],
      ...(typeof result.conversation_id === 'string' ? { sessionId: result.conversation_id } : {}),
      finalText,
      ...(typeof result.status === 'string' ? { resultStatus: result.status } : {}),
      ...(typeof result.error === 'string' ? { errorMessage: result.error } : {}),
    }
  }
  return { events: [] }
}
