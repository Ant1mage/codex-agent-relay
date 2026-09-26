import { mkdirSync, rmSync, symlinkSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'

/**
 * The MCP server is the one bundle that keeps a native dependency external
 * (better-sqlite3 cannot be bundled into JS). Node resolves bare imports from
 * the file's directory upwards, so the bundle needs a node_modules beside it:
 * this links the hoisted copy into out/ so `node out/mcp/stdio.js` runs.
 *
 * A packaged build must copy the package instead of linking it.
 */
const require = createRequire(resolve('packages/core/package.json'))
const target = dirname(require.resolve('better-sqlite3/package.json'))
const linkDirectory = resolve('out/node_modules')
mkdirSync(linkDirectory, { recursive: true })
const link = join(linkDirectory, 'better-sqlite3')
rmSync(link, { recursive: true, force: true })
symlinkSync(target, link, 'dir')
process.stdout.write(`linked better-sqlite3 → ${link}\n`)
