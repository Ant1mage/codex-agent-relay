import { join } from 'node:path'
import { app } from 'electron'

/**
 * Canonical Relay icon exports, in assets/appicon/png/light.
 *
 * These PNGs are the design deliverables and are used as-is; Relay never
 * re-rasterises the SVG to produce platform rasters. Vite copies assets/ into
 * the renderer output (see publicDir in electron.vite.config.ts), so the
 * packaged path mirrors the source tree.
 */
export function appIconPath(size: 16 | 32 | 64 | 128 | 256 | 512 | 1024): string {
  const relative = `appicon/png/light/relay-icon-${size}.png`
  return app.isPackaged
    ? join(import.meta.dirname, '../renderer', relative)
    : join(import.meta.dirname, '../../../../assets', relative)
}
