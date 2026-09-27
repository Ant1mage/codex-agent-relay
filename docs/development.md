# Development

Relay is Rust only: no Node.js, npm, pnpm, yarn, Bun or Deno is involved in
building, testing, running or packaging it.

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
(cd apps/relay-desktop/ui && env -u NO_COLOR trunk build --release)   # → ui/dist
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
| `RELAY_UPDATE_FEED` | Point the updater at a test feed |
| `RUST_LOG` | `tracing` filter, e.g. `RUST_LOG=relay_adapters=debug` |

## Testing

```bash
cargo test --workspace
```

Besides the unit tests, `apps/relay-mcp/tests/delegation.rs` drives the real MCP
stdio surface end to end with fixtures for the runtime CLI and the Codex
app-server:

```text
initialize → tools/list → list_agents → run_agent → wait_agent (result.summary)
  → accept_agent → completed            … plus cancel and a failing worker
```

To exercise a real CLI instead of the fixture, register it as a manual runtime in
`~/.relay/config.toml` and delegate from Codex.

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

A small environment note: `trunk` reads `NO_COLOR` and only accepts `true` or
`false`, so an exported `NO_COLOR=1` aborts it. `env -u NO_COLOR trunk build` is
the portable invocation.

## Signing and notarization

`bundle.macOS.hardenedRuntime` and the entitlements file are already configured.
The release workflow:

1. builds the UI, the daemon and the MCP server,
2. stages the updater public key and runs `cargo tauri build --config` with
   `APPLE_SIGNING_IDENTITY` set,
3. signs `Contents/Resources/bin/relayd` and
   `Contents/Resources/bin/relay-mcp` explicitly — `bin/` is where
   `bundle.resources` puts them — then re-signs the app,
4. fails the release unless both binaries exist, are executable and answer
   `--version`, and unless the updater archive, its `.sig` and `latest.json`
   are present,
5. notarizes with `xcrun notarytool` and staples the ticket,
6. uploads the DMG, the updater archive and `latest.json` to the GitHub release.

Required secrets: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
`KEYCHAIN_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_BASE64`,
`APPLE_API_KEY_ID`, `APPLE_API_ISSUER`, `TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and `TAURI_UPDATER_PUBKEY` (the public
half of the signing key; it is not a secret, but it must reach the bundle).

## Updater

A packaged app can only verify an update when it carries the *public* half of
`TAURI_SIGNING_PRIVATE_KEY`. The repository keeps `plugins.updater.pubkey`
empty and every release injects the `TAURI_UPDATER_PUBKEY` secret as a build
override:

```bash
cargo tauri build --target aarch64-apple-darwin \
  --config "$RUNNER_TEMP/updater-pubkey.json"
# {"plugins":{"updater":{"pubkey":"…"}}} — the public half, base64
```

Two guards keep an un-updatable release from shipping again. The workflow fails
before building when the secret is empty or is not a Tauri public key, and
`build.rs` fails when `RELAY_REQUIRE_UPDATER_PUBKEY=1` (only the release
workflow sets it) and the merged configuration — the `TAURI_CONFIG` overlay
Tauri passes to the build script, else `tauri.conf.json` — has no key. Local
builds leave the variable unset and still work with an empty key, where the
updater honestly reports itself as unsupported. `RELAY_UPDATE_FEED` overrides
the endpoint at runtime for testing.

## Local state

Relay deletes and recreates its own state freely — old development databases are
not migrated:

```bash
rm -rf ~/.relay        # event log, sessions, configuration, server record
```

The schema carries a `PRAGMA user_version`; a database written by an earlier
implementation is reset on first open rather than migrated.
