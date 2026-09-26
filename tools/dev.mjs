#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

/**
 * One command for a development session:
 *
 *   pnpm dev            daemon + tray (panel built first)
 *   pnpm dev --web      ... plus the Vite dev server for the inspector
 *   pnpm dev --no-tray  ... headless (daemon only, or daemon + web)
 *
 * Everything runs in the foreground with a prefixed log and dies together on
 * Ctrl-C, so there is no "start these four things in four terminals" ritual.
 */

const root = resolve(import.meta.dirname, '..')
const flags = new Set(process.argv.slice(2))
const withWeb = flags.has('--web')
const withTray = !flags.has('--no-tray')

const colors = { relayd: '\u001b[36m', web: '\u001b[35m', tray: '\u001b[32m', dev: '\u001b[90m' }
const reset = '\u001b[0m'
const children = new Set()
let shuttingDown = false

function log(name, line) {
  const prefix = (colors[name] ?? colors.dev) + name.padEnd(6) + reset
  for (const part of String(line).split('\n')) {
    if (part.trim()) process.stdout.write(prefix + ' ' + part + '\n')
  }
}

function start(name, command, args, options = {}) {
  const child = spawn(command, args, {
    cwd: root,
    env: { ...process.env, ...options.env },
    stdio: ['ignore', 'pipe', 'pipe'],
    // Own process group: pnpm and Electron both fork, and a bare SIGTERM to the
    // wrapper would leave the real daemon running.
    detached: true,
  })
  children.add(child)
  child.stdout.on('data', (chunk) => log(name, chunk))
  child.stderr.on('data', (chunk) => log(name, chunk))
  child.on('exit', (code, signal) => {
    children.delete(child)
    if (shuttingDown) return
    log('dev', name + ' exited (' + (signal ?? code) + '), stopping the rest')
    shutdown(code ?? 0)
  })
  child.on('error', (error) => log(name, 'failed to start: ' + error.message))
  return child
}

/** Runs a step to completion; the dev session cannot start half-configured. */
function once(name, command, args) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] })
    let output = ''
    child.stdout.on('data', (chunk) => {
      output += chunk
    })
    child.stderr.on('data', (chunk) => {
      output += chunk
    })
    child.on('exit', (code) => (code === 0 ? resolvePromise() : reject(new Error(name + ' failed:\n' + output))))
  })
}

function stop(child) {
  try {
    process.kill(-child.pid, 'SIGTERM')
  } catch {
    try {
      child.kill('SIGTERM')
    } catch {
      // already gone
    }
  }
}

function shutdown(code = 0) {
  if (shuttingDown) return
  shuttingDown = true
  for (const child of children) stop(child)
  setTimeout(() => process.exit(code), 300)
}

async function waitForDaemon(timeoutMs = 30_000) {
  const infoPath = process.env.RELAY_HOME
    ? join(process.env.RELAY_HOME, 'server.json')
    : join(homedir(), '.relay', 'server.json')
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (existsSync(infoPath)) {
      try {
        const info = JSON.parse(readFileSync(infoPath, 'utf8'))
        const response = await fetch(info.url + '/api/health?token=' + info.token)
        if (response.ok) return info
      } catch {
        // daemon still coming up
      }
    }
    await new Promise((done) => setTimeout(done, 400))
  }
  throw new Error('relayd did not become healthy within ' + timeoutMs + 'ms')
}

if (!existsSync(join(root, 'apps/menu-bar/out/panel/index.html'))) {
  log('dev', 'building the control panel (the daemon serves it in development)')
  await once('panel build', 'pnpm', ['--filter', '@relay/menu-bar', 'build:panel'])
}

// The daemon owns configuration, the environment scan and the Codex lifecycle.
start('relayd', 'pnpm', ['exec', 'tsx', 'watch', 'apps/relayd/src/serve.ts'])

const info = await waitForDaemon()
log('dev', 'daemon healthy at ' + info.url + ' (db ' + info.database + ')')
log('dev', 'inspector  ' + info.url + '/#t=' + info.token)
log('dev', 'panel      ' + info.url + '/panel/#t=' + info.token)

if (withWeb) {
  start('web', 'pnpm', ['--filter', '@relay/web', 'dev'])
  log('dev', 'vite       http://127.0.0.1:7354/#t=' + info.token)
}

if (withTray) {
  // Electron must not inherit ELECTRON_RUN_AS_NODE, and the tray must attach to
  // the daemon above instead of spawning the bundled one.
  const env = { ...process.env, RELAY_NO_AUTOSTART: '1' }
  delete env.ELECTRON_RUN_AS_NODE
  start('tray', 'pnpm', ['--filter', '@relay/menu-bar', 'exec', 'electron', '.'], { env })
}

process.on('SIGINT', () => shutdown(0))
process.on('SIGTERM', () => shutdown(0))
process.on('exit', () => {
  for (const child of children) stop(child)
})
