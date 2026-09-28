# Development

## Requirements

| Tool | Why |
| --- | --- |
| macOS on Apple silicon | The only supported platform |
| Rust stable (1.85+) with the `aarch64-apple-darwin` target | Everything |
| `wasm32-unknown-unknown` target | The Leptos UI |
| [Trunk](https://trunkrs.dev) (`cargo install trunk --locked`) | Bundling the UI |
| Tauri CLI (`cargo install tauri-cli --version "^2" --locked`) | The desktop shell |
| Xcode command line tools | Linking and signing |

```bash
rustup target add wasm32-unknown-unknown
```

## Workspace

```text
crates/relay-core        domain, events, projection, policy, run lifecycle, adapter trait
crates/relay-adapters    runtime CLI adapters
crates/relay-storage     SQLite persistence
crates/relay-config      config.toml
crates/relay-api         wire contract + HTTP/SSE surface
crates/relay-codex       Codex identity and integration lifecycle
apps/relayd              the daemon
apps/relay-mcp           the MCP server
apps/relay-desktop/ui    the Leptos UI (its own crate, built for wasm)
apps/relay-desktop/src-tauri  the Tauri shell
```

`apps/relay-desktop/ui` is deliberately outside the root workspace: it is built
for `wasm32-unknown-unknown` by Trunk, while everything else is built for the
host.

## Everyday commands

```bash
cargo build --workspace          # everything except the UI
cargo test --workspace           # unit tests + the MCP delegation end-to-end test
cargo clippy --workspace --all-targets
cargo fmt --all

(cd apps/relay-desktop/ui && cargo check --target wasm32-unknown-unknown)
(cd apps/relay-desktop/ui && trunk build --release)   # → ui/dist
```

## Running a development session

One command builds the UI, builds the debug binaries and starts the menu bar app
with the daemon behind it:

```bash
./scripts/dev.sh                 # UI + debug binaries + desktop shell
./scripts/dev.sh --daemon-only   # just relayd, then open the URLs in a browser
./scripts/dev.sh --stop          # stop the session
./scripts/dev.sh --watch-ui      # also rebuild the UI on change
./scripts/dev.sh --no-ui         # skip the Trunk build (dist/ already exists)
```

The script prints the inspector and panel URLs with their token. A development
session keeps its state in `~/.relay-dev` (override with `RELAY_HOME`), so it
never touches an installed Relay, and it stops only the daemons it started.

The pieces on their own:

```bash
cargo run -p relayd              # the daemon; prints the inspector URL and token
(cd apps/relay-desktop && cargo tauri dev)   # the shell, which starts the daemon
cargo run -p relay-mcp           # the MCP server, exactly as Codex starts it
```

Useful environment variables (all optional):

| Variable | Effect |
| --- | --- |
| `RELAY_HOME` | State directory (default `~/.relay`) |
| `RELAY_PORT` | First port the daemon tries (default 7352; it falls back to the next free one) |
| `RELAY_TOKEN` | Fixed API token instead of a random one |
| `RELAY_DB_PATH`, `RELAY_CONFIG_PATH` | Move the database or the configuration file |
| `RELAY_WEB_ROOT` | Serve a different UI build |
| `RELAY_RESOURCES_DIR` | Where the daemon and the Codex integration find packaged assets |
| `RELAY_NO_AUTOSTART=1` | The desktop shell does not start the daemon |
| `RELAY_DAEMON_COMMAND` | Run a specific daemon binary |
| `RELAY_MCP_ENTRY` | The MCP entry Relay writes into `config.toml` |
| `RUST_LOG` | `tracing` filter, e.g. `RUST_LOG=relay_adapters=debug` |

## Testing

`cargo test --workspace` runs unit tests and the MCP delegation integration
tests. `apps/relay-mcp/tests/delegation.rs` exercises the stdio tools, run
lifecycle, and result projection with fixture CLIs. To test a real runtime,
register it in `~/.relay/config.toml` and delegate from Codex.

## Packaging

```bash
cargo build --release -p relayd -p relay-mcp
(cd apps/relay-desktop/ui && trunk build --release)
(cd apps/relay-desktop && cargo tauri build --target aarch64-apple-darwin)
```

The bundle contains:

```text
Relay.app/Contents/
├── MacOS/Relay              the Tauri shell
├── Resources/bin/relayd     the daemon (also what the shell spawns)
├── Resources/bin/relay-mcp  the MCP server Codex starts
├── Resources/ui/            the Leptos build served by the daemon
├── Resources/integrations/codex/   plugin, hooks and skill sources
└── Info.plist               LSUIElement: menu bar only, no Dock icon
```

`relayd` and `relay-mcp` are ordinary workspace binaries; `bundle.resources`
maps them from `target/release` to `bin/` inside the bundle, which is why
`cargo build --release -p relayd -p relay-mcp` must run before
`cargo tauri build`. The bundle lands in
`target/aarch64-apple-darwin/release/bundle/`.

## Release signing and update checks

The release workflow builds and signs the app and bundled executables,
notarizes the app, and uploads the DMG to the GitHub Release for its version
tag. At startup and when the user selects **Check for updates**, the app
compares its version with GitHub's latest published release. If a newer version
exists, the user can open the release page and download the DMG manually.
Hardened runtime and entitlements are configured in the project.

Required CI secrets: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
`KEYCHAIN_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_BASE64`,
`APPLE_API_KEY_ID`, and `APPLE_API_ISSUER`.

## Local state

Relay stores its configuration, run history, and session data under `~/.relay`
by default. `./scripts/dev.sh` uses `~/.relay-dev` instead. Existing databases
with an incompatible schema are reset rather than migrated, so back up
`relay.sqlite` before upgrading between development versions. Use a separate
`RELAY_HOME` for disposable test runs; do not delete the default state directory
to troubleshoot a build.
