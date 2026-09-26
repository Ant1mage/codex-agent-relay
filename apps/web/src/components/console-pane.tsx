import { useEffect, useMemo, useRef } from 'react'
import { Square } from 'lucide-react'
import { Badge } from './ui/badge.js'
import { Button } from './ui/button.js'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from './ui/empty.js'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs.js'
import { ToggleGroup, ToggleGroupItem } from './ui/toggle-group.js'
import { cn } from '../lib/utils.js'
import { eventsForStep } from '../lib/step-events.js'
import {
  changedFiles,
  consoleRows,
  elapsed,
  statusGlyph,
  visibleEvents,
  type ConsoleKind,
  type Translator,
} from '../lib/format.js'
import { useAppStore } from '../store.js'

/** Left accent per Console category, matching the desktop console. */
const ACCENT: Partial<Record<ConsoleKind, string>> = {
  edit: 'border-l-warn',
  test: 'border-l-ok',
  result: 'border-l-ok',
  warning: 'border-l-warn',
  error: 'border-l-danger',
}

/** The raw view is a debugging aid, not a dump: keep the tail bounded. */
const RAW_LIMIT = 400

function ConsoleRows({ t, events }: { t: Translator; events: ReturnType<typeof consoleRows> }) {
  return (
    <div className="divide-y divide-border/60">
      {events.map((row) => (
        <div
          key={row.id}
          className={cn(
            'log-row grid min-h-[34px] grid-cols-[68px_74px_minmax(0,1fr)] items-baseline gap-3 px-4 py-1.5 hover:bg-muted/40',
            ACCENT[row.kind],
          )}
        >
          <time className="text-[11px] tabular-nums text-faint">
            {new Date(row.timestamp).toLocaleTimeString([], {
              hour: '2-digit',
              minute: '2-digit',
              second: '2-digit',
              hour12: false,
            })}
          </time>
          <span className="text-[10px] font-semibold uppercase tracking-[0.06em] text-muted-foreground">
            {t(`console.${row.kind}`)}
          </span>
          <code className="whitespace-pre-wrap break-words font-mono text-[12px] leading-[1.55]">
            {row.label}
            {row.count ? <i className="not-italic text-faint"> {row.count} {t('console.files')}</i> : null}
            {row.additions !== undefined || row.deletions !== undefined ? (
              <i className="not-italic tabular-nums text-faint">
                {row.additions ? ` +${row.additions}` : ''}
                {row.deletions ? ` -${row.deletions}` : ''}
              </i>
            ) : null}
          </code>
        </div>
      ))}
    </div>
  )
}

/** The log itself: steps, observable activity, changes and raw events. */
export function ConsolePane({ t, onCancelWorker }: { t: Translator; onCancelWorker(workerSessionId: string): void }) {
  const snapshot = useAppStore((state) => state.snapshot)
  const allEvents = useAppStore((state) => state.events)
  const selectedSessionId = useAppStore((state) => state.selectedSessionId)
  const selectedRunId = useAppStore((state) => state.selectedRunId)
  const selectedStepId = useAppStore((state) => state.selectedStepId)
  const selectStep = useAppStore((state) => state.selectStep)
  const tab = useAppStore((state) => state.tab)
  const setTab = useAppStore((state) => state.setTab)
  const scroller = useRef<HTMLDivElement>(null)
  const stick = useRef(true)

  const session = snapshot?.sessions.find((candidate) => candidate.session.id === selectedSessionId)
  const view = session?.runs.find((candidate) => candidate.run.id === selectedRunId)
  const step = view?.steps.find((candidate) => candidate.id === selectedStepId) ?? view?.steps[0]
  const events = useMemo(
    () => (view && step ? eventsForStep(view, step.id, allEvents[view.run.id] ?? []) : []),
    [view, step, allEvents],
  )
  const rows = useMemo(() => consoleRows(events), [events])
  const worker = view?.workers.filter((candidate) => candidate.stepId === step?.id).at(-1)
  const changes = useMemo(() => changedFiles(events), [events])
  const raw = useMemo(() => visibleEvents(events).slice(-RAW_LIMIT).reverse(), [events])

  useEffect(() => {
    const node = scroller.current
    if (!node || !stick.current) return
    node.scrollTop = node.scrollHeight
  }, [rows.length, tab])

  if (!view || !step) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center p-8">
        <Empty className="border-0">
          <EmptyHeader>
            <EmptyTitle>{t('inspector.noRun')}</EmptyTitle>
            <EmptyDescription>{t('inspector.noRunHint')}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    )
  }

  const running = worker?.status === 'running' || worker?.status === 'starting'

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
        <Badge variant="secondary" className="gap-1.5">
          <i className={cn('not-italic', step.status === 'running' ? 'text-[var(--green)]' : 'text-muted-foreground')}>
            {statusGlyph(step.status)}
          </i>
          {t(`run.status.${step.status}`)}
        </Badge>
        <span className="text-xs text-muted-foreground">
          {worker?.runtimeId ?? t('inspector.stopWorker')}
        </span>
        <span className="text-xs text-muted-foreground">
          {elapsed(step.createdAt, worker?.endedAt)}
        </span>
        <span className="ml-auto flex items-center gap-2">
          <span className="hidden text-[11px] text-muted-foreground sm:inline">
            {rows.length} · {events.length} {t('inspector.events')}
          </span>
          {running && worker && (
            <Button variant="outline" size="sm" onClick={() => onCancelWorker(worker.id)}>
              <Square /> {t('inspector.stopWorker')}
            </Button>
          )}
        </span>
      </div>

      {view.steps.length > 1 && (
        <div className="border-b border-border px-4 py-2">
          <ToggleGroup
            type="single"
            value={step.id}
            onValueChange={(value) => value && selectStep(value)}
            className="flex-wrap justify-start gap-1"
          >
            {view.steps.map((candidate) => (
              <ToggleGroupItem key={candidate.id} value={candidate.id} className="gap-1.5 text-xs">
                <span>{t('steps.step')} {candidate.iteration}</span>
                <i className="not-italic text-muted-foreground">{statusGlyph(candidate.status)}</i>
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
      )}

      <Tabs
        value={tab}
        onValueChange={(value) => setTab(value as 'console' | 'changes' | 'raw')}
        className="flex min-h-0 flex-1 flex-col gap-0"
      >
        <div className="flex items-center gap-3 border-b border-border px-4 py-1.5">
          <TabsList>
            <TabsTrigger value="console">{t('console.title')}</TabsTrigger>
            <TabsTrigger value="changes">
              {t('console.changes')}
              {changes.length > 0 && <span className="ml-1 tabular-nums text-muted-foreground">{changes.length}</span>}
            </TabsTrigger>
            <TabsTrigger value="raw">{t('console.rawOutput')}</TabsTrigger>
          </TabsList>
        </div>

        <TabsContent
          value="console"
          ref={scroller}
          onScroll={(event) => {
            const node = event.currentTarget
            stick.current = node.scrollHeight - node.scrollTop - node.clientHeight < 60
          }}
          className="min-h-0 flex-1 overflow-auto"
        >
          {rows.length === 0 ? (
            <p className="px-4 py-6 text-sm text-muted-foreground">{t('console.empty')}</p>
          ) : (
            <ConsoleRows t={t} events={rows} />
          )}
        </TabsContent>

        <TabsContent value="changes" className="min-h-0 flex-1 overflow-auto">
          {changes.length === 0 ? (
            <p className="px-4 py-6 text-sm text-muted-foreground">{t('console.noChanges')}</p>
          ) : (
            <ul className="divide-y divide-border/60">
              {changes.map((file) => (
                <li key={file.path} className="flex items-center gap-3 px-4 py-2">
                  <code className="min-w-0 flex-1 truncate font-mono text-[12px]">{file.path}</code>
                  <span className="text-[11px] tabular-nums text-[var(--green)]">
                    {file.additions ? `+${file.additions}` : ''}
                  </span>
                  <span className="text-[11px] tabular-nums text-destructive">
                    {file.deletions ? `-${file.deletions}` : ''}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </TabsContent>

        <TabsContent value="raw" className="min-h-0 flex-1 overflow-auto">
          {raw.length === 0 ? (
            <p className="px-4 py-6 text-sm text-muted-foreground">{t('console.noRawOutput')}</p>
          ) : (
            <pre className="px-4 py-3 font-mono text-[11px] leading-[1.5] text-muted-foreground">
              {JSON.stringify(raw, null, 2)}
            </pre>
          )}
        </TabsContent>
      </Tabs>
    </section>
  )
}
