import type { RelayDesktopApi } from '../shared/api.js'

declare global {
  interface Window {
    relay: RelayDesktopApi
  }
}

export {}

