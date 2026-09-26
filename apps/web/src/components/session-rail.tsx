import type { HostSession } from '@relay/protocol'
import { cn } from '../lib/utils.js'
import type { Translator } from '../lib/format.js'
import { relativeTime, statusGlyph } from '../lib/format.js'
import { ScrollArea } from './ui/scroll-area.js'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from './ui/empty.js'
import { useAppStore } from '../store.js'

/**
 * Sessions are Codex threads, not directories (see docs/architecture.md): the name
 * comes from Codex and Relay never invents one.
 */
export function SessionRail({ t, onSelect }: { t: Translator; onSelect(sessionId: string): void }) {
  const snapshot = useAppStore((state) => state.snapshot)
  const selectedSessionId = useAppStore((state) => state.selectedSessionId)
  const sessions = snapshot?.sessions ?? []

  return (
    <aside className="flex min-h-0 w-full flex-col border-r border-border bg-[var(--sidebar)]">
      <div className="flex items-center justify-between px-4 py-3">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t('nav.sessions')}
        </span>
        <span className="text-xs tabular-nums text-muted-foreground">{sessions.length}</span>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        {sessions.length === 0 ? (
          <div className="px-4 pb-4">
            <Empty className="border-0 p-0">
              <EmptyHeader>
                <EmptyTitle>{t('sessions.empty')}</EmptyTitle>
                <EmptyDescription>{t('sessions.emptyHint')}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          </div>
        ) : (
          <ul className="flex flex-col gap-0.5 px-2 pb-3">
            {sessions.map(({ session, runs, activeWorkers, awaitingHost }) => (
              <li key={session.id}>
                <button
                  type="button"
                  onClick={() => onSelect(session.id)}
                  className={cn(
                    'flex w-full flex-col gap-1 rounded-md px-2.5 py-2 text-left transition-colors',
                    session.id === selectedSessionId
                      ? 'bg-accent text-accent-foreground'
                      : 'hover:bg-accent/60',
                  )}
                >
                  <span className="line-clamp-2 text-[13px] font-medium leading-snug">{session.displayName}</span>
                  <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                    <i
                      className={cn(
                        'size-1.5 rounded-full',
                        activeWorkers > 0 ? 'bg-[var(--green)]' : awaitingHost > 0 ? 'bg-[var(--amber)]' : 'bg-[var(--border-strong)]',
                      )}
                      aria-hidden="true"
                    />
                    {relativeTime(session.updatedAt, session.status, t)}
                    <span aria-hidden="true">·</span>
                    {runs.length} {t('runs.title')}
                    {activeWorkers > 0 && <span className="text-[var(--green)]">● {activeWorkers}</span>}
                    {awaitingHost > 0 && <span className="text-[var(--amber)]">◆ {awaitingHost}</span>}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </ScrollArea>
    </aside>
  )
}
