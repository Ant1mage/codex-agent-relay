import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'

/**
 * The menu bar mark is the canonical Relay light PNG used as a template image:
 * macOS renders only its alpha channel, so one asset follows both appearances
 * (assets/appicon/README.md). No tray-specific asset and no icon build step.
 *
 * A packaged app carries the assets in Contents/Resources (electron-builder
 * extraResources); a source checkout reads them from the repository.
 */
export function trayIconPath(size: 16 | 32): string {
  const relative = `appicon/png/light/relay-icon-${size}.png`
  const candidates = app.isPackaged
    ? [join(process.resourcesPath, relative), join(process.resourcesPath, 'assets', relative)]
    : [join(import.meta.dirname, '../../../assets', relative), join(import.meta.dirname, 'assets', relative)]
  return candidates.find((candidate) => existsSync(candidate)) ?? candidates[0] ?? relative
}
