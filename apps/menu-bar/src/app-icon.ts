import { join } from 'node:path'
import { app } from 'electron'

/**
 * The menu bar mark is the canonical Relay light PNG used as a template image:
 * macOS renders only its alpha channel, so one asset follows both appearances
 * (assets/appicon/README.md). No tray-specific asset and no icon build step.
 */
export function trayIconPath(size: 16 | 32): string {
  const relative = `appicon/png/light/relay-icon-${size}.png`
  return app.isPackaged
    ? join(import.meta.dirname, 'assets', relative)
    : join(import.meta.dirname, '../../../assets', relative)
}
