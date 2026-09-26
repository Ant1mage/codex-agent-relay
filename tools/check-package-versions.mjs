import { readFileSync } from 'node:fs'

function version(path) {
  return JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8')).version
}

const root = version('../package.json')
const app = version('../apps/menu-bar/package.json')
if (app !== root) {
  throw new Error(`apps/menu-bar/package.json version ${app} must match root package.json ${root}`)
}
process.stdout.write(`Release package versions agree on ${root}.\n`)
