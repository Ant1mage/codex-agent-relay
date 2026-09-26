# Codex Relay Icon

Pure vector relay/branching mark.

Design constraints:
- transparent background
- no outer card
- no outer ring
- no glow
- no shadow
- no gradient
- no letter C
- provider-neutral
- monochrome Codex-like visual language

Files:
- svg/relay-icon.svg         dark mark for light UI
- svg/relay-icon-dark.svg    light mark for dark UI
- png/light/*                transparent PNG exports
- png/dark/*                 transparent PNG exports

## How Relay consumes these

This directory is the canonical Relay icon package; its geometry and appearance
are final.

- In-app Relay marks (sidebar, onboarding, empty state, small branding) use
  `svg/relay-icon.svg` directly as a CSS mask, so `currentColor` controls the
  colour and the mark follows the UI theme. Only its shape and alpha matter, so
  the dark variant is never used for masking.
- Raster needs (favicon, development Dock icon, non-macOS window icon) use
  `png/light/*` directly.
- The PNG exports are deliverables, not build output. Do not regenerate them
  from the SVG.

There is no icon build script and no generated icon directory in this repo.
