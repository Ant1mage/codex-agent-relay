import { useCallback, useEffect, useMemo, useState } from 'react'
import { createTranslator } from '@relay/i18n'
import { Check, CircleAlert } from 'lucide-react'
import type { SessionView } from '@relay/relay-api'
import { ConsolePane } from './components/console-pane.js'
import { Header } from './components/header.js'
import { RunStrip } from './components/run-strip.js'
import { SessionRail } from './components/session-rail.js'
import { Button } from './components/ui/button.js'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from './components/ui/card.js'
import { createClient } from './lib/api.js'
import { buildPath, navigate, parsePath, replace, type Route } from './lib/router.js'
import { connectStream, useAppStore } from './store.js'

/** The run a session should open on: whatever is still moving, else the newest. */
function preferredRun(session: SessionView) {
  return (
    session.runs.find((view) => view.run.status === 'running') ??
    session.runs.find((view) => view.run.status === 'starting') ??
    session.runs.find((view) => view.run.status === 'awaiting_host') ??
    session.runs[0]
  )
}

function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() => parsePath(window.location.pathname))
  useEffect(() => {
    const sync = () => setRoute(parsePath(window.location.pathname))
    window.addEventListener('popstate', sync)
    return () => window.removeEventListener('popstate', sync)
  }, [])
  return route
}

function TokenScreen({ message, hint }: { message: string; hint: string }) {
  return (
    <div className="flex h-full items-center justify-center p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="text-base">{message}</CardTitle>
          <CardDescription className="leading-relaxed">{hint}</CardDescription>
        </CardHeader>
      </Card>
    </div>
  )
}

export function App() {
  const bootstrap = useMemo(createClient, [])
  const route = useRoute()
  const locale = useAppStore((state) => state.locale)
  const snapshot = useAppStore((state) => state.snapshot)
  const notice = useAppStore((state) => state.notice)
  const setNotice = useAppStore((state) => state.setNotice)
  const selectedRunId = useAppStore((state) => state.selectedRunId)
  const [refreshing, setRefreshing] = useState(false)
  const t = useMemo(() => createTranslator(locale), [locale])
  const hasSnapshot = Boolean(snapshot)
  const routeKey = `${route.sessionId ?? ''}|${route.runId ?? ''}`

  useEffect(() => {
    document.documentElement.lang = locale
  }, [locale])

  // The inspector has no theme switch: it follows the browser, like any log view.
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const sync = () => {
      document.documentElement.classList.toggle('dark', media.matches)
      document.documentElement.style.colorScheme = media.matches ? 'dark' : 'light'
    }
    sync()
    media.addEventListener('change', sync)
    return () => media.removeEventListener('change', sync)
  }, [])

  // One stream per page: the store owns the merge, the effect owns the socket.
  useEffect(() => {
    if (!bootstrap.client) return
    return connectStream(bootstrap.client)
  }, [bootstrap.client])

  /*
   * The URL decides what is selected. A deep link from the tray (session or run)
   * wins; anything unresolved falls back to the newest session and its live run,
   * and the address bar is rewritten to match what is actually on screen.
   */
  useEffect(() => {
    if (!snapshot || snapshot.sessions.length === 0) return
    const session =
      snapshot.sessions.find((candidate) => candidate.session.id === route.sessionId) ?? snapshot.sessions[0]
    if (!session) return
    const run =
      (route.runId ? session.runs.find((candidate) => candidate.run.id === route.runId) : undefined) ??
      preferredRun(session)
    const stepId = useAppStore.getState().selectedStepId
    const stepExists = run?.steps.some((step) => step.id === stepId)
    useAppStore
      .getState()
      .select(session.session.id, run?.run.id, stepExists ? stepId : run?.steps[0]?.id)
    const canonical = buildPath({ sessionId: session.session.id, runId: run?.run.id })
    if (canonical !== window.location.pathname) replace({ sessionId: session.session.id, runId: run?.run.id })
  }, [routeKey, hasSnapshot])

  // History is pulled per run; the stream only carries what happens next.
  useEffect(() => {
    if (!bootstrap.client || !selectedRunId) return
    const existing = useAppStore.getState().events[selectedRunId]
    if (existing && existing.length > 0) return
    void bootstrap.client
      .events(selectedRunId)
      .then((batch) => useAppStore.getState().mergeRunEvents(batch.runId, batch.events))
      .catch((error: unknown) =>
        useAppStore.getState().setNotice(error instanceof Error ? error.message : String(error)),
      )
  }, [bootstrap.client, selectedRunId])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(undefined), 3_500)
    return () => window.clearTimeout(timer)
  }, [notice, setNotice])

  const refresh = useCallback(async () => {
    if (!bootstrap.client) return
    setRefreshing(true)
    try {
      useAppStore.getState().setSnapshot(await bootstrap.client.snapshot())
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error))
    } finally {
      setRefreshing(false)
    }
  }, [bootstrap.client, setNotice])

  const copyDiagnostics = useCallback(async () => {
    if (!bootstrap.client) return
    try {
      await navigator.clipboard.writeText(await bootstrap.client.diagnostics())
      setNotice(t('inspector.diagnosticsCopied'))
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error))
    }
  }, [bootstrap.client, setNotice, t])

  const cancelWorker = useCallback(
    async (workerSessionId: string) => {
      if (!bootstrap.client) return
      try {
        await bootstrap.client.cancelWorker(workerSessionId)
        setNotice(t('inspector.stopped'))
      } catch (error) {
        setNotice(error instanceof Error ? error.message : String(error))
      }
    },
    [bootstrap.client, setNotice, t],
  )

  if (bootstrap.missingToken) {
    return <TokenScreen message={t('inspector.tokenTitle')} hint={t('inspector.tokenBody')} />
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background text-foreground">
      <Header
        t={t}
        onRefresh={() => void refresh()}
        onCopyDiagnostics={() => void copyDiagnostics()}
        refreshing={refreshing}
      />

      {snapshot && !snapshot.codex.configured && (
        <div className="flex flex-wrap items-center gap-3 border-b border-border bg-[var(--amber)]/10 px-4 py-2 text-xs">
          <CircleAlert className="size-3.5 text-[var(--amber)]" />
          <span className="font-medium">{t('inspector.codexMissing')}</span>
          <span className="text-muted-foreground">{t('inspector.codexMissingBody')}</span>
          {/* Installing into Codex is configuration: it belongs to the menu bar. */}
          <span className="ml-auto text-muted-foreground">{t('inspector.codexFixInMenuBar')}</span>
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[248px_minmax(0,1fr)]">
        <div className="max-h-[38vh] min-h-0 md:max-h-none">
          <SessionRail
            t={t}
            onSelect={(sessionId) => {
              const session = snapshot?.sessions.find((candidate) => candidate.session.id === sessionId)
              const run = session ? preferredRun(session) : undefined
              navigate({ sessionId, runId: run?.run.id })
            }}
          />
        </div>
        <main className="flex min-h-0 flex-col">
          <RunStrip t={t} onSelect={(runId) => navigate({ sessionId: route.sessionId, runId })} />
          <ConsolePane t={t} onCancelWorker={(id) => void cancelWorker(id)} />
        </main>
      </div>

      {notice && (
        <div className="pointer-events-none fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-md border border-border bg-popover px-3 py-2 text-xs shadow-lg">
          <Check className="size-3.5 text-[var(--green)]" />
          {notice}
        </div>
      )}
    </div>
  )
}
