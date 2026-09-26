import type { InspectorSnapshot } from '@relay/relay-api'
import { ExternalLink, RefreshCw } from 'lucide-react'
import { Badge } from '../components/ui/badge.js'
import { Button } from '../components/ui/button.js'
import { Card, CardContent } from '../components/ui/card.js'
import type { Translator } from '../lib/i18n.js'

const HEALTH_TONE: Record<string, string> = {
  available: 'text-[var(--green)]',
  authentication_required: 'text-[var(--amber)]',
  unavailable: 'text-destructive',
}

/** Which CLIs Relay found on this machine, and what it thinks of each. */
export function RuntimeView({
  t,
  snapshot,
  onRescan,
  busy,
  onOpenInspector,
}: {
  t: Translator
  snapshot: InspectorSnapshot
  onRescan(): void
  busy: boolean
  onOpenInspector(): void
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t('panel.runtime')}
        </span>
        <span className="text-xs tabular-nums text-muted-foreground">{snapshot.runtimes.length}</span>
        <Button size="xs" variant="outline" className="ml-auto" disabled={busy} onClick={onRescan}>
          <RefreshCw className={busy ? 'spin' : undefined} /> {busy ? t('panel.rescanning') : t('panel.rescan')}
        </Button>
      </div>

      {snapshot.runtimes.length === 0 && <p className="text-xs text-muted-foreground">{t('panel.noRuntimes')}</p>}

      {snapshot.runtimes.map((item) => (
        <Card key={item.id}>
          <CardContent className="flex flex-col gap-1 pt-4 text-[12px]">
            <span className="flex items-center gap-2 font-medium">
              {item.adapterId}
              <Badge variant="secondary" className={`text-[10px] ${HEALTH_TONE[item.health] ?? ''}`}>
                {item.health}
              </Badge>
              {item.version && <span className="text-[11px] text-muted-foreground">{item.version}</span>}
            </span>
            <span className="truncate text-[11px] text-muted-foreground" title={item.executablePath}>
              {t('panel.runtime.path')}: {item.executablePath}
            </span>
            <span className="text-[11px] text-muted-foreground">
              {Object.entries(item.capabilities)
                .filter(([, value]) => value === true)
                .map(([key]) => key)
                .join(' · ')}
            </span>
          </CardContent>
        </Card>
      ))}

      {snapshot.diagnostics.length > 0 && (
        <Card>
          <CardContent className="flex flex-col gap-1 pt-4">
            {snapshot.diagnostics.map((line) => (
              <span key={line} className="text-[11px] text-muted-foreground">
                {line}
              </span>
            ))}
          </CardContent>
        </Card>
      )}

      <Button size="sm" variant="outline" onClick={onOpenInspector}>
        <ExternalLink /> {t('panel.openInspector')}
      </Button>
    </div>
  )
}
