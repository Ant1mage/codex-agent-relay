interface ThemeRoot {
  classList: { toggle(name: string, force?: boolean): boolean }
  style: { colorScheme: string }
}

export function applySystemAppearance(root: ThemeRoot, dark: boolean): void {
  root.classList.toggle('dark', dark)
  root.style.colorScheme = dark ? 'dark' : 'light'
}
