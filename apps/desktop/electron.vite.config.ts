import { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'electron-vite'

const workspaceAliases = {
  '@relay/adapter-antigravity': resolve(import.meta.dirname, '../../packages/adapters/antigravity/src/index.ts'),
  '@relay/adapter-deepseek': resolve(import.meta.dirname, '../../packages/adapters/deepseek/src/index.ts'),
  '@relay/adapter-gemini': resolve(import.meta.dirname, '../../packages/adapters/gemini/src/index.ts'),
  '@relay/adapter-kimi': resolve(import.meta.dirname, '../../packages/adapters/kimi/src/index.ts'),
  '@relay/adapter-zai': resolve(import.meta.dirname, '../../packages/adapters/zai/src/index.ts'),
  '@relay/adapter-sdk': resolve(import.meta.dirname, '../../packages/adapter-sdk/src/index.ts'),
  '@relay/core': resolve(import.meta.dirname, '../../packages/core/src/index.ts'),
  '@relay/i18n': resolve(import.meta.dirname, '../../packages/i18n/src/index.ts'),
  '@relay/protocol': resolve(import.meta.dirname, '../../packages/protocol/src/index.ts'),
}

export default defineConfig({
  main: {
    resolve: { alias: workspaceAliases },
    build: {
      externalizeDeps: {
        exclude: [
          '@relay/adapter-antigravity',
          '@relay/adapter-deepseek',
          '@relay/adapter-gemini',
          '@relay/adapter-kimi',
          '@relay/adapter-zai',
          '@relay/adapter-sdk',
          '@relay/core',
          '@relay/protocol',
        ],
      },
      rollupOptions: {
        input: resolve(import.meta.dirname, 'src/main/index.ts'),
      },
    },
  },
  preload: {
    resolve: { alias: workspaceAliases },
    build: {
      rollupOptions: {
        input: resolve(import.meta.dirname, 'src/preload/index.ts'),
        output: {
          format: 'cjs',
          entryFileNames: 'index.cjs',
        },
      },
    },
  },
  renderer: {
    resolve: { alias: workspaceAliases },
    root: resolve(import.meta.dirname, 'src/renderer'),
    publicDir: resolve(import.meta.dirname, '../../assets'),
    plugins: [react()],
    build: {
      rollupOptions: {
        input: resolve(import.meta.dirname, 'src/renderer/index.html'),
      },
    },
  },
})
