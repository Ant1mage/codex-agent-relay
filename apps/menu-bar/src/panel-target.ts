export type PanelTab = 'agents' | 'policy' | 'codex' | 'runtime'
export type PanelIntent = 'new-agent' | 'edit-agent' | 'add-runtime' | 'codex-actions'

export interface PanelTarget {
  /** Daemon base URL, e.g. http://127.0.0.1:7352 */
  base: string
  token: string
  lang: string
  tab?: PanelTab
  intent?: PanelIntent
  profileId?: string
}

export function panelUrl(target: PanelTarget): string {
  const dev = process.env.RELAY_PANEL_DEV_URL
  const url = new URL(dev ?? `${target.base}/panel/`)
  if (dev) url.searchParams.set('base', target.base)
  url.searchParams.set('lang', target.lang)
  if (target.tab) url.searchParams.set('tab', target.tab)
  if (target.intent) url.searchParams.set('intent', target.intent)
  if (target.profileId) url.searchParams.set('profileId', target.profileId)
  // The token rides the fragment: it never reaches the server or a referrer.
  url.hash = `t=${target.token}`
  return url.toString()
}

/** A daemon restart may reuse its port but always rotates its token. */
export function panelConnectionChanged(
  current: PanelTarget | undefined,
  next: PanelTarget,
): boolean {
  return current?.base !== next.base || current.token !== next.token
}
