#!/usr/bin/env bash
#
# One command for a Relay development session — no packaging, no Electron, no Node.
#
#   ./scripts/dev.sh                 build the UI, then run the desktop shell (debug)
#   ./scripts/dev.sh --daemon-only   run just relayd and print the browser URLs
#   ./scripts/dev.sh --stop          stop everything this script starts
#   ./scripts/dev.sh --no-ui         skip the Trunk build (dist/ already exists)
#   ./scripts/dev.sh --watch-ui      also rebuild the UI on change (trunk watch)
#
# Development state lives in ~/.relay-dev by default, so a debug session never
# touches the state of an installed Relay. Override with RELAY_HOME or RELAY_PORT.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

UI_DIR="apps/relay-desktop/ui"
BIN_DIR="target/debug"
LOG_FILE="${RELAY_DEV_LOG:-/tmp/relay-dev.log}"

MODE="desktop"
BUILD_UI=1
WATCH_UI=0

for argument in "$@"; do
  case "$argument" in
    --daemon-only) MODE="daemon" ;;
    --stop) MODE="stop" ;;
    --no-ui) BUILD_UI=0 ;;
    --watch-ui) WATCH_UI=1 ;;
    -h|--help) sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $argument (try --help)" >&2; exit 2 ;;
  esac
done

# The desktop shell passes its environment to the daemon it starts, so exporting
# these here is what keeps a debug session isolated and reproducible.
export RELAY_HOME="${RELAY_HOME:-$HOME/.relay-dev}"
export RELAY_PORT="${RELAY_PORT:-7352}"

UI_WATCH_PID=""
DAEMON_PID=""

cleanup() {
  local status=$?
  if [ -n "$UI_WATCH_PID" ]; then
    kill "$UI_WATCH_PID" 2>/dev/null || true
  fi
  if [ "${RELAY_DEV_KEEP_DAEMON:-0}" != "1" ]; then
    stop_own_daemon
  fi
  exit $status
}

# The pid the daemon recorded for itself, if any.
recorded_daemon_pid() {
  if [ -f "$RELAY_HOME/server.json" ]; then
    sed -n 's/.*"pid": \([0-9]*\).*/\1/p' "$RELAY_HOME/server.json"
  fi
}

# True when a pid belongs to a development daemon from this checkout. An
# installed Relay is never touched, and neither is another session's daemon.
is_our_daemon() {
  local pid="$1"
  [ -n "$pid" ] || return 1
  ps -o command= -p "$pid" 2>/dev/null | grep -q "$BIN_DIR/relayd"
}

# Stops the daemon this session started: the one we launched ourselves, or the
# one the desktop shell started for us (found through its own record).
stop_own_daemon() {
  if [ -n "$DAEMON_PID" ] && is_our_daemon "$DAEMON_PID"; then
    kill "$DAEMON_PID" 2>/dev/null || true
  fi
  local recorded
  recorded="$(recorded_daemon_pid || true)"
  if [ -n "${recorded:-}" ] && is_our_daemon "$recorded"; then
    kill "$recorded" 2>/dev/null || true
  fi
  rm -f "$RELAY_HOME/daemon.lock" "$RELAY_HOME/server.json" 2>/dev/null || true
}

stop_everything() {
  echo "stopping the development session…"
  pkill -f "$REPO_ROOT/$BIN_DIR/relay-desktop" 2>/dev/null || true
  pkill -f "cargo-tauri dev" 2>/dev/null || true
  pkill -f "trunk watch" 2>/dev/null || true
  # Every development daemon, but only from this checkout.
  pkill -f "$REPO_ROOT/$BIN_DIR/relayd" 2>/dev/null || true
  pkill -f "$REPO_ROOT/$BIN_DIR/relay-mcp" 2>/dev/null || true
  rm -f "$RELAY_HOME/daemon.lock" "$RELAY_HOME/server.json" 2>/dev/null || true
  echo "stopped. state kept in $RELAY_HOME (remove it for a clean slate)"
}

if [ "$MODE" = "stop" ]; then
  stop_everything
  exit 0
fi

# cargo, trunk and cargo-tauri live in ~/.cargo/bin; work even from a shell with
# a minimal PATH (a background launch, a fresh terminal, an editor task).
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
  export PATH
fi

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }

if [ "$BUILD_UI" = 1 ]; then
  command -v trunk >/dev/null || {
    echo "trunk is missing: cargo install trunk --locked" >&2
    exit 1
  }
  echo "building the UI (Leptos → wasm)…"
  # Trunk reads NO_COLOR and only accepts true/false, so an exported 1 aborts it.
  (cd "$UI_DIR" && env -u NO_COLOR trunk build --release)
elif [ ! -f "$UI_DIR/dist/index.html" ]; then
  echo "$UI_DIR/dist is missing; run without --no-ui first" >&2
  exit 1
fi

echo "building relayd, relay-mcp and the desktop shell (debug)…"
cargo build -p relayd -p relay-mcp -p relay-desktop

if [ "$WATCH_UI" = 1 ]; then
  echo "watching the UI for changes…"
  (cd "$UI_DIR" && env -u NO_COLOR trunk watch >"$LOG_FILE.ui" 2>&1) &
  UI_WATCH_PID=$!
fi

# Prints the URLs once the daemon has written its record, in the background so it
# works for both modes (the shell starts the daemon itself).
announce_endpoints() {
  local waited=0
  while [ "$waited" -lt 60 ]; do
    if [ -f "$RELAY_HOME/server.json" ]; then
      local token port
      token="$(sed -n 's/.*"token": "\([^"]*\)".*/\1/p' "$RELAY_HOME/server.json")"
      port="$(sed -n 's/.*"port": \([0-9]*\).*/\1/p' "$RELAY_HOME/server.json")"
      if [ -n "$token" ] && [ -n "$port" ]; then
        cat <<EOF

  Relay development session is up
    inspector   http://127.0.0.1:$port/#t=$token
    panel       http://127.0.0.1:$port/panel/?lang=zh-CN#t=$token
    API         http://127.0.0.1:$port/api/health?token=$token
    state       $RELAY_HOME
    database    $RELAY_HOME/relay.sqlite

EOF
        return
      fi
    fi
    sleep 1
    waited=$((waited + 1))
  done
  echo "the daemon did not come up within 60s; see $LOG_FILE" >&2
}

trap cleanup EXIT INT TERM

if [ "$MODE" = "daemon" ]; then
  announce_endpoints &
  echo "starting relayd ($RELAY_HOME, port $RELAY_PORT)…"
  # Absolute path: the cleanup matches on it, and it must not depend on the cwd.
  "$REPO_ROOT/$BIN_DIR/relayd" 2>&1 | tee "$LOG_FILE" &
  DAEMON_PID=$!
  wait "$DAEMON_PID"
else
  command -v cargo-tauri >/dev/null || {
    echo "the Tauri CLI is missing: cargo install tauri-cli --version '^2' --locked" >&2
    exit 1
  }
  announce_endpoints &
  echo "starting the desktop shell (it starts relayd itself)…"
  # `tauri dev` rebuilds on change; the daemon it spawns is stopped on exit.
  env -u NO_COLOR cargo tauri dev
fi
