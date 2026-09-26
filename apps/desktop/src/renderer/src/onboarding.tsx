import { useEffect, useMemo, useState } from 'react'
import { Check, CircleAlert, Plus } from 'lucide-react'
import type { AgentProfile } from '@relay/protocol'
import type { CodexIntegrationStatus } from '../../shared/api.js'
import { useAppStore } from './store.js'
import { ProfileFields } from './profile-editor.js'
import { Button } from './components/ui/button.js'
import { Card, CardContent } from './components/ui/card.js'
import {
  providerMetadata,
  providerOrder,
  runtimeForProvider,
  useRuntimeOptions,
  type ProviderIconId,
  type Translator,
} from './ui.js'

/**
 * Onboarding pages. `agent` is not a page: it is the inline configuration state
 * that the Add action switches into, so onboarding stays on one interaction
 * plane instead of stacking a modal (docs/ui.md 22.2).
 */
type OnboardingPage = 'codex' | 'agents' | 'ready'

const PAGES: OnboardingPage[] = ['codex', 'agents', 'ready']

/**
 * First-run onboarding (docs/ui.md 22).
 *
 * A top-oriented setup workspace inside the normal shell: the sidebar stays
 * visible, the heading sits near the top, and each semantic group is one compact
 * native surface. Deliberately neither a giant white card nor an unstructured
 * landing page.
 */
export function Onboarding({ t }: { t: Translator }) {
  const { snapshot, refresh, setNotice } = useAppStore()
  const [page, setPage] = useState<OnboardingPage>('codex')
  const [codex, setCodex] = useState<CodexIntegrationStatus>()
  const [draft, setDraft] = useState<AgentProfile>()
  const [installing, setInstalling] = useState(false)

  const loadCodexStatus = () => {
    setCodex(undefined)
    void window.relay.codexStatus().then(setCodex)
  }
  useEffect(() => { loadCodexStatus() }, [])

  const installCodexIntegration = async () => {
    setInstalling(true)
    try {
      const result = await window.relay.installCodexIntegration()
      setCodex(result.status)
      setNotice(result.messages.join(' · '))
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error))
    } finally {
      setInstalling(false)
    }
  }

  const runtimes = snapshot?.runtimes ?? []
  const profiles = snapshot?.profiles ?? []
  const usableAgents = profiles.filter((profile) => profile.enabled)
  const draftRuntime = runtimes.find((runtime) => runtime.id === draft?.runtimeId)
  const { options: draftOptions, loading: draftLoading } = useRuntimeOptions(draft?.runtimeId)

  const check = (id: CodexIntegrationCheckId) => codex?.checks.find((item) => item.id === id)
  const pageIndex = PAGES.indexOf(page)

  const saveDraft = async () => {
    if (!draft) return
    await window.relay.saveProfile(draft)
    await refresh()
    setDraft(undefined)
  }

  const addProfile = (provider: ProviderIconId, runtimeId: string) => {
    const metadata = providerMetadata[provider]
    setDraft({
      id: `${provider}-${Date.now()}`,
      name: metadata.label,
      runtimeId,
      description: `${metadata.label} worker added from Relay onboarding.`,
      capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: false, networkAccess: false },
      enabled: true,
    })
  }

  const finish = async () => {
    await window.relay.completeOnboarding()
    await refresh()
  }

  const title = useMemo(() => {
    if (page === 'codex') return t('onboarding.connect')
    if (page === 'agents') return t('onboarding.addAgents')
    return t('onboarding.readyTitle')
  }, [page, t])

  return (
    <main className="onboarding">
      <Card className="onboarding-card">
        <CardContent className="flex flex-col gap-4">
        <div className="onboarding-meta">
          <span>{t('onboarding.setup')}</span>
          <span>{pageIndex + 1} {t('onboarding.of')} {PAGES.length}</span>
        </div>
        <h1>{title}</h1>

        {page === 'codex' && (
          <>
            <p className="onboarding-lede">{t('onboarding.codexLede')}</p>
            <div className="onboarding-group" role="list">
              {!codex && <div className="onboarding-row" role="listitem"><span className="onboarding-note">{t('common.refresh')}…</span></div>}
              {codex && codex.checks.map((item) => (
                <div className="onboarding-row" role="listitem" key={item.id}>
                  <span className={item.ok ? 'tick' : 'warn'}>
                    {item.ok ? <Check size={13} /> : <CircleAlert size={13} />}
                  </span>
                  <span className="onboarding-row-label">{t(`onboarding.check.${item.id}`)}</span>
                  <em title={item.detail}>{item.detail}</em>
                </div>
              ))}
            </div>
            {codex && !codex.configured && (
              <p className="onboarding-note">{t('onboarding.installIntegration')}</p>
            )}
            <div className="onboarding-actions">
              <Button variant="link" size="sm" onClick={loadCodexStatus}>{t('onboarding.checkRetry')}</Button>
              {!codex?.configured && (
                <Button variant="outline" disabled={installing || !codex} onClick={() => void installCodexIntegration()}>
                  {installing ? t('onboarding.installing') : t('onboarding.install')}
                </Button>
              )}
              <Button disabled={!codex?.configured} onClick={() => setPage('agents')}>{t('onboarding.continue')}</Button>
            </div>
          </>
        )}

        {page === 'agents' && !draft && (
          <>
            <p className="onboarding-lede">{t('onboarding.agentsLede')}</p>
            <h2 className="onboarding-section">{t('onboarding.detectedOnComputer')}</h2>
            <div className="onboarding-group" role="list">
              {providerOrder.map((provider) => {
                const runtime = runtimeForProvider(provider, runtimes)
                const metadata = providerMetadata[provider]
                return (
                  <div className="onboarding-row" role="listitem" key={provider}>
                    <img className="provider-icon" src={metadata.src} alt="" />
                    <span className="onboarding-row-label">{metadata.label}</span>
                    <em className={runtime ? 'detected' : undefined}>
                      {runtime ? t('agents.detected') : t('agents.notInstalled')}
                    </em>
                    {runtime
                      ? (
                        <Button variant="outline" size="sm" onClick={() => addProfile(provider, runtime.id)}>
                          <Plus size={11} />{t('agents.add')}
                        </Button>
                      )
                      : <span className="onboarding-row-spacer" />}
                  </div>
                )
              })}
            </div>
            {usableAgents.length > 0 && (
              <>
                <h2 className="onboarding-section">{t('onboarding.selected')}</h2>
                <div className="onboarding-group" role="list">
                  {usableAgents.map((profile) => (
                    <div className="onboarding-row" role="listitem" key={profile.id}>
                      <span className="onboarding-row-label">{profile.name}</span>
                      <em>{[profile.model, profile.reasoning].filter(Boolean).join(' · ')}</em>
                    </div>
                  ))}
                </div>
              </>
            )}
            <div className="onboarding-actions">
              <Button variant="link" size="sm" onClick={() => setPage('codex')}>{t('onboarding.back')}</Button>
              <Button disabled={!usableAgents.length} onClick={() => setPage('ready')}>
                {t('onboarding.continue')}
              </Button>
            </div>
          </>
        )}

        {/* Inline agent configuration: same plane, no nested modal. */}
        {page === 'agents' && draft && (
          <>
            <p className="onboarding-lede">{t('onboarding.configureAgent')} · {draft.name}</p>
            <form
              className="onboarding-group onboarding-form"
              onSubmit={(event) => {
                event.preventDefault()
                void saveDraft().catch((error: unknown) =>
                  setNotice(error instanceof Error ? error.message : String(error)))
              }}
            >
              <ProfileFields
                profile={draft}
                runtimes={runtimes}
                t={t}
                onChange={setDraft}
               
                hideRuntime
              />
              {!draftLoading && draftOptions && !draftOptions.models.length && !draftOptions.levels.length && (
                <p className="onboarding-note">{t('agents.noModelList')}</p>
              )}
              <div className="onboarding-actions">
                <Button type="button" variant="link" size="sm" onClick={() => setDraft(undefined)}>
                  {t('action.cancel')}
                </Button>
                <Button disabled={!draft.name.trim()}>{t('onboarding.addAgent')}</Button>
              </div>
            </form>
          </>
        )}

        {page === 'ready' && (
          <>
            <p className="onboarding-lede">{t('onboarding.readyLede')}</p>
            <h2 className="onboarding-section">{t('onboarding.codexSection')}</h2>
            <div className="onboarding-group">
              <div className="onboarding-row">
                <span className={check('relay-mcp')?.ok && check('relay-skill')?.ok ? 'tick' : 'warn'}>
                  {check('relay-mcp')?.ok && check('relay-skill')?.ok
                    ? <Check size={13} />
                    : <CircleAlert size={13} />}
                </span>
                <span className="onboarding-row-label">{t('onboarding.connected')}</span>
                <em>{check('relay-mcp')?.detail ?? ''}</em>
              </div>
            </div>
            <h2 className="onboarding-section">{t('onboarding.agentsSection')}</h2>
            <div className="onboarding-group">
              {usableAgents.map((profile) => (
                <div className="onboarding-row" key={profile.id}>
                  <span className="onboarding-row-label">{profile.name}</span>
                  <em>{[profile.model, profile.reasoning].filter(Boolean).join(' · ')}</em>
                </div>
              ))}
            </div>
            <p className="onboarding-note">{t('onboarding.askCodex')}</p>
            <div className="onboarding-actions">
              <Button variant="link" size="sm" onClick={() => setPage('agents')}>{t('onboarding.back')}</Button>
              <Button
                disabled={!codex?.configured || usableAgents.length === 0}
                onClick={() => void finish().catch((error: unknown) =>
                  setNotice(error instanceof Error ? error.message : String(error)))}
              >
                {t('onboarding.done')}
              </Button>
            </div>
          </>
        )}
        </CardContent>
      </Card>
    </main>
  )
}

type CodexIntegrationCheckId = 'codex-cli' | 'relay-mcp' | 'relay-skill'
