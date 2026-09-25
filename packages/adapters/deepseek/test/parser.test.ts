import { describe, expect, it } from 'vitest'
import { parseDeepSeekLine } from '../src/index.js'

describe('DeepSeek headless event parser', () => {
  it('maps the documented JSON stream into stable Relay events', () => {
    expect(parseDeepSeekLine('{"type":"session","sessionId":"session-1","cwd":"/tmp"}'))
      .toMatchObject({ sessionId: 'session-1', events: [] })
    expect(
      parseDeepSeekLine(
        '{"type":"tool_call","callId":"c1","tool":"str_replace_editor","input":{"path":"a.ts"}}',
      ).events[0],
    ).toMatchObject({ type: 'tool/edit', data: { callId: 'c1' } })
    expect(
      parseDeepSeekLine(
        '{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"max-tokens"}}',
      ),
    ).toMatchObject({ turnEndKind: 'max-tokens' })
  })

  it('keeps malformed native output as a bounded diagnostic', () => {
    const parsed = parseDeepSeekLine('not-json')
    expect(parsed.events[0]).toMatchObject({
      type: 'worker/message',
      data: { kind: 'diagnostic' },
    })
  })
})

