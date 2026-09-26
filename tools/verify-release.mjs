import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('..', import.meta.url).pathname
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))
const dist = join(root, 'dist')
const appResources = join(dist, 'mac-arm64', 'Relay.app', 'Contents', 'Resources')
const required = [
  join(appResources, 'relayd', 'serve.js'),
  join(appResources, 'relayd', 'web', 'index.html'),
  join(appResources, 'relayd', 'panel', 'index.html'),
  join(appResources, 'mcp', 'stdio.js'),
  join(appResources, 'codex', 'plugin.json'),
  join(appResources, 'app-update.yml'),
  join(dist, `Relay-${version}-arm64.zip`),
  join(dist, `Relay-${version}-arm64.dmg`),
  join(dist, 'latest-mac.yml'),
]

const missing = required.filter((path) => !existsSync(path))
if (missing.length > 0) {
  throw new Error(`Release is incomplete; missing:\n${missing.join('\n')}`)
}

const metadata = readFileSync(join(dist, 'latest-mac.yml'), 'utf8')
if (!metadata.includes(`version: ${version}`) || !/sha512:\s*\S+/.test(metadata)) {
  throw new Error('latest-mac.yml does not contain the package version and sha512 integrity metadata')
}

process.stdout.write(`Verified Relay ${version}: app resources, zip, dmg and update integrity metadata are present.\n`)
