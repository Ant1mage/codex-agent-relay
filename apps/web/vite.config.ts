import { resolve } from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

/**
 * The inspector is a static build served by the Relay daemon, so every API call
 * is same-origin. In development Vite proxies /api to the daemon instead, which
 * keeps the token, the SSE stream and the production paths identical.
 */
const daemon = process.env.RELAY_DAEMON ?? 'http://127.0.0.1:7352'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Mirrors the tsconfig path so shadcn CLI generated files resolve the same way.
  resolve: { alias: { '@': resolve(import.meta.dirname, 'src') } },
  server: {
    port: 7354,
    strictPort: true,
    proxy: {
      '/api': {
        target: daemon,
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: 'out',
    emptyOutDir: true,
    sourcemap: true,
  },
})
