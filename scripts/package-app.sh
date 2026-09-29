#!/usr/bin/env bash
# Build Relay's macOS app bundle in a clearly separated debug or release mode.
#
#   ./scripts/package-app.sh --mode dev       # Relay Dev.app with debug sidecars
#   ./scripts/package-app.sh --mode release   # Relay.app and its DMG
#   ./scripts/package-app.sh --mode dev --skip-ui

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="$REPO_ROOT/apps/relay-desktop/ui"
DESKTOP_DIR="$REPO_ROOT/apps/relay-desktop"
MODE=""
BUILD_UI=1

while [ "$#" -gt 0 ]; do
  argument="$1"
  shift
  case "$argument" in
    --mode|--profile)
      [ "$#" -gt 0 ] || { echo "$argument requires dev or release" >&2; exit 2; }
      [ -z "$MODE" ] || { echo "choose one mode only" >&2; exit 2; }
      MODE="$1"
      shift
      ;;
    --skip-ui) BUILD_UI=0 ;;
    -h|--help)
      sed -n '2,8p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "unknown option: $argument (try --help)" >&2; exit 2 ;;
  esac
done

case "$MODE" in
  dev|release) ;;
  *) echo "specify exactly one mode: --mode dev or --mode release" >&2; exit 2 ;;
esac

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Relay app bundles are currently supported on macOS only" >&2
  exit 1
fi
if [ "$(uname -m)" != "arm64" ]; then
  echo "Relay app bundles are currently supported on Apple silicon only" >&2
  exit 1
fi

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }
command -v trunk >/dev/null || { echo "trunk is required" >&2; exit 1; }
cargo tauri --version >/dev/null 2>&1 || {
  echo "Tauri CLI is required: cargo install tauri-cli --version '^2' --locked" >&2
  exit 1
}

if [ "$BUILD_UI" = 1 ]; then
  echo "building the optimized Leptos UI…"
  (cd "$UI_DIR" && env -u NO_COLOR trunk build --release)
elif [ ! -f "$UI_DIR/dist/index.html" ]; then
  echo "$UI_DIR/dist is missing; run without --skip-ui first" >&2
  exit 1
fi

case "$MODE" in
  dev)
    echo "building debug daemon and MCP binaries…"
    (cd "$REPO_ROOT" && cargo build -p relayd -p relay-mcp)
    echo "bundling Relay Dev.app with debug sidecars…"
    (cd "$DESKTOP_DIR" && env -u NO_COLOR cargo tauri build \
      --debug --bundles app --no-sign \
      --config src-tauri/tauri.dev.conf.json)
    APP="$REPO_ROOT/target/debug/bundle/macos/Relay Dev.app"
    echo "created: $APP"
    echo "For an isolated dev run, launch the bundled executable with:"
    echo "  RELAY_HOME=\$HOME/.relay-dev RELAY_PORT=7352 \"$APP/Contents/MacOS/relay-desktop\""
    ;;
  release)
    echo "building release daemon and MCP binaries…"
    (cd "$REPO_ROOT" && cargo build --release -p relayd -p relay-mcp)
    echo "bundling Relay.app and its DMG…"
    (cd "$DESKTOP_DIR" && env -u NO_COLOR cargo tauri build --bundles app,dmg)
    echo "created: $REPO_ROOT/target/release/bundle/macos/Relay.app"
    echo "created: $REPO_ROOT/target/release/bundle/dmg/"
    ;;
esac
