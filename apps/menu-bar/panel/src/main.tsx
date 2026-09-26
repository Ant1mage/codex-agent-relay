import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App.js'
import { applySystemAppearance } from './lib/system-theme.js'
import './styles.css'

const systemAppearance = window.matchMedia('(prefers-color-scheme: dark)')
const syncSystemAppearance = ({ matches }: Pick<MediaQueryList, 'matches'>) =>
  applySystemAppearance(document.documentElement, matches)

syncSystemAppearance(systemAppearance)
systemAppearance.addEventListener('change', syncSystemAppearance)

const container = document.getElementById('root')
if (!container) throw new Error('Relay panel is missing its root element')
createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
