const STORAGE_KEY = 'relay.token'

/**
 * The daemon prints an inspector URL with the token in the fragment, e.g.
 * http://127.0.0.1:7352/#t=… — fragments never reach a server or a referrer, so
 * this is the one place the token can travel safely through a browser.
 */
export function parseToken(href: string): string | undefined {
  const match = /[#&?]t=([^&]+)/.exec(href)
  if (!match?.[1]) return undefined
  const value = decodeURIComponent(match[1]).trim()
  return value.length > 0 ? value : undefined
}

/** The same URL with the token removed, so it does not stay in the address bar. */
export function withoutToken(href: string): string {
  const url = new URL(href)
  url.searchParams.delete('t')
  const hash = url.hash.replace(/^#/, '').split('&').filter((part) => part && !part.startsWith('t='))
  url.hash = hash.length > 0 ? `#${hash.join('&')}` : ''
  return url.toString()
}

export interface TokenStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
}

/** Token for this page: URL fragment first (fresh from the tray), then storage. */
export function resolveToken(
  href: string,
  storage: TokenStorage,
): { token?: string | undefined; fromUrl: boolean } {
  const fromUrl = parseToken(href)
  if (fromUrl) {
    storage.setItem(STORAGE_KEY, fromUrl)
    return { token: fromUrl, fromUrl: true }
  }
  const stored = storage.getItem(STORAGE_KEY)
  return stored ? { token: stored, fromUrl: false } : { token: undefined, fromUrl: false }
}

export { STORAGE_KEY }
