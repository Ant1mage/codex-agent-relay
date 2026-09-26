import { resolve } from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

/**
 * The control panel is a second, tiny React app built next to the tray: it is
 * the only UI that writes Relay's configuration, and it is loaded into a
 * frameless window anchored to the menu bar icon (docs/menu-bar.md 6).
 */
export default defineConfig({
  root: resolve(import.meta.dirname, 'panel'),
  base: './',
  plugins: [react(), tailwindcss()],
  build: {
    outDir: resolve(import.meta.dirname, 'out/panel'),
    emptyOutDir: true,
    sourcemap: false,
  },
})
