import { useEffect, useState } from 'react'
import { Check, CircleAlert, Plus } from 'lucide-react'
import type { AgentProfile } from '@relay/protocol'
import type { CodexIntegrationStatus } from '../../shared/api.js'
import { useAppStore } from './store.js'
import {
  providerMetadata,
  providerOrder,
  runtimeForProvider,
  type ProviderIconId,
  type Translator,
} from './ui.js'
import { ProfileEditor } from './profile-editor.js'

type OnboardingStep = 'intro' | 'codex' | 'agents' | 'ready'

/**
 * Lightweight progress line. Numbered labels with hairline rules read as
 * progress without the boxed stepper chrome that made this look like a wizard.
 */
function StepLine({ step, t }: { step: Exclude<OnboardingStep, 'intro'>; t: Translator }) {
  const order: Exclude<OnboardingStep, 'intro'>[] = ['codex', 'agents', 'ready']
  const labels = [t('onboarding.connect'), t('onboarding.addAgents'), t('onboarding.ready')]
  const active = order.indexOf(step)
  return (
    <div className="onboarding-steps">
      {order.map((id, index) => (
        <span className={index === active ? 'on' : index < active ? 'ok' : 'todo'} key={id}>
          {index < active ? `\u2713 ${labels[index]}` : `${index + 1} ${labels[index]}`}
          {index < order.length - 1 && <i />}
        </span>
      ))}
    </div>
  )
}

/**
 * First-run onboarding (docs/ui.md 22).
 *
 * The content sits directly on the window background rather than inside a
 * floating card: one column, hairline separators, no shadow. Renders inside the
 * normal shell so the sidebar stays visible.
 */
export function Onboarding({ t }: { t: Translator }) {
  const { snapshot, refresh, setNotice } = useAppStore()
  const [step, setStep] = useState<OnboardingStep>('intro')
  const [codex, setCodex] = useState<CodexIntegrationStatus>()
  const [editing, setEditing] = useState<AgentProfile>()

  const loadCodexStatus = () => {
    setCodex(undefined)
    void window.relay.codexStatus().then(setCodex)
  }
  useEffect(() => { loadCodexStatus() }, [])

  const runtimes = snapshot?.runtimes ?? []
  const profiles = snapshot?.profiles ?? []
  const usableAgents = profiles.filter((profile) => profile.enabled)

  const finish = async () => {
    await window.relay.completeOnboarding()
    await refresh()
  }

  const addProfile = (provider: ProviderIconId, runtimeId: string) => {
    const metadata = providerMetadata[provider]
    setEditing({
      id: `${provider}-${Date.now()}`,
      name: metadata.label,
      runtimeId,
      description: `${metadata.label} worker added from Relay onboarding.`,
      capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: false, networkAccess: false },
      enabled: true,
    })
  }

  return (
    <main className="onboarding">
      <div className="onboarding-pane">
        {step === 'intro' && (
          <>
            <span className="onboarding-mark mark-tinted" role="img" aria-label="Relay" />
            <h1>{t('onboarding.title')}</h1>
            <p className="onboarding-lede">{t('onboarding.intro')}</p>
            <div className="onboarding-actions">
              <button className="primary" onClick={() => setStep('codex')}>{t('onboarding.start')}</button>
            </div>
          </>
        )}

        {step !== 'intro' && (
          <>
            <StepLine step={step} t={t} />

            {step === 'codex' && (
              <>
                <h1>{t('onboarding.connect')}</h1>
                <p className="onboarding-label">{t('onboarding.detection')}</p>
                {!codex && <p className="onboarding-note">{t('common.refresh')}…</p>}
                {codex && (
                  <ul className="onboarding-checks">
                    {codex.checks.map((check) => (
                      <li key={check.id}>
                        <span className={check.ok ? 'tick' : 'warn'}>
                          {check.ok ? <Check size={13} /> : <CircleAlert size={13} />}
                        </span>
                        <span className="onboarding-check-label">{t(`onboarding.check.${check.id}`)}</span>
                        <em title={check.detail}>{check.detail}</em>
                      </li>
                    ))}
                  </ul>
                )}
                {codex && !codex.configured && (
                  <p className="onboarding-note">
                    {t('onboarding.manualHint')}{' '}
                    <code>{codex.checks.find((check) => !check.ok)?.detail}</code>
                  </p>
                )}
                <div className="onboarding-actions">
                  <button className="link-action" onClick={loadCodexStatus}>{t('onboarding.checkRetry')}</button>
                  <button className="primary" onClick={() => setStep('agents')}>{t('onboarding.continue')}</button>
                </div>
              </>
            )}

            {step === 'agents' && (
              <>
                <h1>{t('onboarding.addAgents')}</h1>
                <p className="onboarding-label">{t('onboarding.detectedOnComputer')}</p>
                <div className="provider-rows">
                  {providerOrder.map((provider) => {
                    const runtime = runtimeForProvider(provider, runtimes)
                    const metadata = providerMetadata[provider]
                    return (
                      <div className="provider-row" key={provider}>
                        <img className="provider-icon" src={metadata.src} alt="" />
                        <strong>{metadata.label}</strong>
                        <span className={`provider-row-status${runtime ? ' detected' : ''}`}>
                          {runtime ? t('agents.detected') : t('agents.notInstalled')}
                        </span>
                        <button
                          className="outline"
                          disabled={!runtime}
                          onClick={() => runtime && addProfile(provider, runtime.id)}
                        >
                          <Plus size={11} />{t('agents.add')}
                        </button>
                      </div>
                    )
                  })}
                </div>
                <p className="onboarding-note">{t('onboarding.agentsGoal')}</p>
                <div className="onboarding-actions">
                  <button className="link-action" onClick={() => setStep('codex')}>{t('onboarding.back')}</button>
                  <button className="primary" disabled={!usableAgents.length} onClick={() => setStep('ready')}>
                    {t('onboarding.continue')}
                  </button>
                </div>
              </>
            )}

            {step === 'ready' && (
              <>
                <h1>{t('onboarding.ready')}</h1>
                <p className="onboarding-lede">
                  {t('onboarding.readyBody')} {usableAgents.length} {t('onboarding.agentsAvailable')}.
                </p>
                <p className="onboarding-label">{t('onboarding.selected')}</p>
                <ul className="onboarding-agents">
                  {usableAgents.map((profile) => (
                    <li key={profile.id}>
                      <strong>{profile.name}</strong>
                      <em>{[profile.model, profile.reasoning].filter(Boolean).join(' · ')}</em>
                    </li>
                  ))}
                </ul>
                <p className="onboarding-label">{t('onboarding.openCodex')}</p>
                <ul className="onboarding-checks">
                  <li>
                    <span className={codex?.checks.find((c) => c.id === 'relay-mcp')?.ok ? 'tick' : 'warn'}>
                      {codex?.checks.find((c) => c.id === 'relay-mcp')?.ok
                        ? <Check size={13} />
                        : <CircleAlert size={13} />}
                    </span>
                    <span className="onboarding-check-label">{t('onboarding.check.relay-mcp')}</span>
                    <em title={codex?.checks.find((c) => c.id === 'relay-mcp')?.detail}>
                      {codex?.checks.find((c) => c.id === 'relay-mcp')?.detail ?? ''}
                    </em>
                  </li>
                  <li>
                    <span className={codex?.checks.find((c) => c.id === 'relay-skill')?.ok ? 'tick' : 'warn'}>
                      {codex?.checks.find((c) => c.id === 'relay-skill')?.ok
                        ? <Check size={13} />
                        : <CircleAlert size={13} />}
                    </span>
                    <span className="onboarding-check-label">{t('onboarding.check.relay-skill')}</span>
                    <em title={codex?.checks.find((c) => c.id === 'relay-skill')?.detail}>
                      {codex?.checks.find((c) => c.id === 'relay-skill')?.detail ?? ''}
                    </em>
                  </li>
                </ul>
                {codex && !codex.configured && (
                  <p className="onboarding-note">{t('onboarding.installIntegration')}</p>
                )}
                <p className="onboarding-note">{t('onboarding.askCodex')}</p>
                <div className="onboarding-actions">
                  <button className="link-action" onClick={() => setStep('agents')}>{t('onboarding.back')}</button>
                  <button
                    className="primary"
                    onClick={() => void finish().catch((error: unknown) =>
                      setNotice(error instanceof Error ? error.message : String(error)))}
                  >
                    {t('onboarding.done')}
                  </button>
                </div>
              </>
            )}
          </>
        )}

        {editing && (
          <ProfileEditor
            profile={editing}
            runtimes={runtimes}
            t={t}
            close={() => setEditing(undefined)}
          />
        )}
      </div>
    </main>
  )
}
