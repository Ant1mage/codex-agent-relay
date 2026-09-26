import { useCallback, useEffect, useMemo, useState } from 'react'
import { createTranslator } from '@relay/i18n'
import type { AgentProfile, RelayPolicy, RelayPolicyOverride } from '@relay/protocol'
import type { CodexStatus, InspectorSnapshot, RelayConfigView } from '@relay/relay-api'
import { ExternalLink, RefreshCw, ScrollText, X } from 'lucide-react'
import { Badge } from './components/ui/badge.js'
import { Button } from './components/ui/button.js'
import { ScrollArea } from './components/ui/scroll-area.js'
import { cn } from './lib/utils.js'
import { connection, initialTab, locale } from './lib/api.js'
import { AgentsView } from './views/agents.js'
import { CodexView } from './views/codex.js'
import { PolicyView } from './views/policy.js'
import { RuntimeView } from './views/runtime.js'

type Tab = 'agents' | 'policy' | 'codex' | 'runtime'

/**
 * Relay's control panel: everything that configures Relay itself lives here, in
 * the menu bar's own window. The web inspector stays a viewer
 * (docs/menu-bar.md 1).
 */
export function App() {
  const conn = useMemo(() => connection(), [])
  const t = useMemo(() => createTranslator(locale()), [])
  const [tab, setTab] = useState<Tab>(() => initialTab())
  const [config, setConfig] = useState<RelayConfigView>()
  const [snapshot, setSnapshot] = useState<InspectorSnapshot>()
  const [codex, setCodex] = useState<CodexStatus>()
  const [notice, setNotice] = useState<string>()
  const [error, setError] = useState<string>()
  const [busy, setBusy] = useState(false)

  const reload = useCallback(async () => {
    if (!conn.client) return
    try {
      const [nextConfig, nextSnapshot] = await Promise.all([conn.client.config(), conn.client.snapshot()])
      setConfig(nextConfig)
      setSnapshot(nextSnapshot)
      setCodex(nextSnapshot.codex)
      setError(undefined)
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure))
    }
  }, [conn.client])

  useEffect(() => {
    void reload()
  }, [reload])

  const guard = useCallback(
    async (action: () => Promise<void>) => {
      setBusy(true)
      try {
        await action()
      } catch (failure) {
        setNotice(`${t('panel.saveFailed')}: ${failure instanceof Error ? failure.message : String(failure)}`)
      } finally {
        setBusy(false)
      }
    },
    [t],
  )

  const saveProfile = useCallback(
    (profile: AgentProfile) =>
      guard(async () => {
        setConfig(await conn.client!.saveProfile(profile))
        setNotice(t('panel.saved'))
      }),
    [conn.client, guard, t],
  )

  const deleteProfile = useCallback(
    (profileId: string) =>
      guard(async () => {
        setConfig(await conn.client!.deleteProfile(profileId))
        setNotice(t('panel.saved'))
      }),
    [conn.client, guard, t],
  )

  const savePolicy = useCallback(
    (policy: RelayPolicy, workspaceOverrides: Record<string, RelayPolicyOverride>) =>
      guard(async () => {
        setConfig(await conn.client!.savePolicy({ policy, workspaceOverrides }))
        setNotice(t('panel.saved'))
      }),
    [conn.client, guard, t],
  )

  const runCodex = useCallback(
    (action: 'install' | 'repair' | 'update' | 'remove') =>
      guard(async () => {
        const result = await conn.client!.codex(action)
        setCodex(result.status)
        setNotice(result.messages.join(' · '))
        await reload()
      }),
    [conn.client, guard, reload, t],
  )

  const rescan = useCallback(
    () =>
      guard(async () => {
        const result = await conn.client!.refresh()
        await reload()
        setNotice(`${result.runtimes} runtimes · ${result.profiles} profiles`)
      }),
    [conn.client, guard, reload],
  )

  const openInspector = useCallback(() => {
    if (!conn.base) return
    const url = new URL(conn.base)
    window.open(url.toString(), '_blank')
  }, [conn.base])

  if (conn.error || !conn.client) {
    return (
      <div className="panel-shell items-center justify-center p-6 text-sm text-muted-foreground">
        {conn.error ?? t('panel.daemonDown')}
      </div>
    )
  }

  const tabs: Array<{ id: Tab; label: string }> = [
    { id: 'agents', label: t('nav.agents') },
    { id: 'policy', label: t('panel.policy') },
    { id: 'codex', label: t('menu.codex') },
    { id: 'runtime', label: t('panel.runtime') },
  ]

  return (
    <div className="panel-shell">
      <header className="flex items-center gap-2 border-b border-border px-3 py-2">
        <span className="mark-tinted size-3.5 text-foreground" role="img" aria-label="Relay" />
        <span className="text-xs font-semibold">Relay</span>
        {snapshot && (
          <Badge variant={snapshot.codex.configured ? 'secondary' : 'destructive'} className="text-[10px]">
            {snapshot.codex.configured ? t('menu.codexConnected') : t('menu.codexMissing')}
          </Badge>
        )}
        <span className="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="icon-xs" title={t('panel.openInspector')} onClick={openInspector}>
            <ExternalLink />
          </Button>
          <Button variant="ghost" size="icon-xs" title={t('common.refresh')} onClick={() => void reload()}>
            <RefreshCw className={busy ? 'spin' : undefined} />
          </Button>
          <Button variant="ghost" size="icon-xs" title={t('panel.close')} onClick={() => window.close()}>
            <X />
          </Button>
        </span>
      </header>

      <nav className="flex gap-1 border-b border-border px-2 py-1.5">
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => setTab(item.id)}
            className={cn(
              'rounded-md px-2.5 py-1 text-xs transition-colors',
              tab === item.id ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/60',
            )}
          >
            {item.label}
          </button>
        ))}
      </nav>

      <ScrollArea className="min-h-0 flex-1">
        <div className="p-3">
          {error && <p className="mb-2 text-xs text-destructive">{error}</p>}
          {tab === 'agents' && config && (
            <AgentsView
              t={t}
              client={conn.client}
              config={config}
              runtimes={snapshot?.runtimes ?? []}
              onSave={saveProfile}
              onDelete={deleteProfile}
            />
          )}
          {tab === 'policy' && config && (
            <PolicyView
              t={t}
              config={config}
              workspaces={[...new Set((snapshot?.sessions ?? []).map((session) => session.session.cwd))]}
              onSave={savePolicy}
            />
          )}
          {tab === 'codex' && codex && (
            <CodexView t={t} status={codex} onAction={runCodex} busy={busy} />
          )}
          {tab === 'runtime' && snapshot && (
            <RuntimeView t={t} snapshot={snapshot} onRescan={rescan} busy={busy} onOpenInspector={openInspector} />
          )}
        </div>
      </ScrollArea>

      {notice && (
        <footer className="flex items-center gap-2 border-t border-border px-3 py-1.5 text-[11px] text-muted-foreground">
          <ScrollText className="size-3" />
          <span className="line-clamp-2">{notice}</span>
        </footer>
      )}
    </div>
  )
}
