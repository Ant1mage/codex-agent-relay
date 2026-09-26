import { describe, expect, it } from 'vitest'
import { parseInstalledPlugin, parseMarketplaceRoot, readMcpEntry } from '../src/environment.js'

describe('readMcpEntry', () => {
  it('reads command and args out of the relay table only', () => {
    const contents = [
      '[mcp_servers.other]',
      'command = "other"',
      'args = ["--x"]',
      '',
      '[mcp_servers.relay]',
      'command = "/usr/bin/node"',
      'args = ["/opt/relay/mcp/stdio.js"]',
      '',
      '[projects."/tmp"]',
      'trust = true',
    ].join('\n')
    expect(readMcpEntry(contents)).toEqual({ command: '/usr/bin/node', args: ['/opt/relay/mcp/stdio.js'] })
  })

  it('reports nothing when Relay is not configured', () => {
    expect(readMcpEntry('[mcp_servers.other]\ncommand = "x"')).toBeUndefined()
  })
})

describe('codex CLI output parsing', () => {
  it('finds our plugin and its state', () => {
    const output = [
      'Marketplace `relay`',
      '',
      'PLUGIN    STATUS              VERSION        SOURCE',
      'relay@relay  installed, enabled  0.0.0+abc123   /Users/x/.relay/codex-plugin/plugins/relay',
      'other@x      not installed                     /tmp/other',
    ].join('\n')
    expect(parseInstalledPlugin(output)).toEqual({
      status: 'installed, enabled',
      source: '/Users/x/.relay/codex-plugin/plugins/relay',
    })
  })

  it('does not mistake another plugin for ours', () => {
    expect(parseInstalledPlugin('relayx@relay  installed, enabled  1')).toBeUndefined()
  })

  it('finds the registered marketplace root', () => {
    const output = ['MARKETPLACE  ROOT', 'openai-bundled  /tmp/bundled', 'relay  /Users/x/.relay/codex-plugin'].join('\n')
    expect(parseMarketplaceRoot(output)).toBe('/Users/x/.relay/codex-plugin')
  })
})
