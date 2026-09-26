import { describe, expect, it } from 'vitest'
import { panelConnectionChanged, panelUrl, type PanelTarget } from '../src/panel-target.js'

const target = (token: string, base = 'http://127.0.0.1:7352'): PanelTarget => ({
  base,
  token,
  lang: 'zh-CN',
  tab: 'runtime',
})

describe('panel daemon connection lifecycle', () => {
  it('keeps the token in the fragment', () => {
    const url = new URL(panelUrl(target('fresh-token')))
    expect(url.searchParams.has('token')).toBe(false)
    expect(url.hash).toBe('#t=fresh-token')
  })

  it('requires a renderer reload when relayd rotates token or base URL', () => {
    expect(panelConnectionChanged(target('old'), target('new'))).toBe(true)
    expect(panelConnectionChanged(target('same'), target('same'))).toBe(false)
    expect(panelConnectionChanged(target('same'), target('same', 'http://127.0.0.1:7353'))).toBe(true)
  })
})
