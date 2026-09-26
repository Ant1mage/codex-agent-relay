import { useEffect, useMemo, useState } from 'react'
import {
  Check,
  CircleAlert,
  Info,
  MoreHorizontal,
  RefreshCw,
  Settings as SettingsIcon,
  Square,
  X,
} from 'lucide-react'
import type { HostSession, Step, StepStatus } from '@relay/protocol'
import type { DesktopRunView } from '../../shared/api.js'
import type { CodexIntegrationStatus } from '../../shared/api.js'
import { useAppStore } from './store.js'
import { cn } from './lib/utils.js'
import { Badge } from './components/ui/badge.js'
import { Item, ItemContent, ItemGroup, ItemMedia, ItemTitle } from './components/ui/item.js'
import { ScrollArea } from './components/ui/scroll-area.js'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './components/ui/select.js'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from './components/ui/sheet.js'
import { Tabs, TabsList, TabsTrigger } from './components/ui/tabs.js'
import { Button } from './components/ui/button.js'
import {
  Sidebar as ShadcnSidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from './components/ui/sidebar.js'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from './components/ui/dropdown-menu.js'
import { createTranslator } from '@relay/i18n'
import { Onboarding } from './onboarding.js'
import { SettingsSheet } from './settings.js'
import {
  ConsoleRows,
  ProviderIcon,
  StatusLabel,
  consoleRows,
  elapsed,
  eventSummary,
  relativeTime,
  statusGlyph,
  storedFontSize,
  useRuntimeOptions,
  type Inspector,
  type Theme,
  type Translator,
} from './ui.js'

/* ------------------------------------------------------------------ */
/* Sidebar (docs/ui.md 4-5)                                            */
/* ------------------------------------------------------------------ */


/**
 * Relay's environment at a glance, in the window chrome beside the sidebar
 * control. It reports what was actually detected rather than a decorative
 * status, and doubles as the way back into setup once onboarding is done.
 */
function EnvironmentChip({ t }: { t: Translator }) {
  const { snapshot, openOnboarding } = useAppStore()
  const [codex, setCodex] = useState<CodexIntegrationStatus>()

  // Same checks the first-run flow performs, so the chip and setup agree.
  useEffect(() => { void window.relay.codexStatus().then(setCodex) }, [])

  const failed = (codex?.checks ?? []).filter((check) => !check.ok)
  const ready = Boolean(codex?.configured)
  const detail = (codex?.checks ?? [])
    .map((check) => `${check.ok ? '✓' : '✗'} ${t(`onboarding.check.${check.id}`)}${check.ok ? '' : ` — ${check.detail}`}`)
    .join('\n')

  return (
    <Button
      variant="ghost"
      size="xs"
      className="env-chip"
      title={detail}
      // Always the setup surface: this reports Codex integration state, and the
      // first-run flow is the only place that explains or fixes it.
      onClick={openOnboarding}
    >
      <span className={ready ? 'size-1.5 rounded-full bg-[var(--green)]' : 'size-1.5 rounded-full bg-[var(--amber)]'} aria-hidden="true" />
      <span>{ready ? t('env.ready') : t('env.needsSetup')}</span>
      {failed.length > 0 && <Badge variant="secondary" className="h-4 px-1.5 text-[9px]">{failed.length}</Badge>}
    </Button>
  )
}

function RelaySidebar({ t, macOS }: { t: Translator; macOS: boolean }) {
  const { snapshot, selectedSessionId, selectSession, setSettingsOpen } = useAppStore()
  const sessions = snapshot?.sessions ?? []
  return (
    <ShadcnSidebar collapsible="offcanvas">
      <SidebarHeader className={macOS ? 'pt-10' : undefined}>
        <div className="flex items-center gap-2 px-2 py-1 text-sm font-semibold">
          <span className="size-4 mark-tinted" role="img" aria-label="Relay" />
          <span>{t('app.name')}</span>
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>{t('sessions.title')}</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {sessions.map((session) => (
                <SidebarMenuItem key={session.id}>
                  <SidebarMenuButton
                    asChild
                    isActive={session.id === selectedSessionId}
                    tooltip={session.displayName}
                    className="h-auto items-start py-2"
                  >
                    <button type="button" onClick={() => selectSession(session.id)}>
                      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span>{session.displayName}</span>
                        <span className="text-xs text-muted-foreground">
                          {relativeTime(session.updatedAt, session.status, t)}
                        </span>
                      </span>
                    </button>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
              {!sessions.length && (
                <SidebarMenuItem>
                  <span className="block px-2 py-1 text-xs text-muted-foreground">{t('sessions.empty')}</span>
                </SidebarMenuItem>
              )}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton onClick={() => setSettingsOpen(true)} tooltip={t('settings.title')}>
              <SettingsIcon />
              <span>{t('settings.title')}</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </ShadcnSidebar>
  )
}

/**
 * Session contextual menu (docs/ui.md 6/19). Session-level controls live here so
 * the toolbar never carries a global Stop button.
 */
function SessionMenu({ session, view, t }: { session: HostSession; view: DesktopRunView | undefined; t: Translator }) {
  const { setNotice, refresh } = useAppStore()

  const stopAll = async () => {
    const result = await window.relay.cancelSessionWorkers(session.id)
    setNotice(result.count ? t('session.stopped').replace('{count}', String(result.count)) : t('session.stoppedNone'))
    await refresh()
  }
  const openWorkspace = async () => {
    const result = await window.relay.openWorkspace(session.cwd)
    if (!result.ok) setNotice(result.message ?? t('error.generic'))
  }
  const copyId = async () => {
    await navigator.clipboard.writeText(session.nativeSessionId)
    setNotice(t('session.idCopied'))
  }

  return (
    <div className="session-menu">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-xs" aria-label={t('session.menu')} title={t('session.menu')}>
            <MoreHorizontal size={16} />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => void stopAll()}>{t('session.stopAll')}</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void openWorkspace()}>{t('session.openWorkspace')}</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void copyId()}>{t('session.copyId')}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {view && <span className="session-run-status"><StatusLabel status={view.run.status} t={t} /></span>}
    </div>
  )
}

/* ------------------------------------------------------------------ */
/* Step (docs/ui.md 7-11)                                              */
/* ------------------------------------------------------------------ */

interface StepItem { view: DesktopRunView; step: Step }

/** A node is either one worker or a group of workers Codex ran concurrently. */
interface StepGroup { key: string; items: StepItem[] }

const ACTIVE_STEP_STATUSES: StepStatus[] = ['running', 'starting']

function stepIsActive(step: Step): boolean {
  return ACTIVE_STEP_STATUSES.includes(step.status)
}

/** Worker window in ms; an unfinished step is treated as still open. */
function workerWindow(item: StepItem): { start: number; end: number } {
  const worker = [...item.view.workers].reverse().find((candidate) => candidate.stepId === item.step.id)
  const start = new Date(item.step.createdAt).getTime()
  const end = worker?.endedAt ? new Date(worker.endedAt).getTime() : Number.POSITIVE_INFINITY
  return { start, end: Math.max(start, end) }
}

function stepsOverlap(a: StepItem, b: StepItem): boolean {
  const left = workerWindow(a)
  const right = workerWindow(b)
  return left.start <= right.end && right.start <= left.end
}

/**
 * Groups adjacent top-level workers that genuinely ran at the same time, so
 * concurrent work never reads as a sequential chain (docs/ui.md 10). Overlap is
 * proven by the worker time windows rather than inferred from the order alone.
 */
function groupSteps(items: StepItem[]): StepGroup[] {
  const ordered = [...items].sort((left, right) => left.step.createdAt.localeCompare(right.step.createdAt))
  const groups: StepGroup[] = []
  for (const item of ordered) {
    const last = groups[groups.length - 1]
    const canJoin = last !== undefined &&
      last.items.length < 3 &&
      last.items.some((member) => stepsOverlap(member, item))
    if (canJoin && last) last.items.push(item)
    else groups.push({ key: item.step.id, items: [item] })
  }
  return groups
}

/** One clickable worker tab inside a group or on its own. */
function StepNode({
  item,
  selected,
  active,
  t,
  registerRef,
}: {
  item: StepItem
  selected: boolean
  active: boolean
  t: Translator
  registerRef(element: HTMLButtonElement | null): void
}) {
  const { snapshot, selectStep } = useAppStore()
  const { view, step } = item
  const profile = snapshot?.profiles.find((candidate) => candidate.id === view.run.profileId)
  const runtime = snapshot?.runtimes.find((candidate) => candidate.id === profile?.runtimeId)
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const name = profile?.name ?? view.run.profileId
  const classes = ['step-node']
  if (selected) classes.push('selected')
  if (active) classes.push('current')
  classes.push(step.status)

  return (
    <button
      ref={registerRef}
      className={classes.join(' ')}
      onClick={() => selectStep(view.run.id, step.id)}
      title={`${name} · ${t(`run.status.${step.status}`)}`}
    >
      <span className="step-node-head">
        <i className={`step-glyph ${step.status}`} aria-hidden="true">{statusGlyph(step.status)}</i>
        <ProviderIcon name={profile?.name ?? view.run.profileId} adapterId={runtime?.adapterId} />
        <strong>{name}</strong>
      </span>
      <em>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</em>
    </button>
  )
}

/**
 * A node holding 2-3 workers Codex dispatched concurrently, so parallel work is
 * not drawn as a sequential chain (docs/ui.md 10).
 */
function StepGroupNode({
  group,
  t,
  selectedKey,
  activeKey,
  registerRef,
}: {
  group: StepGroup
  t: Translator
  selectedKey: string | undefined
  activeKey: string | undefined
  registerRef(element: HTMLButtonElement | null): void
}) {
  const { snapshot, selectStep } = useAppStore()
  const profiles = snapshot?.profiles ?? []
  const running = group.items.filter((item) => stepIsActive(item.step)).length
  const label = `${t('steps.parallel')} · ${
    running
      ? `${running} ${t('steps.running')}`
      : `${group.items.length}`
  }`

  return (
    <div className={`step-node step-parallel${activeKey === group.key ? ' current' : ''}`}>
      <span className="step-parallel-head">
        <i className="step-glyph running" aria-hidden="true">{statusGlyph('running')}</i>
        <strong>{label}</strong>
      </span>
      <div className="step-parallel-members">
        {group.items.map((item) => (
          <StepNode
            key={item.step.id}
            item={item}
            selected={selectedKey === item.step.id}
            active={activeKey === item.step.id}
            t={t}
            registerRef={registerRef}
          />
        ))}
      </div>
    </div>
  )
}

/**
 * Step is a horizontal navigator over the top-level workers of one run, which is
 * exactly a scrollable tab strip — so it uses Tabs rather than a hand-built row
 * of buttons (docs/ui.md 7.4, 20.1). TabsList keeps the selected trigger
 * scrolled into view and gives the pattern correct keyboard behaviour for free.
 */
function StepNavigator({ items, t }: { items: StepItem[]; t: Translator }) {
  const { selectedRunId, selectedStepId, selectStep } = useAppStore()
  const groups = useMemo(() => groupSteps(items), [items])

  // Tab values must be unique across runs; a step id alone can repeat.
  const valueOf = (item: StepItem) => `${item.view.run.id}:${item.step.id}`
  const current = items.find(
    (item) => item.view.run.id === selectedRunId && item.step.id === selectedStepId,
  )
  const value = current ? valueOf(current) : ''

  return (
    <Tabs
      value={value}
      onValueChange={(next) => {
        const target = items.find((item) => valueOf(item) === next)
        if (target) selectStep(target.view.run.id, target.step.id)
      }}
      className="step-strip"
    >
      <div className="step-strip-label">{t('steps.title')}</div>
      <TabsList variant="line" className="step-scroll" aria-label={t('steps.title')}>
        {groups.map((group) => {
          if (group.items.length === 1) {
            const item = group.items[0]!
            return (
              <TabsTrigger key={group.key} value={valueOf(item)} className={`step-node h-auto flex-none whitespace-normal ${item.step.status}${stepIsActive(item.step) ? ' current' : ''}`}>
                <StepNodeBody item={item} t={t} />
              </TabsTrigger>
            )
          }
          // Concurrent workers read as one bounded unit (docs/ui.md 10).
          return (
            <TabsTrigger
              key={group.key}
              value={valueOf(group.items[0]!)}
              className="step-node step-parallel h-auto flex-none whitespace-normal"
            >
              <span className="step-parallel-head">
                <i className="step-glyph running" aria-hidden="true">{statusGlyph('running')}</i>
                <strong>
                  {t('steps.parallel')} · {group.items.filter((i) => stepIsActive(i.step)).length || group.items.length}
                </strong>
              </span>
              <span className="step-parallel-members">
                {group.items.map((item) => (
                  <span key={item.step.id} className="step-parallel-member">
                    <StepNodeBody item={item} t={t} />
                  </span>
                ))}
              </span>
            </TabsTrigger>
          )
        })}
      </TabsList>
    </Tabs>
  )
}

/** The inner content of a Step node: status glyph, provider, name, timing. */
function StepNodeBody({ item, t }: { item: StepItem; t: Translator }) {
  const { snapshot } = useAppStore()
  const { view, step } = item
  const profile = snapshot?.profiles.find((candidate) => candidate.id === view.run.profileId)
  const runtime = snapshot?.runtimes.find((candidate) => candidate.id === profile?.runtimeId)
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  return (
    <>
      <span className="step-node-head">
        <i className={`step-glyph ${step.status}`} aria-hidden="true">{statusGlyph(step.status)}</i>
        <ProviderIcon name={profile?.name ?? view.run.profileId} adapterId={runtime?.adapterId} />
        <strong>{profile?.name ?? view.run.profileId}</strong>
      </span>
      <em>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</em>
    </>
  )
}

/* ------------------------------------------------------------------ */
/* Console (docs/ui.md 12-14)                                          */
/* ------------------------------------------------------------------ */

function Console({
  view,
  step,
  t,
  openInspector,
  cliInfoOpen,
  toggleCliInfo,
}: {
  view: DesktopRunView
  step: Step
  t: Translator
  openInspector(inspector: Exclude<Inspector, undefined>): void
  cliInfoOpen: boolean
  toggleCliInfo(): void
}) {
  const { setNotice, snapshot } = useAppStore()
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const events = view.events.filter((event) => !event.stepId || event.stepId === step.id)
  const rows = useMemo(() => consoleRows(events), [events])
  const rawCount = events.filter((event) => event.nativeEvent !== undefined).length
  const active = step.status === 'running' || step.status === 'starting'
  const profile = snapshot?.profiles.find((item) => item.id === view.run.profileId)

  return (
    <section className="console-panel">
      <header className="console-header">
        <div>
          <strong>{t('console.title')} · {profile?.name ?? view.run.profileId}</strong>
          <span>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</span>
        </div>
        <div className="console-actions">
          <Button variant="outline" size="xs" onClick={() => openInspector('raw')}>{t('console.rawOutput')} <span className="text-muted">{rawCount}</span></Button>
          {/* Stop is worker-level and sits beside the selected CLI (docs/ui.md 19) */}
          {active && worker && (
            <Button
              variant="outline"
              size="xs"
              className="text-danger"
              onClick={async () => {
                const response = await window.relay.cancelWorker(worker.id)
                setNotice(response.accepted ? t('runs.cancelQueued') : response.message)
              }}
            >
              <Square size={11} />{t('action.stop')}
            </Button>
          )}
          <Button
            variant={cliInfoOpen ? 'secondary' : 'ghost'}
            size="icon-xs"
            aria-pressed={cliInfoOpen}
            aria-label={cliInfoOpen ? t('cliInfo.hide') : t('cliInfo.show')}
            title={cliInfoOpen ? t('cliInfo.hide') : t('cliInfo.show')}
            onClick={toggleCliInfo}
          >
            <Info size={15} />
          </Button>
        </div>
      </header>
      <ScrollArea className="console-body min-h-0 h-full">
        {!rows.length && <div className="console-empty">{t('console.empty')}</div>}
        <ConsoleRows rows={rows} t={t} />
      </ScrollArea>
    </section>
  )
}

/* ------------------------------------------------------------------ */
/* CLI Info (docs/ui.md 15-16, 18)                                     */
/* ------------------------------------------------------------------ */

type Override = { model?: string; reasoning?: string }
const PROFILE_SELECTION = '__relay_profile_selection__'

function overrideKey(runId: string): string {
  return `relay.override.${runId}`
}

function CliInfo({ view, step, t, close, openInspector }: {
  view: DesktopRunView
  step: Step
  t: Translator
  close(): void
  openInspector(inspector: Exclude<Inspector, undefined>): void
}) {
  const { snapshot, setNotice } = useAppStore()
  const worker = [...view.workers].reverse().find((candidate) => candidate.stepId === step.id)
  const profile = snapshot?.profiles.find((candidate) => candidate.id === view.run.profileId)
  const runtime = snapshot?.runtimes.find((candidate) => candidate.id === worker?.runtimeId)
  const { options } = useRuntimeOptions(runtime?.id)
  const [override, setOverride] = useState<Override>(() => {
    try {
      return JSON.parse(localStorage.getItem(overrideKey(view.run.id)) ?? '{}') as Override
    } catch {
      return {}
    }
  })
  const active = step.status === 'running' || step.status === 'starting'
  const changeCount = view.events.filter((event) => (!event.stepId || event.stepId === step.id) && event.type === 'tool/edit').length
  const childAgents = new Set(
    view.events
      .filter((event) => event.type === 'child/started')
      .map((event) => String((event.data as { agentId?: string } | undefined)?.agentId ?? event.id)),
  ).size
  /**
   * Reasoning values are CLI tokens, so label them using the CLI's own names and
   * only fall back to a generic label for a value the CLI no longer reports.
   */
  const reasoningLabel = (value: string | undefined) => {
    if (!value) return t('agents.default')
    return options?.levels.find((level) => level.value === value)?.label ?? value
  }

  const applyOverride = (key: keyof Override, value: string) => {
    const merged: Override = { ...override }
    if (value) merged[key] = value
    else delete merged[key]
    setOverride(merged)
    localStorage.setItem(overrideKey(view.run.id), JSON.stringify(merged))
    // Never mutate a running worker; the override applies to the next run.
    if (active) setNotice(t('cliInfo.overrideHint'))
  }

  return (
    <aside className="cli-info">
      <header>
        <div className="cli-info-title">
          <span className="cli-profile">
            <ProviderIcon name={profile?.name ?? view.run.profileId} adapterId={runtime?.adapterId} />
            <strong>{profile?.name ?? view.run.profileId}</strong>
          </span>
          <em>{t(`run.status.${step.status}`)} · {elapsed(step.createdAt, worker?.endedAt)}</em>
        </div>
        <Button variant="ghost" size="icon-xs" aria-label={t('cliInfo.hide')} title={t('cliInfo.hide')} onClick={close}><X size={15} /></Button>
      </header>
      <dl>
        <div><dt>{t('cliInfo.runtime')}</dt><dd>{runtime?.adapterId ?? worker?.runtimeId ?? t('common.notAvailable')}</dd></div>
        <div>
          <dt>{t('agents.model')}</dt>
          <dd>
            {/* Session-level override; applies to the next run (docs/ui.md 16.2) */}
            {options?.models.length ? (
              <Select
                value={override.model ?? PROFILE_SELECTION}
                onValueChange={(next) => applyOverride('model', next === PROFILE_SELECTION ? '' : next)}
              >
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value={PROFILE_SELECTION}>{profile?.model ?? t('agents.notSelected')}</SelectItem>
                  {options.models.map((model) => (
                    <SelectItem value={model.value} key={model.value}>{model.label ?? model.value}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <span className="field-note">{t('agents.noModelList')}</span>
            )}
          </dd>
        </div>
        <div>
          <dt>{t('agents.reasoning')}</dt>
          <dd>
            {options?.levels.length ? (
              <Select
                value={override.reasoning ?? PROFILE_SELECTION}
                onValueChange={(next) => applyOverride('reasoning', next === PROFILE_SELECTION ? '' : next)}
              >
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value={PROFILE_SELECTION}>{reasoningLabel(profile?.reasoning)}</SelectItem>
                  {options.levels.map((level) => (
                    <SelectItem value={level.value} key={level.value}>{level.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <span className="field-note">{t('agents.noReasoningLevels')}</span>
            )}
          </dd>
        </div>
        <div><dt>{t('cliInfo.workingDirectory')}</dt><dd title={view.run.cwd}>{view.run.cwd}</dd></div>
        <div><dt>{t('cliInfo.started')}</dt><dd>{new Date(step.createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</dd></div>
        {childAgents > 0 && <div><dt>{t('cliInfo.agents')}</dt><dd>{childAgents}</dd></div>}
        {/* Changes live here, not in the Console header (docs/ui.md 18) */}
        <div>
          <dt>{t('cliInfo.changes')}</dt>
          <dd>
            {changeCount
              ? (
                <Button variant="link" size="xs" onClick={() => openInspector('changes')}>
                  {changeCount} {t('console.files')}
                </Button>
              )
              : t('console.noChanges')}
          </dd>
        </div>
      </dl>
    </aside>
  )
}

/* ------------------------------------------------------------------ */
/* Drill-down sheets (docs/ui.md 14, 18)                               */
/* ------------------------------------------------------------------ */

/**
 * Raw Output and Changes are drill-downs over the workspace, not permanent
 * regions — a Sheet is the framework's answer for exactly that (docs/ui.md 14).
 */
function InspectorSheet({ inspector, view, step, t, close }: {
  inspector: Exclude<Inspector, undefined>
  view: DesktopRunView
  step: Step
  t: Translator
  close(): void
}) {
  const events = view.events.filter((event) => !event.stepId || event.stepId === step.id)
  const changes = events.filter((event) => event.type === 'tool/edit')
  const raw = events.filter((event) => event.nativeEvent !== undefined)
  const title = inspector === 'changes' ? t('console.changes') : t('console.rawOutput')

  return (
    <Sheet open onOpenChange={(next) => { if (!next) close() }}>
      <SheetContent side="right" className="inspector-sheet w-full max-w-[520px] gap-0">
        <SheetHeader>
          <SheetTitle>{title}</SheetTitle>
          <SheetDescription>{view.run.profileId}</SheetDescription>
        </SheetHeader>
        <ScrollArea className="min-h-0 flex-1">
          {inspector === 'changes'
            ? (
              <div className="change-list">
                {!changes.length && <p>{t('console.noChanges')}</p>}
                {changes.map((event) => (
                  <article key={event.id}>
                    <strong className="change-path">{eventSummary(event)}</strong>
                    <pre>{JSON.stringify(event.data, null, 2)}</pre>
                  </article>
                ))}
              </div>
            )
            : (
              <div className="raw-output">
                {!raw.length && <p>{t('console.noRawOutput')}</p>}
                {raw.map((event) => (
                  <article key={event.id}>
                    <time>{event.timestamp}</time>
                    <strong>{event.type}</strong>
                    <pre>{JSON.stringify(event.nativeEvent, null, 2)}</pre>
                  </article>
                ))}
              </div>
            )}
        </ScrollArea>
      </SheetContent>
    </Sheet>
  )
}

/* ------------------------------------------------------------------ */
/* Workspace, empty state, app shell                                   */
/* ------------------------------------------------------------------ */

/** Minimal empty state content (docs/ui.md 22.3). */
function EmptyStateBody({ t, hint }: { t: Translator; hint?: boolean }) {
  return (
    <>
      <span className="empty-mark mark-tinted" role="img" aria-hidden="true" />
      <h1>{hint ? t('runs.empty') : t('empty.noSessions')}</h1>
      <p>{t('empty.useRelay')}</p>
    </>
  )
}

function SessionWorkspace({
  t,
  macOS,
  sidebarCollapsed,
}: {
  t: Translator
  macOS: boolean
  sidebarCollapsed: boolean
}) {
  const { snapshot, selectedSessionId, selectedRunId, selectedStepId, loading, refresh, onboardingOpen } = useAppStore()
  const [inspector, setInspector] = useState<Inspector>()
  // Contextual inspector: closed until the user asks for it (docs/ui.md 15).
  const [cliInfoOpen, setCliInfoOpen] = useState(() => localStorage.getItem('relay.cli-info-open') === 'true')

  if (onboardingOpen) return <Onboarding t={t} />

  const session = snapshot?.sessions.find((item) => item.id === selectedSessionId)
  if (!session) {
    return (
      <main className="workspace empty-workspace">
        <EmptyStateBody t={t} />
      </main>
    )
  }

  const views = snapshot?.runs.filter((item) => item.run.hostSessionId === selectedSessionId) ?? []
  const stepItems: StepItem[] = views.flatMap((view) => view.steps.map((step) => ({ view, step })))
  const selectedView = views.find((view) => view.run.id === selectedRunId) ?? views[0]
  const selectedStep = selectedView?.steps.find((step) => step.id === selectedStepId) ?? selectedView?.steps[0]
  const toggleCliInfo = () =>
    setCliInfoOpen((current) => {
      const next = !current
      localStorage.setItem('relay.cli-info-open', String(next))
      return next
    })

  return (
    <main className="workspace">
      {/* The title names the Codex session and never changes with Step selection;
          worker/task detail belongs to Step and Console (docs/ui.md 6). */}
      <header className="workspace-heading">
        <div className="flex min-w-0 items-start gap-2">
          <SidebarTrigger
            className={cn('mt-0.5 shrink-0', macOS && sidebarCollapsed && 'ml-14')}
            aria-label={sidebarCollapsed ? t('action.showSidebar') : t('action.hideSidebar')}
          />
          <div className="min-w-0">
          <h1 title={session.displayName}>{session.displayName}</h1>
          <p>
            {selectedView && <StatusLabel status={selectedView.run.status} t={t} />}
            {selectedView && <span className="heading-sep">·</span>}
            <span>{new Date(session.startedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
          </p>
          </div>
        </div>
        <div className="heading-actions">
          <EnvironmentChip t={t} />
          <Button variant="ghost" size="icon-xs" title={t('common.refresh')} onClick={() => void refresh()}>
            <RefreshCw className={loading ? 'spin' : ''} size={15} />
          </Button>
          <SessionMenu session={session} view={selectedView} t={t} />
        </div>
      </header>

      {stepItems.length ? (
        <>
          <StepNavigator items={stepItems} t={t} />
          {selectedView && selectedStep && (
            <div className={cliInfoOpen ? 'workspace-lower cli-info-open' : 'workspace-lower'}>
              <Console
                view={selectedView}
                step={selectedStep}
                t={t}
                openInspector={setInspector}
                cliInfoOpen={cliInfoOpen}
                toggleCliInfo={toggleCliInfo}
              />
              {cliInfoOpen && (
                <CliInfo
                  view={selectedView}
                  step={selectedStep}
                  t={t}
                  close={toggleCliInfo}
                  openInspector={setInspector}
                />
              )}
            </div>
          )}
        </>
      ) : (
        <div className="workspace-placeholder">
          <EmptyStateBody t={t} hint />
        </div>
      )}

      {inspector && selectedView && selectedStep && (
        <InspectorSheet
          inspector={inspector}
          view={selectedView}
          step={selectedStep}
          t={t}
          close={() => setInspector(undefined)}
        />
      )}
    </main>
  )
}

export function App() {
  const { locale, refresh, error, notice, setNotice } = useAppStore()
  const [theme, setThemeState] = useState<Theme>(() => (localStorage.getItem('relay.theme') as Theme | null) ?? 'system')
  const [fontSize, setFontSizeState] = useState(storedFontSize)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem('relay.sidebar-collapsed') === 'true')
  const t = createTranslator(locale)

  const setTheme = (next: Theme) => { localStorage.setItem('relay.theme', next); setThemeState(next) }
  const setSidebarOpen = (open: boolean) => {
    localStorage.setItem('relay.sidebar-collapsed', String(!open))
    setSidebarCollapsed(!open)
  }
  const setFontSize = (next: number) => {
    const value = Math.min(20, Math.max(14, Math.round(next)))
    localStorage.setItem('relay.font-size', String(value))
    setFontSizeState(value)
  }

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    if (theme === 'system') document.documentElement.removeAttribute('data-theme')
  }, [theme])
  useEffect(() => {
    // One root setting scales shadcn controls and Relay's rem-based typography
    // together. Do not add a second custom multiplier here.
    document.documentElement.style.fontSize = `${fontSize}px`
  }, [fontSize])
  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2_000)
    return () => window.clearInterval(timer)
  }, [refresh])
  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(undefined), 3_500)
    return () => window.clearTimeout(timer)
  }, [notice, setNotice])

  const macOS = navigator.userAgent.includes('Macintosh')

  return (
    <SidebarProvider open={!sidebarCollapsed} onOpenChange={setSidebarOpen}>
      <RelaySidebar t={t} macOS={macOS} />
      <SidebarInset className="min-h-svh">
        <SessionWorkspace t={t} macOS={macOS} sidebarCollapsed={sidebarCollapsed} />
      </SidebarInset>
      <SettingsSheet t={t} theme={theme} setTheme={setTheme} fontSize={fontSize} setFontSize={setFontSize} />
      {error && <div className="toast error"><CircleAlert size={14} />{error}</div>}
      {notice && <div className="toast"><Check size={14} />{notice}</div>}
    </SidebarProvider>
  )
}
