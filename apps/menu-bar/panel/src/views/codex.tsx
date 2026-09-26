import type { CodexStatus } from '@relay/relay-api'
import { Download, RefreshCw, Trash2, Wrench } from 'lucide-react'
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

/**
 * The Codex integration as a lifecycle, not a single button: install, repair,
 * update and remove, each reported step by step, with the reason a check is not
 * ok visible next to it.
 */
export function CodexView({
  t,
  status,
  onAction,
  busy,
}: {
  t: Translator
  status: CodexStatus
  onAction(action: 'install' | 'repair' | 'update' | 'remove'): void
  busy: boolean
}) {
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardContent className="flex flex-col gap-2 pt-4">
          {status.checks.map((check) => (
            <div key={check.id} className="flex flex-col gap-0.5 border-b border-border/60 pb-2 last:border-0 last:pb-0">
              <div className="flex items-center gap-2">
                <i className={cn('not-italic text-xs', check.ok ? 'text-[var(--green)]' : 'text-destructive')}>
                  {check.ok ? '✓' : '✗'}
                </i>
                <span className="text-[13px]">{t(LABELS[check.id] as never)}</span>
                {!check.ok && (
                  <Badge variant="outline" className="text-[10px] uppercase">
                    {check.status}
                  </Badge>
                )}
              </div>
              <span className="truncate pl-4 text-[11px] text-muted-foreground" title={check.detail}>
                {check.detail}
              </span>
              {check.hint && <span className="pl-4 text-[11px] text-[var(--amber)]">{check.hint}</span>}
            </div>
          ))}
        </CardContent>
      </Card>

      <div className="flex flex-wrap gap-2">
        <Button size="sm" disabled={busy} onClick={() => onAction('install')}>
          <Download /> {t('panel.install')}
        </Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => onAction('repair')}>
          <Wrench /> {t('panel.repair')}
        </Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => onAction('update')}>
          <RefreshCw /> {t('panel.update')}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="text-destructive"
          disabled={busy}
          onClick={() => {
            if (window.confirm(t('panel.removeConfirm'))) onAction('remove')
          }}
        >
          <Trash2 /> {t('panel.remove')}
        </Button>
      </div>
    </div>
  )
}
