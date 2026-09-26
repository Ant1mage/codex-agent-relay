#!/usr/bin/env bash
#
# Regenerate every Relay icon asset from the SVG masters in assets/appicon.
#
#   pnpm icons        # from the repo root
#   tools/build-icons.sh
#
# Source of truth (edit these, never the PNGs):
#   assets/appicon/svg/relay-icon.svg       dark mark, for light backgrounds
#   assets/appicon/svg/relay-icon-dark.svg  light mark, for dark backgrounds
#
# assets/appicon/png/** holds designer exports for reference only; everything
# Relay ships is rendered here from the SVG so the sizes stay exact.
#
# Output (assets/app-icon/build, copied into the app by Vite's publicDir):
#   appicon-*.png, appicon.icns, appicon.ico, appicon.png
#   relay-mark.png / relay-mark-dark.png, plus 2x large variants

set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$script_dir/.." && pwd)
src="$root/assets/appicon/svg"
out="$root/assets/app-icon/build"

die() { printf 'build-icons: %s\n' "$*" >&2; exit 1; }

command -v sips >/dev/null 2>&1 || die 'sips not found (this script needs macOS)'
dark_mark="$src/relay-icon.svg"
light_mark="$src/relay-icon-dark.svg"
[ -f "$dark_mark" ] || die "missing source $dark_mark"
# The light master is not rendered (the mask tints the alpha), but its presence
# is still required so the pair cannot drift apart unnoticed.
[ -f "$light_mark" ] || die "missing source $light_mark"

size() { # size <svg> <pixels> <output>
  local svg=$1 px=$2 dest=$3
  sips -s format png -z "$px" "$px" "$svg" --out "$dest" >/dev/null \
    || die "sips failed rendering $(basename -- "$svg") at ${px}px"
}

finish() { # finish <pixels> <output> [unsharp]
  local px=$1 file=$2 blur=${3:-}
  if [ -n "$blur" ]; then
    # Optional polish: some sips builds lack unsharpMask, so never fail on it.
    if sips -s format png -s unsharpMask "$blur" "$file" --out "$file.sharp" >/dev/null 2>&1 \
       && [ -s "$file.sharp" ]; then
      mv "$file.sharp" "$file"
    else
      rm -f "$file.sharp"
    fi
  fi
  local got
  got=$(sips -g pixelWidth "$file" | awk '/pixelWidth/{print $2}')
  [ "$got" = "$px" ] || die "$(basename -- "$file") is ${got}px, expected ${px}px"
}

rm -rf "$out"
mkdir -p "$out/AppIcon.iconset"

# --- macOS app icon (.icns) --------------------------------------------------
# One SVG for every size: the current mark is a bare, thick-stroked glyph that
# stays legible at 16px, so it needs no separate small-size optical variant.
for px in 16 32 128 256 512; do
  stroke=
  [ "$px" -eq 128 ] && stroke='0.4'
  # iconutil requires these exact dimensions; @2x entries are real pixel sizes.
  for rep in "$px" "$((px * 2))"; do
    label=$px
    tag=''
    [ "$rep" -ne "$px" ] && tag='@2x'
    size "$dark_mark" "$rep" "$out/AppIcon.iconset/icon_${label}x${label}${tag}.png"
    finish "$rep" "$out/AppIcon.iconset/icon_${label}x${label}${tag}.png" "$stroke"
  done
done

# --- standalone PNGs (Linux, favicon, in-app, source control) ----------------
size "$dark_mark" 1024 "$out/appicon-1024.png"
size "$dark_mark"  512 "$out/appicon-512.png"
size "$dark_mark"  256 "$out/appicon-256.png"
size "$dark_mark"  128 "$out/appicon-128.png"
size "$dark_mark"   64 "$out/appicon-64.png"
size "$dark_mark"   32 "$out/appicon-32.png"
size "$dark_mark"   16 "$out/appicon-16.png"
finish 64 "$out/appicon-64.png" 0.4
finish 32 "$out/appicon-32.png" 0.4
finish 16 "$out/appicon-16.png" 0.4
# 128 doubles as the generic PNG an icon theme or the browser can pick up.
cp "$out/appicon-128.png" "$out/appicon.png"

# --- in-app marks -----------------------------------------------------------
# Drawn beside 13px UI text, so rendered at 2x. Only the alpha channel is used:
# the renderer tints these from the current text colour with a CSS mask, which
# follows the theme automatically. The light variant therefore needs no raster.
size "$dark_mark" 32 "$out/relay-mark.png"
size "$dark_mark" 60 "$out/relay-mark-lg.png"

# --- Provider marks ---------------------------------------------------------
# Copied into the build output so the packaged app serves one icon directory.
for name in deepseek gemini glm grok kimi; do
  cp "$root/assets/providers/$name.svg" "$out/provider-$name.svg"
done

# --- Windows .ico -----------------------------------------------------------
# sips only writes a single-resolution .ico, so the container is assembled here:
# a 6-byte ICONDIR, one ICONDIRENTRY per size, then the PNG payloads. Windows
# picks the best size from this.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for px in 16 32 48 64 128 256; do
  size "$dark_mark" "$px" "$tmp/$px.png"
done
python3 - "$out/appicon.ico" "$tmp/16.png" "$tmp/32.png" "$tmp/48.png" \
                        "$tmp/64.png" "$tmp/128.png" "$tmp/256.png" <<'PY'
import struct, sys

dest, sources = sys.argv[1], sys.argv[2:]
images = []
for path in sources:
    with open(path, 'rb') as handle:
        data = handle.read()
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        sys.exit(f'build-icons: {path} is not a PNG')
    width, height = struct.unpack('>II', data[16:24])
    if width != height or not 1 <= width <= 256:
        sys.exit(f'build-icons: unsupported .ico size {width}x{height} in {path}')
    images.append((width, data))

# Directory entry stores 256 as 0.
header = struct.pack('<HHH', 0, 1, len(images))
offset = len(header) + 16 * len(images)
entries, payload = b'', b''
for size, data in images:
    dim = 0 if size == 256 else size
    entries += struct.pack('<BBBBHHII', dim, dim, 0, 0, 1, 32, len(data), offset)
    payload += data
    offset += len(data)

with open(dest, 'wb') as handle:
    handle.write(header + entries + payload)
PY

if command -v iconutil >/dev/null 2>&1; then
  iconutil -c icns "$out/AppIcon.iconset" -o "$out/appicon.icns"
else
  printf 'build-icons: iconutil not found, skipped appicon.icns\n' >&2
fi

printf 'build-icons: wrote %s\n' "$out"
ls -1 "$out"
