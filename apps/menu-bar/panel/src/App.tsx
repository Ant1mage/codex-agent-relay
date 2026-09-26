import { useCallback, useEffect, useMemo, useState } from 'react'
import { createTranslator } from '@relay/i18n'
import type { AgentProfile, RelayPolicy, RelayPolicyOverride } from '@relay/protocol'
import type { CodexStatus, InspectorSnapshot, RelayConfigView } from '@relay/relay-api'
import { AlertTriangle, ExternalLink, RefreshCw, ScrollText, X } from 'lucide-react'
import { Badge } from './components/ui/badge.js'
import { Button } from './components/ui/button.js'
import { ScrollArea } from './components/ui/scroll-area.js'
import { Spinner } from './components/ui/spinner.js'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './components/ui/tabs.js'
import {
  connection,
  initialIntent,
  initialProfileId,
  initialTab,
  locale,
  type PanelIntent,
  type PanelTab,
} from './lib/api.js'
import { AgentsView } from './views/agents.js'
import { CodexView } from './views/codex.js'
import { PolicyView } from './views/policy.js'
import { RuntimeView } from './views/runtime.js'

/**
 * Relay's control panel: everything that configures Relay itself lives here, in
 * the menu bar's own window. The web inspector stays a viewer
 * (see docs/architecture.md).
 *
 * Layout rules that keep it readable in a 420px popover: one column, every text
 * node either truncates or wraps, and no element may set its own width from
 * content (that is what overflowed the Codex tab before).
 */
export function App() {
  const conn = useMemo(() => connection(), [])
  const t = useMemo(() => createTranslator(locale()), [])
  const [tab, setTab] = useState<PanelTab>(() => initialTab())
  const [intent, setIntent] = useState<PanelIntent | undefined>(() => initialIntent())
  const [intentProfileId, setIntentProfileId] = useState<string | undefined>(() => initialProfileId())
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

  // The tray navigates the open panel through the preload instead of reloading it.
  useEffect(() => {
    return window.relayPanel?.onNavigate((request) => {
      if (request.tab) setTab(request.tab)
      if (request.intent) setIntent(request.intent)
      setIntentProfileId(request.profileId)
      void reload()
    })
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
        setIntent(undefined)
        setNotice(t('panel.saved'))
      }),
    [conn.client, guard, t],
  )

  const deleteProfile = useCallback(
    (profileId: string) =>
      guard(async () => {
        setConfig(await conn.client!.deleteProfile(profileId))
        setIntent(undefined)
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

  const saveRuntime = useCallback(
    (entry: { id: string; adapterId: string; executablePath: string; label?: string }) =>
      guard(async () => {
        const result = await conn.client!.saveRuntime(entry)
        setConfig(result.config)
        await reload()
        setNotice(result.probe.ok ? `${t('panel.saved')} · ${result.probe.version ?? ''}` : result.probe.error)
      }).then(() => undefined),
    [conn.client, guard, reload, t],
  )

  const deleteRuntime = useCallback(
    (runtimeId: string) =>
      guard(async () => {
        setConfig(await conn.client!.deleteRuntime(runtimeId))
        await reload()
        setNotice(t('panel.saved'))
      }),
    [conn.client, guard, reload, t],
  )

  const runCodex = useCallback(
    (action: 'install' | 'repair' | 'update' | 'remove') =>
      guard(async () => {
        const result = await conn.client!.codex(action)
        setCodex(result.status)
        setNotice(result.messages.join(' · '))
        await reload()
      }),
    [conn.client, guard, reload],
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

  const loadAdapters = useCallback(
    async () => (await conn.client!.adapters()).adapters,
    [conn.client],
  )

  const openInspector = useCallback(() => {
    void window.relayPanel?.openInspector()
  }, [])

  if (conn.error || !conn.client) {
    return (
      <div className="panel-shell items-center justify-center p-6 text-sm text-muted-foreground">
        {conn.error ?? t('panel.daemonDown')}
      </div>
    )
  }

  const tabs: Array<{ id: PanelTab; label: string }> = [
    { id: 'agents', label: t('nav.agents') },
    { id: 'policy', label: t('panel.policy') },
    { id: 'codex', label: t('menu.codex') },
    { id: 'runtime', label: t('panel.runtime') },
  ]

  return (
    <Tabs
      value={tab}
      onValueChange={(value) => {
        setTab(value as PanelTab)
        setIntent(undefined)
      }}
      className="panel-shell gap-0"
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <span className="mark-tinted size-3.5 shrink-0 text-foreground" role="img" aria-label="Relay" />
        <span className="shrink-0 text-xs font-semibold">Relay</span>
        {snapshot && (
          <Badge variant={snapshot.codex.configured ? 'secondary' : 'destructive'} className="shrink-0 text-[10px]">
            {snapshot.codex.configured ? t('menu.codexConnected') : t('menu.codexMissing')}
          </Badge>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1">
          <Button variant="ghost" size="icon-xs" title={t('panel.openInspector')} onClick={openInspector}>
            <ExternalLink />
          </Button>
          <Button variant="ghost" size="icon-xs" title={t('common.refresh')} onClick={() => void reload()}>
            {busy ? <Spinner /> : <RefreshCw />}
          </Button>
          <Button variant="ghost" size="icon-xs" title={t('panel.close')} onClick={() => window.close()}>
            <X />
          </Button>
        </span>
      </header>

      <TabsList className="h-auto w-full shrink-0 rounded-none border-b px-2 py-1.5">
        {tabs.map((item) => (
          <TabsTrigger
            key={item.id}
            value={item.id}
            className="min-w-0 truncate text-xs"
          >
            {item.label}
          </TabsTrigger>
        ))}
      </TabsList>

      {config && config.warnings.length > 0 && (
        <div className="shrink-0 space-y-1 border-b border-destructive/40 bg-destructive/10 px-3 py-2">
          {config.warnings.map((warning) => (
            <p key={warning} className="flex gap-1.5 text-[11px] leading-snug text-destructive">
              <AlertTriangle className="mt-0.5 size-3 shrink-0" />
              <span className="min-w-0 break-words">{warning}</span>
            </p>
          ))}
        </div>
      )}

      <ScrollArea className="min-h-0 flex-1">
        <div className="min-w-0">
          {error && <p className="break-words px-3 pt-3 text-xs text-destructive">{error}</p>}
          <TabsContent value="agents" className="m-0 min-w-0 p-3">
            {config && (
              <AgentsView
                t={t}
                client={conn.client}
                config={config}
                runtimes={snapshot?.runtimes ?? []}
                startNew={intent === 'new-agent'}
                editProfileId={intent === 'edit-agent' ? intentProfileId : undefined}
                onIntentHandled={() => {
                  setIntent(undefined)
                  setIntentProfileId(undefined)
                }}
                onSave={saveProfile}
                onDelete={deleteProfile}
              />
            )}
          </TabsContent>
          <TabsContent value="policy" className="m-0 min-w-0 p-3">
            {config && (
              <PolicyView
                t={t}
                config={config}
                workspaces={[...new Set((snapshot?.sessions ?? []).map((session) => session.session.cwd))]}
                onSave={savePolicy}
              />
            )}
          </TabsContent>
          <TabsContent value="codex" className="m-0 min-w-0 p-3">
            {codex && (
              <CodexView
                t={t}
                status={codex}
                onAction={runCodex}
                busy={busy}
                highlightActions={intent === 'codex-actions'}
              />
            )}
          </TabsContent>
          <TabsContent value="runtime" className="m-0 min-w-0 p-3">
            {snapshot && config && (
              <RuntimeView
                t={t}
                snapshot={snapshot}
                manualRuntimes={config.manualRuntimes}
                onRescan={rescan}
                onSave={saveRuntime}
                onDelete={deleteRuntime}
                onProbe={(input) => conn.client!.probeRuntime(input)}
                onLoadAdapters={loadAdapters}
                startNew={intent === 'add-runtime'}
                onIntentHandled={() => setIntent(undefined)}
                busy={busy}
              />
            )}
          </TabsContent>
        </div>
      </ScrollArea>

      {notice && (
        <footer className="flex shrink-0 items-start gap-2 border-t border-border px-3 py-1.5 text-[11px] text-muted-foreground">
          <ScrollText className="mt-0.5 size-3 shrink-0" />
          <span className="min-w-0 line-clamp-2 break-words">{notice}</span>
        </footer>
      )}
    </Tabs>
  )
}
