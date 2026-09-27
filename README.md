# Relay — External Agent Runtime for OpenAI Codex

[English](README.md) · [简体中文](README.zh-CN.md)

[![CI](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex decides what to delegate. Relay controls how it runs.**

Relay is a local runtime and control plane for Codex. Codex delegates a bounded
task to an external coding agent; Relay resolves the runtime, applies policy,
starts and supervises the worker process, and reports the result back into the
same Codex session.

Relay is not another AI IDE: no editor, no chat window, no agent loop of its own.
Planning, review and orchestration stay in Codex.

## Stack

| Layer | Implementation |
| --- | --- |
| Desktop shell | Tauri 2 (Rust), menu bar only |
| UI | Leptos + Trunk → WebAssembly |
| Relay Core | Rust (`crates/relay-core`) |
| Daemon (`relayd`) | Rust + Axum (HTTP/SSE) |
| MCP server (`relay-mcp`) | Rust + rmcp (stdio) |
| Storage | SQLite through `rusqlite` |
| Configuration | TOML (`~/.relay/config.toml`) |
| Worker runtimes | External CLIs, spawned as separate processes |

There is no Node.js, Electron, Chromium, npm or Vite anywhere — not at runtime
and not in the build.

## Current MVP

The verified end-to-end slice is **macOS + Codex + DeepSeek**:

| | |
| --- | --- |
| Platform | macOS, Apple silicon |
| Host | Codex (CLI or IDE extension) |
| Worker runtime | DeepSeek Harness (`dsh`), installed and authenticated |

Kimi, Z.ai and Antigravity adapters are in the tree with parser fixtures, but
they have no end-to-end verification yet.

## Build and run

Requirements: macOS on Apple silicon, Rust (stable), and
[Trunk](https://trunkrs.dev) for the UI.

```bash
cargo build --workspace                 # core, daemon, MCP server, desktop shell
cargo test --workspace                  # unit tests + the delegation end-to-end test
cargo install trunk --locked
(cd apps/relay-desktop/ui && trunk build --release)

./scripts/dev.sh                       # debug session: UI + daemon + menu bar app
```

`./scripts/dev.sh` builds the Leptos UI, builds the debug binaries and starts the
menu bar app with the daemon behind it, printing the inspector and panel URLs. It
keeps its state in `~/.relay-dev`, so it never touches an installed Relay.
`--daemon-only`, `--stop`, `--watch-ui` and `--no-ui` cover the rest.

```bash
cargo run -p relayd                     # the daemon alone, on 127.0.0.1:7352
(cd apps/relay-desktop && cargo tauri dev)   # the menu bar app (starts relayd itself)
```

`cargo tauri build` produces `Relay.app` and a DMG. Full instructions, including
signing and notarization, are in [docs/development.md](docs/development.md).

## Quick start

1. **Launch Relay.** The menu bar icon appears; the daemon runs behind it.
   `relayd` prints the inspector URL, including a one-time token.
2. **Check Runtimes.** Menu bar → Runtimes. Detection *finds* CLIs; it never
   creates an agent for you. If your `dsh` lives outside the usual locations,
   register the executable by hand in the same tab.
3. **Create an Agent.** Control panel → Agents → New agent. Pick the runtime,
   choose the model and reasoning values the CLI itself reports, set
   capabilities, save.
4. **Install into Codex.** Control panel → Codex → Install. Relay writes a local
   plugin marketplace, registers the `relay` MCP server pointing at its own Rust
   binary, and shows five checks.
5. **Delegate from Codex.**

```text
$relay use DeepSeek to review the current implementation and report potential issues.
```

## How it works

```text
Codex ──$relay / MCP──► relay-mcp ──► RunController ──► runtime adapter ──► dsh
  ▲                          │                                                 │
  └──── result.summary ──────┘◄──────────── Relay events ◄────────────────────┘

relayd        ──► configuration, runtime scan, Codex integration, HTTP/SSE
menu bar      ──► status, quick actions, configuration
inspector     ◄── execution state, events, logs
```

Relay owns runtime execution, policy, lifecycle and observability. Codex owns
planning and review. Native CLIs keep their own agent loop; Relay never
re-implements one.

Full architecture: [docs/architecture.md](docs/architecture.md).

## Supported runtimes

| Runtime | CLI | Adapter | Status |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | **Supported / MVP** |
| Kimi Code | `kimi` | `kimi-code` | Planned |
| Antigravity CLI | `agy` | `antigravity-cli` | Planned |
| Z.ai / GLM | `zai-cli` | `zai-cli` | Planned |

`dsh` versions differ in what they expose: a version with a `--json` stream is
used in structured mode, one without it in bounded plain-text mode. Relay reports
which one it found instead of assuming.

## Documentation

- [docs/architecture.md](docs/architecture.md) — crates, processes, data model, events.
- [docs/codex-integration.md](docs/codex-integration.md) — what Relay installs into Codex, checks, repair.
- [docs/development.md](docs/development.md) — build, test, release, signing.

## License

Released under the [MIT License](LICENSE).
