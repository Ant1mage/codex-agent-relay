export interface Route {
  // Explicitly undefined-able: callers build routes from possibly-missing ids
  // and the compiler runs with exactOptionalPropertyTypes.
  sessionId?: string | undefined
  runId?: string | undefined
}

/** /s/<session> and /s/<session>/r/<run> are the only two shapes the UI needs. */
export function parsePath(pathname: string): Route {
  const parts = pathname.split('/').filter((part) => part.length > 0)
  if (parts[0] !== 's' || !parts[1]) return {}
  const sessionId = decodeURIComponent(parts[1])
  if (parts[2] !== 'r' || !parts[3]) return { sessionId }
  return { sessionId, runId: decodeURIComponent(parts[3]) }
}

export function buildPath(route: Route): string {
  if (!route.sessionId) return '/'
  const base = `/s/${encodeURIComponent(route.sessionId)}`
  return route.runId ? `${base}/r/${encodeURIComponent(route.runId)}` : base
}

/**
 * URL is the source of truth for what is selected, so the tray can deep-link
 * into a session or a run and the browser back button keeps working.
 */
export function navigate(route: Route): void {
  const path = buildPath(route)
  if (window.location.pathname !== path) window.history.pushState(null, '', path)
  window.dispatchEvent(new PopStateEvent('popstate'))
}

export function replace(route: Route): void {
  const path = buildPath(route)
  if (window.location.pathname !== path) window.history.replaceState(null, '', path)
  window.dispatchEvent(new PopStateEvent('popstate'))
}

