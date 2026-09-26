import type { AgentProfile } from '@relay/protocol'
import type { RunView } from '@relay/relay-api'
import { cn } from '../lib/utils.js'
import type { Translator } from '../lib/format.js'
import { elapsed, relativeTime, statusGlyph } from '../lib/format.js'
import { useAppStore } from '../store.js'

function profileName(profiles: AgentProfile[], id: string): string {
  return profiles.find((profile) => profile.id === id)?.name ?? id
}

const STATUS_TONE: Record<string, string> = {
  running: 'text-[var(--green)]',
  starting: 'text-[var(--green)]',
  awaiting_host: 'text-[var(--amber)]',
  failed: 'text-destructive',
  orphaned: 'text-destructive',
  interrupted: 'text-destructive',
}

/** Every run in the selected session; the run owns the steps the console shows. */
export function RunStrip({ t, onSelect }: { t: Translator; onSelect(runId: string): void }) {
  const snapshot = useAppStore((state) => state.snapshot)
  const selectedSessionId = useAppStore((state) => state.selectedSessionId)
  const selectedRunId = useAppStore((state) => state.selectedRunId)
  const session = snapshot?.sessions.find((candidate) => candidate.session.id === selectedSessionId)
  const profiles = snapshot?.profiles ?? []
  const runs: RunView[] = session?.runs ?? []

  if (!session) return null

  return (
    <div className="flex min-h-0 flex-col border-b border-border">
      <div className="flex items-center gap-2 px-4 py-2">
        <span className="truncate text-[13px] font-semibold">{session.session.displayName}</span>
        <span className="truncate text-[11px] text-muted-foreground">{session.session.cwd}</span>
        <span className="ml-auto text-[11px] text-muted-foreground">
          {t('cliInfo.started')} {new Date(session.session.startedAt).toLocaleString()}
        </span>
      </div>
      {runs.length === 0 ? (
        <p className="px-4 pb-3 text-xs text-muted-foreground">{t('inspector.noRun')}</p>
      ) : (
        <div className="flex gap-2 overflow-x-auto px-4 pb-3">
          {runs.map((view) => {
            const active = view.run.id === selectedRunId
            const worker = view.workers.at(-1)
            return (
              <button
                key={view.run.id}
                type="button"
                onClick={() => onSelect(view.run.id)}
                className={cn(
                  'flex min-w-[220px] max-w-[320px] shrink-0 flex-col gap-1 rounded-lg border px-3 py-2 text-left transition-colors',
                  active ? 'border-ring bg-accent' : 'border-border bg-card hover:border-[var(--border-strong)]',
                )}
              >
                <span className="flex items-center gap-2 text-[12px] font-medium">
                  <i className={cn('not-italic', STATUS_TONE[view.run.status] ?? 'text-muted-foreground')}>
                    {statusGlyph(view.run.status)}
                  </i>
                  {profileName(profiles, view.run.profileId)}
                  <span className="ml-auto text-[10px] font-normal uppercase tracking-wide text-muted-foreground">
                    {t(`run.status.${view.run.status}`)}
                  </span>
                </span>
                <span className="line-clamp-2 text-[11px] leading-snug text-muted-foreground">{view.run.task}</span>
                <span className="flex items-center gap-2 text-[10px] text-muted-foreground">
                  <span>{view.steps.length} {t('inspector.steps')}</span>
                  <span aria-hidden="true">·</span>
                  <span>{elapsed(view.run.createdAt, worker?.endedAt ?? undefined)}</span>
                  <span aria-hidden="true">·</span>
                  <span>{relativeTime(view.run.updatedAt, view.run.status, t)}</span>
                </span>
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
