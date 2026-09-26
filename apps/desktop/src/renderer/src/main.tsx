import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App.js'
import { installBrowserPreview } from './browser-preview.js'
import './tailwind.css'
import './styles.css'

installBrowserPreview()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
