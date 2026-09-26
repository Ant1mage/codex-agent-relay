import { useEffect, useState } from 'react'
import type { RelayPolicy, RelayPolicyOverride } from '@relay/protocol'
import type { RelayConfigView } from '@relay/relay-api'
import { Button } from '../components/ui/button.js'
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card.js'
import { Input } from '../components/ui/input.js'
import { Label } from '../components/ui/label.js'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select.js'
import { Switch } from '../components/ui/switch.js'
import { Field, FieldGroup, FieldLabel } from '../components/ui/field.js'
import type { Translator } from '../lib/i18n.js'

const SWITCHES = [
  { key: 'allowWrite', label: 'settings.allowWrite' },
  { key: 'allowCommands', label: 'settings.allowCommands' },
  { key: 'allowNetwork', label: 'settings.allowNetwork' },
  { key: 'requireWorktreeForParallelWriters', label: 'settings.requireWorktree' },
] as const

function NumberField({
  id,
  label,
  value,
  min,
  max,
  onCommit,
}: {
  id: string
  label: string
  value: number
  min: number
  max: number
  onCommit(value: number): void
}) {
  const [draft, setDraft] = useState(String(value))
  useEffect(() => setDraft(String(value)), [value])
  const commit = () => {
    const next = Number(draft)
    if (Number.isInteger(next) && next >= min && next <= max) onCommit(next)
    else setDraft(String(value))
  }
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
        }}
      />
    </Field>
  )
}

/**
 * Routing policy: Global defaults plus optional per-workspace overrides. This is
 * the same file relay-mcp resolves before every run, so a change applies to the
 * next delegation without restarting Codex.
 */
export function PolicyView({
  t,
  config,
  workspaces,
  onSave,
}: {
  t: Translator
  config: RelayConfigView
  workspaces: string[]
  onSave(policy: RelayPolicy, workspaceOverrides: Record<string, RelayPolicyOverride>): void
}) {
  const [policy, setPolicy] = useState<RelayPolicy>(config.policy)
  const [overrides, setOverrides] = useState<Record<string, RelayPolicyOverride>>(config.workspaceOverrides)
  const [workspace, setWorkspace] = useState<string>(workspaces[0] ?? '')

  useEffect(() => setPolicy(config.policy), [config.policy])
  useEffect(() => setOverrides(config.workspaceOverrides), [config.workspaceOverrides])

  const override = overrides[workspace] ?? {}
  const patchOverride = (change: Partial<RelayPolicyOverride>) =>
    setOverrides((current) => ({ ...current, [workspace]: { ...current[workspace], ...change } }))

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle className="text-sm">{t('settings.global')}</CardTitle>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-3">
          <FieldGroup className="gap-3">
            <NumberField
              id="max-runs"
              label={t('settings.maxRuns')}
              value={policy.maxConcurrentRuns}
              min={1}
              max={32}
              onCommit={(value) => setPolicy({ ...policy, maxConcurrentRuns: value })}
            />
            <NumberField
              id="max-writers"
              label={t('settings.maxWriters')}
              value={policy.maxConcurrentWriters}
              min={1}
              max={16}
              onCommit={(value) => setPolicy({ ...policy, maxConcurrentWriters: value })}
            />
            {SWITCHES.map((item) => (
              <div key={item.key} className="flex min-w-0 items-center justify-between gap-3">
                <Label className="min-w-0 truncate text-xs font-normal">{t(item.label)}</Label>
                <Switch
                  className="shrink-0"
                  checked={policy[item.key]}
                  onCheckedChange={(checked: boolean) => setPolicy({ ...policy, [item.key]: checked })}
                />
              </div>
            ))}
          </FieldGroup>
        </CardContent>
      </Card>

      <Card className="min-w-0">
        <CardHeader>
          <CardTitle className="text-sm">{t('settings.workspaceSection')}</CardTitle>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-3">
          <Select value={workspace} onValueChange={setWorkspace}>
            <SelectTrigger className="w-full min-w-0">
              <SelectValue placeholder={t('settings.workspace')} />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {workspaces.map((path) => (
                  <SelectItem key={path} value={path}>
                    {path}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          {workspace ? (
            <>
              {SWITCHES.map((item) => (
                <div key={item.key} className="flex min-w-0 items-center justify-between gap-3">
                  <Label className="min-w-0 truncate text-xs font-normal">{t(item.label)}</Label>
                  <Switch
                    className="shrink-0"
                    checked={override[item.key] ?? policy[item.key]}
                    onCheckedChange={(checked: boolean) => patchOverride({ [item.key]: checked })}
                  />
                </div>
              ))}
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    setOverrides((current: Record<string, RelayPolicyOverride>) => {
                      const next = { ...current }
                      delete next[workspace]
                      return next
                    })
                  }
                >
                  {t('panel.workspace.clear')}
                </Button>
              </div>
            </>
          ) : (
            <p className="text-xs text-muted-foreground">{t('panel.workspace.none')}</p>
          )}
        </CardContent>
      </Card>

      <Button size="sm" className="w-full" onClick={() => onSave(policy, overrides)}>
        {t('action.save')}
      </Button>
    </div>
  )
}
