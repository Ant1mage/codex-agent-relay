# Relay Web UI

The Leptos application provides two views served by `relayd`:

| Route | Purpose |
| --- | --- |
| `/`, `/s/<session>`, `/s/<session>/r/<run>` | Inspector for sessions, runs, events, and worker output |
| `/panel` | Control panel for agents, runtimes, policy, and Codex integration |

The UI is compiled to WebAssembly and served by the local daemon. It calls the
daemon's HTTP API for data and receives live run updates over server-sent events.

## Build

From this directory:

```bash
cargo check --target wasm32-unknown-unknown
trunk build
trunk build --release
```

Trunk writes the bundle to `dist/`. For full workspace and desktop build steps,
see [Development](../../../docs/development.md).

## Structure

```text
src/main.rs          route selection
src/api.rs           HTTP and server-sent events client
src/state.rs         inspector and control-panel state
src/views/           inspector and control-panel pages
src/components/      shared controls and layout
src/i18n.rs          English and Simplified Chinese strings
src/format.rs        event and console formatting
styles.css           theme and layout
```

The daemon provides the API token to the UI. Requests authenticate with that
token; event streams use the token in their URL because `EventSource` cannot set
request headers.
