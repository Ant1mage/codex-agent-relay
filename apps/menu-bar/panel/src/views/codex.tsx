import { useState } from 'react'
import type { CodexStatus } from '@relay/relay-api'
import { Download, RefreshCw, Trash2, Wrench } from 'lucide-react'
import { ConfirmDialog } from '../components/confirm-dialog.js'
import { Badge } from '../components/ui/badge.js'
import { Button } from '../components/ui/button.js'
import { Card, CardContent } from '../components/ui/card.js'
import { cn } from '../lib/utils.js'
import type { Translator } from '../lib/i18n.js'

const LABELS: Record<string, string> = {
  'codex-cli': 'onboarding.check.codex-cli',
  'relay-mcp': 'onboarding.check.relay-mcp',
  'relay-skill': 'onboarding.check.relay-skill',
  'relay-plugin': 'onboarding.check.relay-plugin',
  'relay-hooks': 'onboarding.check.relay-hooks',
}

const TONE: Record<string, string> = {
  ok: 'text-success',
  outdated: 'text-warning',
  legacy: 'text-warning',
  stale: 'text-destructive',
  missing: 'text-muted-foreground',
}

/**
 * The Codex integration as a lifecycle: install, repair, update and remove,
 * with a reason next to every check. Long paths wrap instead of setting the
 * width — an unbounded detail string used to stretch this tab to 816px.
 */
export function CodexView({
  t,
  status,
  onAction,
  busy,
  highlightActions,
}: {
  t: Translator
  status: CodexStatus
  onAction(action: 'install' | 'repair' | 'update' | 'remove'): void
  busy: boolean
  highlightActions: boolean
}) {
  const installed = status.checks.filter((check) => check.ok).length
  const [removeOpen, setRemoveOpen] = useState(false)

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <Card className="min-w-0">
        <CardContent className="flex min-w-0 flex-col gap-0 p-3">
          {status.checks.map((check) => (
            <div key={check.id} className="min-w-0 border-b border-border/60 py-2 first:pt-0 last:border-0 last:pb-0">
              <div className="flex min-w-0 items-center gap-1.5">
                <i className={cn('shrink-0 not-italic text-xs', check.ok ? TONE.ok : TONE[check.status])}>
                  {check.ok ? '✓' : '✗'}
                </i>
                <span className="min-w-0 flex-1 truncate text-[12px]">{t(LABELS[check.id] as never)}</span>
                <Badge variant="outline" className="shrink-0 text-[9px] uppercase">
                  {check.status}
                </Badge>
              </div>
              <p className="mt-0.5 break-all text-[10px] leading-snug text-muted-foreground">{check.detail}</p>
              {check.hint && (
                <p className="mt-0.5 break-words text-[10px] leading-snug text-warning">{check.hint}</p>
              )}
            </div>
          ))}
        </CardContent>
      </Card>

      <div className="min-w-0">
        <div className="mb-1.5 flex items-center gap-2">
          <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {t('panel.actions')}
          </span>
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {installed}/{status.checks.length}
          </span>
        </div>
        <div className={cn('flex min-w-0 flex-col gap-1.5', highlightActions && 'rounded-md ring-1 ring-ring/60 ring-offset-2 ring-offset-background p-1.5')}>
          <Button size="sm" className="w-full" disabled={busy} onClick={() => onAction('install')}>
            <Download /> {t('panel.install')}
          </Button>
          <div className="flex min-w-0 gap-1.5">
            <Button size="sm" variant="outline" className="min-w-0 flex-1" disabled={busy} onClick={() => onAction('repair')}>
              <Wrench /> {t('panel.repair')}
            </Button>
            <Button size="sm" variant="outline" className="min-w-0 flex-1" disabled={busy} onClick={() => onAction('update')}>
              <RefreshCw /> {t('panel.update')}
            </Button>
          </div>
          <Button
            size="sm"
            variant="ghost"
            className="w-full justify-start text-destructive hover:text-destructive"
            disabled={busy}
            onClick={() => setRemoveOpen(true)}
          >
            <Trash2 /> {t('panel.remove')}
          </Button>
        </div>
      </div>
      <ConfirmDialog
        open={removeOpen}
        onOpenChange={setRemoveOpen}
        title={t('panel.codex.removeTitle')}
        description={t('panel.removeConfirm')}
        cancelLabel={t('action.cancel')}
        confirmLabel={t('panel.remove')}
        onConfirm={() => onAction('remove')}
      />
    </div>
  )
}
