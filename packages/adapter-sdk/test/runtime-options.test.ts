import { describe, expect, it } from 'vitest'
import type { AdapterCapabilities } from '@relay/protocol'
import {
  detectModelFlag,
  parseModels,
  parseReasoning,
  probeRuntimeOptions,
} from '../src/runtime-options.js'

const base: AdapterCapabilities = {
  nonInteractive: true,
  structuredEvents: true,
  cwd: true,
  resume: true,
  send: false,
  cancel: true,
  childSessions: false,
}

const evidence = (text: string) => ({ text, executablePath: '/usr/local/bin/agent' })

describe('detectModelFlag', () => {
  it('finds an advertised model flag', () => {
    expect(detectModelFlag(evidence('  --model <name>   Model to use'))).toBe('--model')
  })

  it('accepts the common aliases', () => {
    expect(detectModelFlag(evidence('  --models <list>'))).toBe('--models')
    expect(detectModelFlag(evidence('  --model-name <n>'))).toBe('--model-name')
  })

  it('reports nothing when the CLI has no model flag', () => {
    expect(detectModelFlag(evidence('  --json\n  --cwd <path>'))).toBeUndefined()
  })

  it('does not match an unrelated flag that merely contains "model"', () => {
    // --model-config is not a model selector.
    expect(detectModelFlag(evidence('  --model-config <path>'))).toBeUndefined()
  })
})

describe('parseModels', () => {
  it('reads the enumerated names from a colon list', () => {
    const result = parseModels(evidence('  --model <model>   Model to use: flash, pro'))
    expect(result.models.map((model) => model.value)).toEqual(['flash', 'pro'])
    expect(result.flag).toBe('--model')
    expect(result.diagnostics).toEqual([])
  })

  it('reads names from a parenthesised or piped list', () => {
    expect(parseModels(evidence('  --model <m>  (gpt-5|gpt-5-mini)')).models.map((m) => m.value))
      .toEqual(['gpt-5', 'gpt-5-mini'])
  })

  it('never treats the metavariable placeholder as a model name', () => {
    const result = parseModels(evidence('  --model <model>  Choose a model'))
    expect(result.models).toEqual([])
  })

  it('explains an absent flag instead of inventing models', () => {
    const result = parseModels(evidence('  --json'))
    expect(result.models).toEqual([])
    expect(result.diagnostics[0]).toContain('does not advertise a model flag')
  })

  it('explains a flag that is accepted but unnamed', () => {
    const result = parseModels(evidence('  --model <m>   Override the model'))
    expect(result.models).toEqual([])
    expect(result.flag).toBe('--model')
    expect(result.diagnostics[0]).toContain('does not list model names')
  })

  it('de-duplicates repeated names', () => {
    const result = parseModels(evidence('  --model <m>   : pro, pro, flash'))
    expect(result.models.map((model) => model.value)).toEqual(['pro', 'flash'])
  })
})

describe('parseReasoning', () => {
  it('builds ordered levels from the CLI enumeration', () => {
    const result = parseReasoning(evidence('  --reasoning <level>   Effort (low|medium|high)'))
    expect(result.levels.map((level) => level.strength)).toEqual([1, 2, 3])
    expect(result.levels.map((level) => level.value)).toEqual(['low', 'medium', 'high'])
    expect(result.levels.map((level) => level.label)).toEqual(['Low', 'Medium', 'High'])
    expect(result.flag).toBe('--reasoning')
  })

  it('supports a four-stop scale rather than forcing three', () => {
    const result = parseReasoning(evidence('  --reasoning-effort <l>  : minimal, low, high, maximum'))
    expect(result.levels).toHaveLength(4)
    expect(result.levels.at(-1)?.value).toBe('maximum')
  })

  it('falls back to a numeric scale when levels are unnamed, and says so', () => {
    const result = parseReasoning(evidence('  --reasoning <n>   Set effort'))
    expect(result.levels.map((level) => level.strength)).toEqual([1, 2, 3, 4, 5])
    expect(result.levels.map((level) => level.value)).toEqual(['1', '2', '3', '4', '5'])
    expect(result.diagnostics[0]).toContain('without listing levels')
  })

  it('reports nothing for a CLI whose real help has no model or reasoning flag', () => {
    // Verbatim flag surface of @deepseek-ai/dsh 0.1.5-rc.3, the only runtime
    // installed during development. It exposes no model/reasoning option, so the
    // probe must stay silent rather than offer a picker that cannot affect a run.
    const dshHelp = [
      'Usage: dsh [options]',
      '',
      'Options:',
      '  -V, --version                output the version number',
      '  --profile <name>             profile to run',
      '  --resume <id>                resume a session',
      '  --patch <path>               apply a patch',
      '  --dump-config                print the resolved config',
      '  --dump-default-config        print the default config',
      '  --from-default-profile       start from the default profile',
      '  --help                       display help for command',
    ].join('\n')
    const { capabilities, options } = probeRuntimeOptions(base, evidence(dshHelp), 'runtime:deepseek-harness', 'deepseek-harness')
    expect(options.models).toEqual([])
    expect(options.levels).toEqual([])
    expect(capabilities.modelSelection).toBe(false)
    expect(options.diagnostics).toHaveLength(2)
  })

  it('explains an absent reasoning flag', () => {
    const result = parseReasoning(evidence('  --json'))
    expect(result.levels).toEqual([])
    expect(result.diagnostics[0]).toContain('does not advertise a reasoning flag')
  })
})

describe('probeRuntimeOptions', () => {
  it('collects models, levels, flags and diagnostics together', () => {
    const { options } = probeRuntimeOptions(
      base,
      evidence('  --model <m>  : flash, pro\n  --reasoning <l>  (low|high)'),
      'runtime:demo',
      'demo',
    )
    expect(options.runtimeId).toBe('runtime:demo')
    expect(options.adapterId).toBe('demo')
    expect(options.models).toHaveLength(2)
    expect(options.levels).toHaveLength(2)
    expect(options.modelFlag).toBe('--model')
    expect(options.reasoningFlag).toBe('--reasoning')
    expect(options.diagnostics).toEqual([])
  })

  it('omits flags it could not find', () => {
    const { options } = probeRuntimeOptions(base, evidence('  --help'), 'r', 'a')
    expect(options.modelFlag).toBeUndefined()
    expect(options.reasoningFlag).toBeUndefined()
    expect(options.diagnostics).toHaveLength(2)
  })

  it('only claims model selection when the CLI advertises a flag', () => {
    expect(probeRuntimeOptions(base, evidence('  --model <m> : a, b'), 'r', 'a').capabilities.modelSelection).toBe(true)
    expect(probeRuntimeOptions(base, evidence('  --json'), 'r', 'a').capabilities.modelSelection).toBe(false)
  })

  it('preserves the declared capabilities', () => {
    const { capabilities } = probeRuntimeOptions(base, evidence('  --model <m> : a'), 'r', 'a')
    expect(capabilities.nonInteractive).toBe(base.nonInteractive)
    expect(capabilities.childSessions).toBe(base.childSessions)
  })
})
