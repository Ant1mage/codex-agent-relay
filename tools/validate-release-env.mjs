import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('..', import.meta.url).pathname
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))
const app = JSON.parse(readFileSync(join(root, 'apps/menu-bar/package.json'), 'utf8'))
if (app.version !== version) {
  throw new Error(`Menu bar package version ${app.version} must match root version ${version}`)
}
const tag = process.argv[2]
if (tag !== `v${version}`) {
  throw new Error(`Release tag ${tag ?? '(missing)'} must match package version v${version}`)
}

const required = [
  'CSC_LINK',
  'CSC_KEY_PASSWORD',
  'APPLE_API_KEY_BASE64',
  'APPLE_API_KEY_ID',
  'APPLE_API_ISSUER',
]
const missing = required.filter((name) => !process.env[name])
if (missing.length > 0) throw new Error(`Missing release secrets: ${missing.join(', ')}`)
