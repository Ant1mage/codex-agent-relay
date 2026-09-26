import { useEffect, useState } from 'react'
import type { InspectorSnapshot, ManualRuntimeView } from '@relay/relay-api'
import { ExternalLink, Plus, RefreshCw, Trash2, X, Zap } from 'lucide-react'
import { Badge } from '../components/ui/badge.js'
import { Button } from '../components/ui/button.js'
import { Card, CardContent } from '../components/ui/card.js'
import { Input } from '../components/ui/input.js'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select.js'
import { Field, FieldDescription, FieldLabel } from '../components/ui/field.js'
import { cn } from '../lib/utils.js'
import type { Translator } from '../lib/i18n.js'

const HEALTH_TONE: Record<string, string> = {
  available: 'text-[var(--green)]',
  authentication_required: 'text-[var(--amber)]',
  unavailable: 'text-destructive',
}

/**
 * Runtimes on this machine. Detection is read-only; the add form registers a
 * CLI by hand for installs the scanner cannot find, and the daemon probes it
 * before anything is saved.
 */
export function RuntimeView({
  t,
  snapshot,
  manualRuntimes,
  onRescan,
  onSave,
  onDelete,
  onProbe,
  startNew,
  onIntentHandled,
  busy,
  onOpenInspector,
}: {
  t: Translator
  snapshot: InspectorSnapshot
  manualRuntimes: ManualRuntimeView[]
  onRescan(): void
  onSave(entry: { id: string; adapterId: string; executablePath: string; label?: string }): Promise<void>
  onDelete(runtimeId: string): void
  onProbe(input: {
    adapterId: string
    executablePath: string
  }): Promise<{ ok: boolean; version?: string | undefined; error?: string | undefined }>
  startNew: boolean
  onIntentHandled(): void
  busy: boolean
  onOpenInspector(): void
}) {
  const [adding, setAdding] = useState(false)
  const [adapters, setAdapters] = useState<string[]>([])
  const [adapterId, setAdapterId] = useState('')
  const [executablePath, setExecutablePath] = useState('')
  const [probe, setProbe] = useState<{ ok: boolean; version?: string | undefined; error?: string | undefined }>()
  const [checking, setChecking] = useState(false)

  useEffect(() => {
    if (!startNew) return
    setAdding(true)
    onIntentHandled()
  }, [startNew])

  useEffect(() => {
    if (!adding || adapters.length > 0) return
    // The catalogue comes from the daemon, so a new adapter shows up by itself.
    void fetch('/api/adapters')
      .then((response) => response.json())
      .then((value: { adapters?: string[] }) => {
        const list = value.adapters ?? []
        setAdapters(list)
        setAdapterId((current) => current || list[0] || '')
      })
      .catch(() => setAdapters([]))
  }, [adding, adapters.length])

  const check = async () => {
    setChecking(true)
    try {
      setProbe(await onProbe({ adapterId, executablePath }))
    } finally {
      setChecking(false)
    }
  }

  const detected = snapshot.runtimes.filter((item) => !manualRuntimes.some((manual) => manual.id === item.id))

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex min-w-0 items-center gap-2">
        <span className="ml-auto flex shrink-0 items-center gap-1.5">
          <Button size="xs" variant="outline" disabled={busy} onClick={onRescan}>
            <RefreshCw className={busy ? 'spin' : undefined} /> {busy ? t('panel.rescanning') : t('panel.rescan')}
          </Button>
          <Button size="xs" onClick={() => setAdding(true)}>
            <Plus /> {t('panel.addRuntime')}
          </Button>
        </span>
      </div>

      {adding && (
        <Card className="min-w-0">
          <CardContent className="flex min-w-0 flex-col gap-3 p-3">
            <div className="flex items-center justify-between">
              <span className="min-w-0 truncate text-xs font-semibold">{t('panel.addRuntime')}</span>
              <Button
                variant="ghost"
                size="icon-xs"
                onClick={() => {
                  setAdding(false)
                  setProbe(undefined)
                }}
              >
                <X />
              </Button>
            </div>
            <Field className="min-w-0">
              <FieldLabel>{t('cliInfo.runtime')}</FieldLabel>
              <Select value={adapterId} onValueChange={setAdapterId}>
                <SelectTrigger className="w-full min-w-0">
                  <SelectValue placeholder={t('panel.noRuntimes')} />
                </SelectTrigger>
                <SelectContent>
                  {adapters.map((id) => (
                    <SelectItem key={id} value={id}>
                      {id}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field className="min-w-0">
              <FieldLabel htmlFor="runtime-path">{t('panel.runtime.path')}</FieldLabel>
              <Input
                id="runtime-path"
                className="min-w-0 font-mono text-[11px]"
                placeholder="/usr/local/bin/dsh"
                value={executablePath}
                onChange={(event) => {
                  setExecutablePath(event.target.value)
                  setProbe(undefined)
                }}
              />
              <FieldDescription className="break-words">{t('panel.runtime.pathHint')}</FieldDescription>
            </Field>
            {probe && (
              <p className={cn('break-words text-[11px]', probe.ok ? 'text-[var(--green)]' : 'text-destructive')}>
                {probe.ok ? `${t('panel.runtime.probeOk')}${probe.version ? ` · ${probe.version}` : ''}` : probe.error}
              </p>
            )}
            <div className="flex items-center gap-2">
              <Button size="sm" variant="outline" disabled={!executablePath || checking} onClick={() => void check()}>
                <Zap /> {checking ? t('panel.runtime.checking') : t('panel.runtime.check')}
              </Button>
              <Button
                size="sm"
                className="flex-1"
                disabled={!adapterId || !executablePath}
                onClick={() => {
                  void onSave({
                    id: `manual-${adapterId.replace(/[^a-z0-9-]/gi, '')}-${Date.now().toString(36)}`,
                    adapterId,
                    executablePath,
                  }).then(() => {
                    setAdding(false)
                    setProbe(undefined)
                    setExecutablePath('')
                  })
                }}
              >
                {t('action.save')}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {snapshot.runtimes.length === 0 && <p className="text-xs text-muted-foreground">{t('panel.noRuntimes')}</p>}

      {detected.map((item) => (
        <Card key={item.id} className="min-w-0">
          <CardContent className="flex min-w-0 flex-col gap-1 p-3 text-[12px]">
            <span className="flex min-w-0 items-center gap-2 font-medium">
              <span className="min-w-0 truncate">{item.adapterId}</span>
              <Badge variant="secondary" className={`shrink-0 text-[10px] ${HEALTH_TONE[item.health] ?? ''}`}>
                {item.health}
              </Badge>
              {item.version && <span className="shrink-0 text-[11px] text-muted-foreground">{item.version}</span>}
            </span>
            <span className="break-all text-[11px] leading-snug text-muted-foreground">{item.executablePath}</span>
            <span className="truncate text-[11px] text-muted-foreground">
              {Object.entries(item.capabilities)
                .filter(([, value]) => value === true)
                .map(([key]) => key)
                .join(' · ')}
            </span>
          </CardContent>
        </Card>
      ))}

      {manualRuntimes.length > 0 && (
        <>
          <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {t('panel.runtime.manual')}
          </span>
          {manualRuntimes.map((entry) => (
            <Card key={entry.id} className="min-w-0">
              <CardContent className="flex min-w-0 items-center gap-2 p-3">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12px] font-medium">{entry.adapterId}</span>
                  <span className="block break-all text-[11px] text-muted-foreground">{entry.executablePath}</span>
                </span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  className="shrink-0 text-destructive"
                  title={t('panel.delete')}
                  onClick={() => onDelete(entry.id)}
                >
                  <Trash2 />
                </Button>
              </CardContent>
            </Card>
          ))}
        </>
      )}

      {snapshot.diagnostics.length > 0 && (
        <Card className="min-w-0">
          <CardContent className="flex min-w-0 flex-col gap-1 p-3">
            {snapshot.diagnostics.map((line) => (
              <span key={line} className="break-words text-[11px] text-muted-foreground">
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
