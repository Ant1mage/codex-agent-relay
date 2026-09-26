import { readFileSync } from 'node:fs'

/**
 * Single source of truth for the app version: the root package.json. Bundles get
 * it as a compile-time constant, so relayd, the MCP server and the tray all
 * report the same version without reading a file at runtime.
 *
 * Usage: esbuild ... --define:$(node tools/define-version.mjs)
 */
const root = new URL('../package.json', import.meta.url)
const version = JSON.parse(readFileSync(root, 'utf8')).version ?? '0.0.0'
process.stdout.write('__RELAY_VERSION__=' + JSON.stringify(version))
