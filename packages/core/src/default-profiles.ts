import type { AgentProfile, Runtime } from '@relay/protocol'

export function defaultProfiles(runtimes: Runtime[]): AgentProfile[] {
  return runtimes.flatMap<AgentProfile>((runtime) => {
    if (runtime.adapterId === 'deepseek-harness') return [
      {
        id: 'deepseek-code', name: 'DeepSeek Code', runtimeId: runtime.id,
        description: 'Coding worker with workspace write and command capabilities.',
        capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: false }, enabled: true,
      },
      {
        id: 'deepseek-research', name: 'DeepSeek Research', runtimeId: runtime.id,
        description: 'Read-only research worker with network access.',
        capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: true }, enabled: true,
      },
    ]
    if (runtime.adapterId === 'antigravity-cli') return [{
      id: 'antigravity-research', name: 'Antigravity Research', runtimeId: runtime.id,
      description: 'Large-context research and second-opinion worker with structured tool events.',
      capabilities: { readWorkspace: true, writeWorkspace: false, executeCommands: false, networkAccess: true }, enabled: true,
    }]
    if (runtime.adapterId === 'kimi-code') return [{
      id: 'kimi-code', name: 'Kimi Code', runtimeId: runtime.id,
      description: 'General coding worker powered by Kimi Code CLI.',
      capabilities: { readWorkspace: true, writeWorkspace: true, executeCommands: true, networkAccess: true }, enabled: true,
    }]
    if (runtime.adapterId === 'zai-cli') return [{
      id: 'glm-zai', name: 'GLM / Z.ai', runtimeId: runtime.id,
      description: 'GLM-backed structured CLI assistant for bounded analysis and second opinions.',
      capabilities: { readWorkspace: false, writeWorkspace: false, executeCommands: false, networkAccess: true }, enabled: true,
    }]
    return []
  })
}
