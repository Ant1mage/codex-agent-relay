import { CheckCircle2, Copy, Languages, RefreshCw } from 'lucide-react'
import { Badge } from './ui/badge.js'
import { Button } from './ui/button.js'
import { cn } from '../lib/utils.js'
import type { Translator } from '../lib/format.js'
import { useAppStore } from '../store.js'

function ConnectionBadge({ t }: { t: Translator }) {
  const connection = useAppStore((state) => state.connection)
  const tone =
    connection === 'live'
      ? 'bg-[var(--green)]'
      : connection === 'connecting'
        ? 'bg-[var(--amber)]'
        : 'bg-destructive'
  const label =
    connection === 'live'
      ? t('inspector.live')
      : connection === 'connecting'
        ? t('inspector.connecting')
        : t('inspector.offline')
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <i className={cn('size-1.5 rounded-full', tone)} aria-hidden="true" />
      {label}
    </span>
  )
}

/** Window chrome: identity, liveness, machine facts and the page-level actions. */
export function Header({
  t,
  onRefresh,
  onCopyDiagnostics,
  refreshing,
}: {
  t: Translator
  onRefresh(): void
  onCopyDiagnostics(): void
  refreshing: boolean
}) {
  const snapshot = useAppStore((state) => state.snapshot)
  const locale = useAppStore((state) => state.locale)
  const setLocale = useAppStore((state) => state.setLocale)
  const runs = snapshot?.sessions.reduce((total, session) => total + session.runs.length, 0) ?? 0
  const active =
    snapshot?.sessions.reduce((total, session) => total + session.activeWorkers, 0) ?? 0
  const codexReady = snapshot?.codex.configured ?? false

  return (
    <header className="flex flex-wrap items-center gap-3 border-b border-border bg-card px-4 py-2.5">
      <span className="mark-tinted size-4 shrink-0 text-foreground" role="img" aria-label="Relay" />
      <h1 className="text-sm font-semibold tracking-tight">{t('inspector.title')}</h1>
      <ConnectionBadge t={t} />
      <div className="hidden items-center gap-2 text-xs text-muted-foreground sm:flex">
        <span>{snapshot?.sessions.length ?? 0} {t('nav.sessions')}</span>
        <span aria-hidden="true">·</span>
        <span>{runs} {t('runs.title')}</span>
        <span aria-hidden="true">·</span>
        <span className="tabular-nums">{active} {t('common.active')}</span>
      </div>

      <div className="ml-auto flex items-center gap-1.5">
        <Badge variant={codexReady ? 'secondary' : 'destructive'} className="hidden md:inline-flex">
          {codexReady ? (
            <>
              <CheckCircle2 /> {t('onboarding.check.codex-cli')}
            </>
          ) : (
            t('inspector.codexMissing')
          )}
        </Badge>
        <Button
          variant="ghost"
          size="icon-sm"
          title={locale === 'en' ? t('language.zh-CN') : t('language.en')}
          onClick={() => setLocale(locale === 'en' ? 'zh-CN' : 'en')}
        >
          <Languages />
        </Button>
        <Button variant="ghost" size="icon-sm" title={t('inspector.copyDiagnostics')} onClick={onCopyDiagnostics}>
          <Copy />
        </Button>
        <Button variant="ghost" size="icon-sm" title={t('common.refresh')} onClick={onRefresh}>
          <RefreshCw className={refreshing ? 'spin' : undefined} />
        </Button>
      </div>
    </header>
  )
}
