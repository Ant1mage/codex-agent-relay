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

These files are the masters. `tools/build-icons.sh` (run via `pnpm icons`) reads
`svg/relay-icon.svg` and renders every shipped asset into
`assets/app-icon/build/`: the sized PNGs, `appicon.icns`, `appicon.ico`, and the
in-app marks. The `png/**` exports are kept for reference and design review; the
build does not use them, so the shipped sizes stay exact.

In-app marks are tinted from the PNG alpha channel with a CSS mask, so
`relay-icon-dark.svg` is a reference for the dark-surface colour rather than a
separate raster. Only `svg/relay-icon.svg` currently drives the build.
