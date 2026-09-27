# Relay Desktop Shell

This crate implements Relay's macOS menu bar app with Tauri 2. It opens the
control panel and inspector, manages the local `relayd` process, and handles
desktop lifecycle and updates. Runtime execution and configuration belong to
`relayd`, not the shell.

## Build and test

Run these commands from the repository root:

```bash
cargo check -p relay-desktop
cargo test -p relay-desktop
cargo clippy -p relay-desktop --all-targets
```

To run or package the desktop app, build the UI and release daemon binaries
first. See [Development](../../../docs/development.md).

## Packaging requirements

- `relayd` and `relay-mcp` are bundled as resources. Build both in release mode
  before packaging: `cargo build --release -p relayd -p relay-mcp`.
- `bundle.resources` includes the Codex integration files, the built UI, and
  app icons. The daemon serves the UI and reads the bundled integration assets.
- The app is a menu bar accessory, with no Dock icon. Runtime icons come from
  `assets/appicon/`.
- Signed updates require a Tauri updater public key in the release bundle and a
  matching private key in the release environment. See the release workflow.

## Runtime configuration

| Variable | Purpose |
| --- | --- |
| `RELAY_HOME` | Relay state directory |
| `RELAY_SERVER_INFO_PATH` | Override the daemon's `server.json` path |
| `RELAY_DAEMON_COMMAND` | Use a specific `relayd` executable |
| `RELAY_NO_AUTOSTART=1` | Do not start the daemon from the app |
| `RELAY_RESOURCES_DIR` | Override the packaged resources directory |
| `RELAY_BROWSER` | Choose the browser used for the inspector |
| `RELAY_PANEL_DEV_URL` | Load a panel from a development URL |
| `RELAY_UPDATE_FEED` | Override the update feed |

The panel receives its token in the URL fragment, not the query string. When an
open panel keeps the same daemon URL and token, the shell sends a `relay:panel`
event for navigation. A changed daemon URL or token reloads the panel.
