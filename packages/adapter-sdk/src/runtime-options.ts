import { spawnSync } from 'node:child_process'

import type {
  AdapterCapabilities,
  ModelOption,
  ReasoningLevel,
  RuntimeOptions,
} from '@relay/protocol'

/**
 * Help-text probing for model and reasoning choices.
 *
 * Relay must not invent model names or reasoning levels: a picker that cannot
 * affect a run is worse than no picker. These helpers read what a CLI says about
 * itself and report only that. When a CLI does not advertise a flag, the probe
 * reports nothing and explains why, and the UI shows the CLI's own default
 * rather than a fabricated list.
 */

export interface HelpEvidence {
  /** Raw help/usage output, stdout and stderr concatenated. */
  text: string
  /** Executable the output came from, used in diagnostics. */
  executablePath: string
}

/**
 * Reads a CLI's usage output once so both the capability probe and the options
 * report can share it. Mirrors the `spawnSync(..., '--help')` probing the
 * adapters already do for feature detection.
 */
export function readHelp(
  executablePath: string,
  prefixArgs: string[] = [],
  environment?: NodeJS.ProcessEnv,
): HelpEvidence {
  try {
    const result = spawnSync(executablePath, [...prefixArgs, '--help'], {
      encoding: 'utf8',
      timeout: 5_000,
      env: { ...process.env, ...environment },
    })
    return {
      text: `${result.stdout ?? ''}\n${result.stderr ?? ''}`,
      executablePath,
    }
  } catch {
    // A CLI that refuses --help simply reports no options.
    return { text: '', executablePath }
  }
}

function cap(value: string, max: number): string {
  return value.length > max ? value.slice(0, max) : value
}

function unique(values: string[]): string[] {
  return [...new Set(values)]
}

/** Flag forms to accept for the model selector, most specific first. */
const MODEL_FLAG_NAMES = ['model', 'models', 'model-name'] as const
const REASONING_FLAG_NAMES = ['reasoning-effort', 'reasoning', 'thinking', 'effort'] as const

/**
 * Finds a `--flag` in help text and returns the flag name plus the values the
 * CLI lists for it, e.g. `--model <model>  Model to use: flash, pro`.
 */
function findFlag(
  text: string,
  names: readonly string[],
): { flag: string; values: string[] } | undefined {
  for (const name of names) {
    // The flag must end after its name: "=", whitespace, a metavariable or the
    // end of line. This keeps --model from matching --model-name/--model-config.
    const line = new RegExp(`^.*--${name}(?=[\\s=<\\[]|$)[^\\n]*$`, 'im').exec(text)?.[0]
    if (!line) continue
    const values: string[] = []
    // Enumeration forms a CLI might use: "a, b, c", "(a|b|c)", "[a|b]".
    const afterColon = /:\s*([^\n]+)$/.exec(line)?.[1]
    const inParens = /[([]([^)\]]+)[)\]]/.exec(line)?.[1]
    for (const segment of [afterColon, inParens]) {
      if (!segment) continue
      for (const token of segment.match(/[A-Za-z][A-Za-z0-9_.-]*/g) ?? []) values.push(token)
    }
    // A metavariable placeholder such as <model> is not a value.
    const placeholder = /[<[]([A-Za-z0-9_.-]+)[>\]]/.exec(line)?.[1]?.toLowerCase()
    const choices = unique(values).filter((value) => value.toLowerCase() !== placeholder)
    return { flag: name, values: choices }
  }
  return undefined
}

/** True when the CLI help advertises a model flag at all. */
export function detectModelFlag(evidence: HelpEvidence): string | undefined {
  const match = findFlag(evidence.text, MODEL_FLAG_NAMES)
  return match ? `--${match.flag}` : undefined
}

/**
 * Builds the model list purely from what the CLI lists. A CLI that takes a model
 * but does not enumerate names yields an empty list plus a diagnostic, which the
 * UI renders as "this CLI does not publish its model list".
 */
export function parseModels(evidence: HelpEvidence): { models: ModelOption[]; flag?: string; diagnostics: string[] } {
  const match = findFlag(evidence.text, MODEL_FLAG_NAMES)
  if (!match) {
    return {
      models: [],
      diagnostics: [
        `${evidence.executablePath} does not advertise a model flag in --help; Relay cannot offer model selection for it`,
      ],
    }
  }
  const models = unique(match.values).map((value) => ({ value, label: cap(value, 200) }))
  if (!models.length) {
    return {
      models: [],
      flag: `--${match.flag}`,
      diagnostics: [
        `${evidence.executablePath} accepts --${match.flag} but does not list model names in --help; enter one in Settings or rely on the CLI default`,
      ],
    }
  }
  return { models, flag: `--${match.flag}`, diagnostics: [] }
}

const DEFAULT_LEVEL_LABELS = ['Low', 'Medium', 'High', 'Very high', 'Max']

/**
 * Builds reasoning levels from the CLI's enumeration. When the CLI accepts a
 * reasoning flag but does not enumerate values, this falls back to a plain
 * numeric scale and records that the labels are Relay's, not the CLI's.
 */
export function parseReasoning(evidence: HelpEvidence): {
  levels: ReasoningLevel[]
  flag?: string
  diagnostics: string[]
} {
  const match = findFlag(evidence.text, REASONING_FLAG_NAMES)
  if (!match) {
    return {
      levels: [],
      diagnostics: [
        `${evidence.executablePath} does not advertise a reasoning flag in --help; Relay cannot offer reasoning control for it`,
      ],
    }
  }
  const values = unique(match.values).slice(0, 5)
  if (!values.length) {
    return {
      levels: DEFAULT_LEVEL_LABELS.map((label, index) => ({
        strength: index + 1,
        label,
        value: String(index + 1),
      })),
      flag: `--${match.flag}`,
      diagnostics: [
        `${evidence.executablePath} accepts --${match.flag} without listing levels; Relay shows a 1-5 scale and passes the number`,
      ],
    }
  }
  return {
    levels: values.map((value, index) => ({
      strength: index + 1,
      label: cap(value.charAt(0).toUpperCase() + value.slice(1), 200),
      value,
    })),
    flag: `--${match.flag}`,
    diagnostics: [],
  }
}

/**
 * Builds a runtime's reported options from one help snapshot. `modelSelection`
 * in the returned capabilities is true only when a model flag was found, so the
 * UI can tell "no models" apart from "selection unsupported".
 */
export function probeRuntimeOptions(
  declared: AdapterCapabilities,
  evidence: HelpEvidence,
  runtimeId: string,
  adapterId: string,
): { capabilities: AdapterCapabilities; options: RuntimeOptions } {
  const models = parseModels(evidence)
  const reasoning = parseReasoning(evidence)
  return {
    capabilities: { ...declared, modelSelection: models.flag !== undefined },
    options: {
      runtimeId,
      adapterId,
      models: models.models,
      levels: reasoning.levels,
      // 'cli' when the CLI advertised a list; 'default' means nothing was found.
      source: models.models.length || reasoning.levels.length ? 'cli' : 'default',
      ...(models.flag ? { modelFlag: models.flag } : {}),
      ...(reasoning.flag ? { reasoningFlag: reasoning.flag } : {}),
      diagnostics: [...models.diagnostics, ...reasoning.diagnostics],
    },
  }
}

/**
 * Appends model and reasoning flags to a CLI argument list.
 *
 * Only flags the CLI advertised in its own --help are used, and only when the
 * profile actually carries a value, so a runtime without model support is never
 * handed an argument it does not understand.
 */
/**
 * Narrows a StartInput/ResumeInput to just the selection fields. ResumeInput
 * carries no model or reasoning, so a resume keeps the CLI's current settings.
 */
export function selectionOf(input: object): SelectionArgs {
  const candidate = input as { model?: unknown; reasoning?: unknown }
  return {
    ...(typeof candidate.model === 'string' ? { model: candidate.model } : {}),
    ...(typeof candidate.reasoning === 'string' ? { reasoning: candidate.reasoning } : {}),
  }
}

/** Shape shared by StartInput's optional model/reasoning pair. */
export type SelectionArgs = {
  readonly model?: string | undefined
  readonly reasoning?: string | undefined
}

export function withSelectionArgs(
  args: string[],
  selection: SelectionArgs,
  probed: Pick<RuntimeOptions, 'modelFlag' | 'reasoningFlag'>,
): string[] {
  const next = [...args]
  if (selection.model && probed.modelFlag) next.push(probed.modelFlag, selection.model)
  if (selection.reasoning && probed.reasoningFlag) next.push(probed.reasoningFlag, selection.reasoning)
  return next
}
