# relay-desktop (src-tauri)

Relay's desktop shell: **Tauri 2, Rust only**. No Node, no npm, no Vite, and no
frontend build step of any kind. The shell is one menu bar item plus the control
panel it opens; it owns the tray, the panel window, relayd supervision, the
updater, the clipboard, opening URLs and the app lifecycle. It contains no Relay
business logic — everything it shows or changes goes through relayd over HTTP.

It is the port of `apps/menu-bar`:

| Electron | Rust |
| --- | --- |
| `menu-bar/src/menu-model.ts` | `src/menu_model.rs` |
| `menu-bar/src/daemon.ts` | `src/daemon.rs` |
| `menu-bar/src/panel.ts`, `panel-target.ts` | `src/panel.rs` |
| `menu-bar/src/updater.ts` | `src/updater.rs` |
| `menu-bar/src/main.ts` | `src/shell.rs` + `src/tray.rs` |
| `packages/i18n` (menu keys) | `src/i18n.rs` |
| `packages/relay-api/src/client.ts` | `src/api.rs` |

## Build and verify

```sh
cargo check -p relay-desktop      # from the repository root
cargo test  -p relay-desktop      # 28 unit tests, no GUI session required
cargo clippy -p relay-desktop --all-targets
cargo fmt   -p relay-desktop
```

The tests cover the parts that must not need a display: the menu model (status
label precedence, both variants, the session/worker caps and overflow, blocked
agent labels, the Codex check labels, the update block), the panel URL builder
(token in the fragment, never a query parameter), the navigation event payload,
the nonce identity rule, daemon command resolution and the browser choice.

To run the app itself (`tauri dev` / `tauri build`) the sidecars and the UI
described below have to be in place first.

## Configuration notes

### `frontendDist` and the panel

`build.frontendDist` is `../ui/dist` and both commands are empty strings: the UI
is a separate Leptos/Trunk crate owned by another agent (`apps/relay-desktop/ui`)
and the installed app loads the panel over HTTP from relayd, never from embedded
assets. `apps/relay-desktop/src-tauri/build.rs` creates the *empty* directory
`../ui/dist` when it is missing, because Tauri's code generator and resource
copier both require the path to exist. Nothing inside `ui/` is ever written, and
`trunk build` overwrites the directory as usual.

### `bundle.externalBin` (the sidecars)

`relayd` and `relay-mcp` are declared as sidecars, so Tauri copies them next to
the app binary (`Contents/MacOS/relayd`), which is where `daemon.rs` looks for
them first. Tauri's build script fails when the staged file is missing, so:

* release builds refuse to build until the real binaries are staged:

  ```sh
  cargo build --release -p relayd -p relay-mcp
  cp target/release/relayd    apps/relay-desktop/src-tauri/binaries/relayd-aarch64-apple-darwin
  cp target/release/relay-mcp apps/relay-desktop/src-tauri/binaries/relay-mcp-aarch64-apple-darwin
  ```

* development builds stage an **empty** placeholder so `cargo check`/`cargo test`
  work from a fresh checkout, and say so with a `cargo:warning`. The shell treats
  an empty file as "no daemon" (`daemon::daemon_command_with` skips empty
  candidates), so a missing daemon stays a clear error instead of a silent one.
  Building `relayd` for real (`cargo build -p relayd`) puts a working binary in
  `target/debug/relayd`, which is the first place the shell looks.

`binaries/` is ignored by git (see `binaries/.gitignore`); the staging directory
is a build product.

### `bundle.resources`

The map form is used deliberately:

```json
"resources": { "../../../integrations/codex": "integrations/codex", "../ui/dist": "ui" }
```

Tauri resolves resource paths relative to *this* directory and rewrites every
`..` component to `_up_` in the destination, so the list form would bury the
Codex assets and the UI under `Contents/Resources/_up_/…`. The destinations are
the paths the runtime actually reads: `relay-codex` looks for
`<resources>/integrations/codex`, and relayd's `web_root()` looks for
`<resources>/ui` (or `<resources>/web`). The tray icons are listed as well,
because a packaged app resolves them from `Contents/Resources/assets/appicon/…`
— without that entry a bundle would have no menu bar icon and the shell would
exit at launch.

### Icons

`bundle.icon` points at the existing `build-resources/icon.icns` and the PNGs in
`assets/appicon/png/light/` — no icon is copied into this crate. The tray icon is
resolved at runtime by `daemon::asset_path`: packaged builds read
`Contents/Resources/assets/appicon/png/light/…` (and `…/appicon/…`), a source
checkout reads the repository `assets/` directory.

Electron attached a 16 pt image plus a 32 px representation to one `NSImage`;
`tray-icon` (through Tauri) takes a single image and scales it to the menu bar
height, so the shell hands it the **2× asset** (`relay-icon-32.png`) — the same
bitmap Electron carried as its 2× representation, and the one that keeps the
Retina detail. `relay-icon-16.png` is the fallback when only it exists.
`set_icon_as_template(true)` is `icon_as_template(true)`: the mark is drawn from
its alpha channel, so one asset follows both appearances. If neither asset can be
loaded the shell logs the reason and exits, exactly like the Electron app.

### Updater

`plugins.updater.endpoints` is the GitHub releases endpoint for
`Ant1mage/codex-agent-relay`; `RELAY_UPDATE_FEED` overrides it at runtime (that is how the
update path is tested locally). Two things must be filled in before a release:

* `plugins.updater.pubkey` is intentionally empty. With no public key
  `updates_supported()` returns false for an unpackaged build, and a packaged
  build without a key reports `unsupported` instead of a verification error.
  Put the minisign public key generated by `tauri signer generate` here.
* `bundle.createUpdaterArtifacts` is true, so bundling needs the matching
  `TAURI_SIGNING_PRIVATE_KEY` (and its password) in the environment.

Statuses are the Electron ones — `unsupported | idle | checking | available |
downloading | downloaded | none | error` — and the menu renders them identically.
`check`, `download`, `install` and `restart` are the whole surface. Before
installing, a *verified* daemon is stopped (macOS installs in place and does not
relaunch, so the shell restarts the app afterwards).

### No Dock icon

Two halves, because one is not enough: `Info.plist` (next to this file, merged
into the bundle by `tauri build`) sets `LSUIElement`, so the accessory app never
gets a Dock tile; `shell::run` additionally calls
`AppHandle::set_activation_policy(ActivationPolicy::Accessory)` in `setup`, which
is what makes a `tauri dev` run behave the same way. If the tray icon cannot be
built the shell logs why and exits, like the Electron app did.

### Capabilities

`capabilities/default.json` grants no plugin permissions. Every plugin is driven
from Rust, and the panel is served by relayd, so the web page never calls Tauri
IPC: it talks HTTP to the daemon. Only the platform core defaults are listed.

## Environment variables

| Variable | Effect |
| --- | --- |
| `RELAY_HOME` | State directory; `server.json` is read from here (via `relay-config`) |
| `RELAY_SERVER_INFO_PATH` | Explicit `server.json` path |
| `RELAY_DAEMON_COMMAND` | Absolute path to a relayd binary, instead of the bundled one |
| `RELAY_NO_AUTOSTART=1` | Never start relayd from the shell |
| `RELAY_RESOURCES_DIR` | Passed to a packaged daemon; also used to find assets |
| `RELAY_BROWSER` | Browser the inspector opens in, instead of the first installed Chromium |
| `RELAY_PANEL_DEV_URL` | Panel URL override for a UI dev server (adds `?base=`) |
| `RELAY_UPDATE_FEED` | Update endpoint override |

## Contract with the panel UI

The panel window loads
`<daemon>/panel/?lang=…[&tab=…][&intent=…][&profileId=…]#t=<token>`. The token is
in the fragment, never a query parameter.

When the panel is already open **and** the daemon base URL and token are
unchanged, the shell does not reload it. It dispatches a DOM event instead:

```js
window.addEventListener('relay:panel', (event) => {
  const { tab, intent, profileId } = event.detail; // any of them may be absent
});
```

A rotated token (relayd rotates on every start) or a moved port reloads the
window with the new URL instead, so the renderer never keeps a dead credential.

## Deliberate differences from the Electron app

* The tray icon is a single image rather than a multi-representation `NSImage`
  (see Icons above); the 2× asset is used.
* Electron denied `window.open` from the panel explicitly; Tauri/wry's default
  new-window handling denies it too, so no hook is installed.
* `fullscreenable: false` has no `WebviewWindowBuilder` equivalent in Tauri 2.12;
  the panel is frameless, not resizable to fullscreen by a title-bar button, and
  is hidden as soon as it loses focus.
* The daemon-unreachable messages the shell shows in the menu
  (`daemon 未运行，无法打开配置面板`, the stale `server.json` notice, the missing
  `relayd` entry notice) are ported verbatim from the TypeScript, including their
  language, so support answers keep matching the Electron app.
* An error header is truncated to 80 **characters** rather than 80 UTF-16 units,
  so a non-ASCII message cannot be cut mid-codepoint.
* An error reported by a client action (rescan, cancel, Codex repair,
  diagnostics) stays visible for at least one poll interval. In the Electron app
  the refresh that followed such an action cleared `lastError` immediately —
  because the daemon was healthy — so those messages were never rendered at all.
* `apps/relay-mcp` does not currently compile (a `HashMap`/`BTreeMap` mismatch in
  its config reloader). That is outside this crate; `cargo check -p relay-desktop`
  and `cargo test -p relay-desktop` are unaffected.
