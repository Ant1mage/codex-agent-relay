import { RelayClient } from '@relay/relay-api'
import { resolveToken, withoutToken } from './token.js'

export interface ClientBootstrap {
  client?: RelayClient
  /** Set when the page has no token yet; the UI then explains how to get one. */
  missingToken: boolean
}

/**
 * One client for the page. The inspector is served by the daemon, so the API is
 * same-origin; in development Vite proxies /api to the same place.
 */
export function createClient(): ClientBootstrap {
  const { token, fromUrl } = resolveToken(window.location.href, window.localStorage)
  if (fromUrl) {
    // The token has been stored; take it out of the address bar.
    window.history.replaceState(null, '', withoutToken(window.location.href))
  }
  if (!token) return { missingToken: true }
  const base = import.meta.env.VITE_RELAY_BASE ?? window.location.origin
  return { client: new RelayClient({ baseUrl: base, token }), missingToken: false }
}
