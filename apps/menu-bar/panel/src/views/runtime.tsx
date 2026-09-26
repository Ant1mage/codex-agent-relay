import { useEffect, useMemo, useState } from 'react'
import type { Runtime } from '@relay/protocol'
import type { InspectorSnapshot, ManualRuntimeView } from '@relay/relay-api'
import { ArrowLeft, Pencil, Plus, RefreshCw, Trash2, Zap } from 'lucide-react'
import { ConfirmDialog } from '../components/confirm-dialog.js'
import { Badge } from '../components/ui/badge.js'
import { Button } from '../components/ui/button.js'
import { Empty, EmptyDescription, EmptyHeader } from '../components/ui/empty.js'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '../components/ui/field.js'
import { Input } from '../components/ui/input.js'
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemTitle,
} from '../components/ui/item.js'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select.js'
import { Separator } from '../components/ui/separator.js'
import { Spinner } from '../components/ui/spinner.js'
import type { Translator } from '../lib/i18n.js'
import { cn } from '../lib/utils.js'

interface RuntimeDraft {
  id: string
  adapterId: string
  executablePath: string
  label: string
  isNew: boolean
}

interface RuntimeRow {
  runtime?: Runtime
  manual?: ManualRuntimeView
}

function healthVariant(health: Runtime['health'] | 'unavailable') {
  if (health === 'available') return 'success' as const
  if (health === 'authentication_required') return 'warning' as const
  return 'destructive' as const
}

/**
 * Runtimes use the same list/editor navigation as Agent Profiles. Automatic
 * and manual entries stay in one list; their origin is metadata, not a second
 * visual hierarchy.
 */
export function RuntimeView({
  t,
  snapshot,
  manualRuntimes,
  onRescan,
  onSave,
  onDelete,
  onProbe,
  onLoadAdapters,
  startNew,
  onIntentHandled,
  busy,
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
  onLoadAdapters(): Promise<string[]>
  startNew: boolean
  onIntentHandled(): void
  busy: boolean
}) {
  const [draft, setDraft] = useState<RuntimeDraft>()
  const [deleteEntry, setDeleteEntry] = useState<ManualRuntimeView>()
  const [adapters, setAdapters] = useState<string[]>([])
  const [probe, setProbe] = useState<{ ok: boolean; version?: string | undefined; error?: string | undefined }>()
  const [checking, setChecking] = useState(false)

  const beginNew = () => {
    setProbe(undefined)
    setDraft({
      id: `manual-runtime-${Date.now().toString(36)}`,
      adapterId: adapters[0] ?? '',
      executablePath: '',
      label: '',
      isNew: true,
    })
  }

  useEffect(() => {
    if (!startNew) return
    beginNew()
    onIntentHandled()
  }, [startNew])

  useEffect(() => {
    if (!draft || adapters.length > 0) return
    void onLoadAdapters()
      .then((list) => {
        setAdapters(list)
        setDraft((current) => (current ? { ...current, adapterId: current.adapterId || list[0] || '' } : current))
      })
      .catch(() => setAdapters([]))
  }, [draft, adapters.length, onLoadAdapters])

  const rows = useMemo<RuntimeRow[]>(() => {
    const manualById = new Map(manualRuntimes.map((entry) => [entry.id, entry]))
    const merged: RuntimeRow[] = snapshot.runtimes.map((runtime) => {
      const manual = manualById.get(runtime.id)
      return manual ? { runtime, manual } : { runtime }
    })
    for (const manual of manualRuntimes) {
      if (!snapshot.runtimes.some((runtime) => runtime.id === manual.id)) merged.push({ manual })
    }
    return merged
  }, [manualRuntimes, snapshot.runtimes])

  const check = async () => {
    if (!draft) return
    setChecking(true)
    try {
      setProbe(await onProbe({ adapterId: draft.adapterId, executablePath: draft.executablePath }))
    } finally {
      setChecking(false)
    }
  }

  if (draft) {
    return (
      <div className="flex min-w-0 flex-col gap-3">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon-xs" onClick={() => setDraft(undefined)} title={t('onboarding.back')}>
            <ArrowLeft />
          </Button>
          <span className="min-w-0 flex-1 truncate text-xs font-semibold">
            {draft.isNew ? t('panel.addRuntime') : t('panel.runtime.edit')}
          </span>
        </div>

        <FieldGroup className="min-w-0 gap-3">
          <Field className="min-w-0">
            <FieldLabel htmlFor="runtime-name">{t('panel.runtime.name')}</FieldLabel>
            <Input
              id="runtime-name"
              value={draft.label}
              placeholder={draft.adapterId || t('cliInfo.runtime')}
              onChange={(event) => setDraft({ ...draft, label: event.target.value })}
            />
            <FieldDescription>{t('panel.runtime.nameHint')}</FieldDescription>
          </Field>
          <Field className="min-w-0">
            <FieldLabel>{t('cliInfo.runtime')}</FieldLabel>
            <Select
              value={draft.adapterId}
              onValueChange={(adapterId) => {
                setDraft({ ...draft, adapterId })
                setProbe(undefined)
              }}
            >
              <SelectTrigger className="w-full min-w-0">
                <SelectValue placeholder={t('panel.noRuntimes')} />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {adapters.map((id) => (
                    <SelectItem key={id} value={id}>
                      {id}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          <Field className="min-w-0">
            <FieldLabel htmlFor="runtime-path">{t('panel.runtime.path')}</FieldLabel>
            <Input
              id="runtime-path"
              className="min-w-0 font-mono text-[11px]"
              placeholder="/usr/local/bin/dsh"
              value={draft.executablePath}
              onChange={(event) => {
                setDraft({ ...draft, executablePath: event.target.value })
                setProbe(undefined)
              }}
            />
            <FieldDescription className="break-words">{t('panel.runtime.pathHint')}</FieldDescription>
          </Field>
        </FieldGroup>

        {probe && (
          <p className={cn('break-words text-[11px]', probe.ok ? 'text-success' : 'text-destructive')}>
            {probe.ok ? `${t('panel.runtime.probeOk')}${probe.version ? ` · ${probe.version}` : ''}` : probe.error}
          </p>
        )}

        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={!draft.adapterId || !draft.executablePath || checking}
            onClick={() => void check()}
          >
            {checking ? <Spinner /> : <Zap />} {checking ? t('panel.runtime.checking') : t('panel.runtime.check')}
          </Button>
          <Button
            size="sm"
            className="flex-1"
            disabled={!draft.adapterId || !draft.executablePath || busy}
            onClick={() => {
              void onSave({
                id: draft.id,
                adapterId: draft.adapterId,
                executablePath: draft.executablePath,
                ...(draft.label.trim() ? { label: draft.label.trim() } : {}),
              }).then(() => setDraft(undefined))
            }}
          >
            {t('action.save')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex min-w-0 items-center gap-2">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-xs font-semibold">{t('panel.runtime')}</h2>
          <p className="text-[10px] tabular-nums text-muted-foreground">
            {rows.length} {t('panel.runtime.onThisMac')}
          </p>
        </div>
        <Button size="icon-xs" variant="ghost" disabled={busy} title={t('panel.rescan')} onClick={onRescan}>
          {busy ? <Spinner /> : <RefreshCw />}
        </Button>
        <Button size="xs" onClick={beginNew}>
          <Plus /> {t('panel.addRuntime')}
        </Button>
      </div>

      {rows.length === 0 ? (
        <Empty className="gap-2 border py-8">
          <EmptyHeader>
            <EmptyDescription>{t('panel.noRuntimes')}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <ItemGroup className="gap-1.5">
          {rows.map(({ runtime, manual }) => {
            const health = runtime?.health ?? 'unavailable'
            const adapterId = runtime?.adapterId ?? manual?.adapterId ?? 'unknown'
            const name = manual?.label ?? adapterId
            const capabilityCount = runtime
              ? Object.values(runtime.capabilities).filter((value) => value === true).length
              : 0
            const content = (
              <ItemContent className="min-w-0 gap-1">
                <ItemTitle className="w-full min-w-0 gap-1.5 text-[12px]">
                  <span className="min-w-0 truncate">{name}</span>
                  {name !== adapterId && <span className="shrink-0 text-[10px] font-normal text-muted-foreground">{adapterId}</span>}
                </ItemTitle>
                <div className="flex min-w-0 flex-wrap items-center gap-1">
                  <Badge variant="outline" className="px-1.5 py-0 text-[9px]">
                    {manual ? t('panel.runtime.sourceManual') : t('panel.runtime.sourceDetected')}
                  </Badge>
                  <Badge variant={healthVariant(health)} className="px-1.5 py-0 text-[9px]">
                    {t(`panel.runtime.health.${health}` as never)}
                  </Badge>
                  {runtime?.version && <span className="text-[10px] text-muted-foreground">{runtime.version}</span>}
                  {capabilityCount > 0 && (
                    <span className="text-[10px] text-muted-foreground">
                      · {capabilityCount} {t('panel.runtime.capabilities')}
                    </span>
                  )}
                </div>
                <ItemDescription className="line-clamp-1 break-all font-mono text-[10px]" title={runtime?.executablePath ?? manual?.executablePath}>
                  {runtime?.executablePath ?? manual?.executablePath}
                </ItemDescription>
              </ItemContent>
            )

            return (
              <Item key={runtime?.id ?? manual?.id} variant="outline" size="sm" className="min-w-0 flex-nowrap px-3 py-2.5">
                {content}
                {manual && (
                  <ItemActions className="shrink-0">
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      title={t('panel.runtime.edit')}
                      onClick={() => {
                        setProbe(undefined)
                        setDraft({ ...manual, label: manual.label ?? '', isNew: false })
                      }}
                    >
                      <Pencil />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      className="text-destructive hover:text-destructive"
                      title={t('panel.delete')}
                      onClick={() => setDeleteEntry(manual)}
                    >
                      <Trash2 />
                    </Button>
                  </ItemActions>
                )}
              </Item>
            )
          })}
        </ItemGroup>
      )}

      {snapshot.diagnostics.length > 0 && (
        <div className="flex min-w-0 flex-col gap-2 pt-1">
          <Separator />
          <p className="text-[10px] font-medium text-muted-foreground">{t('panel.runtime.diagnostics')}</p>
          <div className="flex min-w-0 flex-col gap-1">
            {snapshot.diagnostics.map((line) => (
              <p key={line} className="break-words text-[10px] leading-snug text-muted-foreground">
                {line}
              </p>
            ))}
          </div>
        </div>
      )}

      <ConfirmDialog
        open={Boolean(deleteEntry)}
        onOpenChange={(open) => {
          if (!open) setDeleteEntry(undefined)
        }}
        title={t('panel.runtime.deleteTitle')}
        description={t('panel.runtime.deleteConfirm')}
        cancelLabel={t('action.cancel')}
        confirmLabel={t('panel.delete')}
        onConfirm={() => {
          if (deleteEntry) onDelete(deleteEntry.id)
          setDeleteEntry(undefined)
        }}
      />
    </div>
  )
}
