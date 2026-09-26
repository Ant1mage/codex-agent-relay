import { describe, expect, it } from 'vitest'
import { applySystemAppearance } from '../panel/src/lib/system-theme.js'
import { panelBackgroundColor } from '../src/panel-theme.js'

describe('Panel system appearance', () => {
  it('keeps the renderer class and color scheme aligned with macOS changes', () => {
    const classes = new Set<string>()
    const root = {
      classList: {
        toggle(name: string, force = false) {
          if (force) classes.add(name)
          else classes.delete(name)
          return force
        },
      },
      style: { colorScheme: '' },
    }

    applySystemAppearance(root, true)
    expect(classes.has('dark')).toBe(true)
    expect(root.style.colorScheme).toBe('dark')

    applySystemAppearance(root, false)
    expect(classes.has('dark')).toBe(false)
    expect(root.style.colorScheme).toBe('light')
  })

  it('uses a matching native window background before React paints', () => {
    expect(panelBackgroundColor(false)).toBe('#ffffff')
    expect(panelBackgroundColor(true)).toBe('#09090b')
  })
})
