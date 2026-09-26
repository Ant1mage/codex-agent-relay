import { RelayClient } from '@relay/relay-api'

/**
 * The panel is served by the daemon, so the API is same-origin and the only
 * thing it needs is the token, which the tray puts in the URL fragment.
 * Nothing is persisted: the panel is a view over the daemon.
 */
export interface PanelConnection {
  client?: RelayClient
  base?: string
  error?: string
}

export function parseToken(href: string): string | undefined {
  const match = /[#&?]t=([^&]+)/.exec(href)
  if (!match?.[1]) return undefined
  const value = decodeURIComponent(match[1]).trim()
  return value.length > 0 ? value : undefined
}

export function connection(
  href = window.location.href,
  search = window.location.search,
): PanelConnection {
  const params = new URLSearchParams(search)
  const base = params.get('base') ?? window.location.origin
  const token = parseToken(href)
  if (!token) return { error: '面板缺少 daemon 令牌' }
  return { client: new RelayClient({ baseUrl: base, token }), base }
}

export function locale(search = window.location.search): 'en' | 'zh-CN' {
  return new URLSearchParams(search).get('lang') === 'en' ? 'en' : 'zh-CN'
}

export function initialTab(search = window.location.search): 'agents' | 'policy' | 'codex' | 'runtime' {
  const tab = new URLSearchParams(search).get('tab')
  return tab === 'policy' || tab === 'codex' || tab === 'runtime' ? tab : 'agents'
}
