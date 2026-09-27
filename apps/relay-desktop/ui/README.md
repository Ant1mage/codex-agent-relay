# Relay UI (Leptos + Trunk)

One wasm bundle, two surfaces. `apps/relayd` serves the built `dist/` at `/`
(inspector) and `/panel` (control panel), with SPA fallback to `index.html`.

```
trunk build          # debug bundle into dist/
trunk build --release
trunk serve          # local dev server for the UI alone
```

There is no Node.js, npm, pnpm, React, Vite or Tailwind anywhere in this crate:
the stylesheet is hand-written, the icons are inline SVG and the wasm bundle is
produced by Trunk + `wasm-bindgen`.

## Surfaces

| Path | Surface | Notes |
| --- | --- | --- |
| `/`, `/s/<session>`, `/s/<session>/r/<run>` | Inspector | read-only observation of the live projection |
| `/panel?tab=…&intent=…&lang=…&profileId=…&base=…` | Control panel | Agents, Runtimes, Policy, Codex, Status |

## Bootstrap contract

* The token arrives as `#t=<token>` (or `?t=<token>`), is stored in
  `localStorage` under `relay.token`, and is stripped from the address bar with
  `history.replaceState`.
* Every `/api/*` request sends `Authorization: Bearer <token>` *and*
  `?token=<token>`; the SSE URL carries `?token=` because `EventSource` cannot
  set headers.
* No token: the inspector shows a "missing token" card, the panel shows the
  daemon-down message.
* The inspector takes its language from `localStorage['relay.locale']` then
  `navigator.language`; the panel uses `?lang=en`, anything else is `zh-CN`.

## Layout

```
src/main.rs            pathname routing between the two surfaces
src/api.rs             fetch client + SSE, ported from packages/relay-api/src/client.ts
src/dom.rs             window/history/localStorage/matchMedia/clipboard
src/format.rs          console formatting, ported from apps/web/src/lib/format.ts
src/i18n.rs            en + zh-CN message catalogue (same keys as packages/i18n)
src/state.rs           the inspector store and the panel store
src/components/        header, session rail, run strip, console pane, controls, notice
src/views/             inspector, panel, agents, runtimes, policy, codex, status
styles.css             hand-written; light/dark through CSS variables and `.dark`
```

## Building for wasm without Trunk

`cargo check --target wasm32-unknown-unknown` and
`cargo build --target wasm32-unknown-unknown` both work standalone; only the
`wasm-bindgen` glue and the `dist/` layout need Trunk.
