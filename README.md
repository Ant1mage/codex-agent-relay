# Relay — External Agent Runtime for OpenAI Codex

[English](README.md) · [简体中文](README.zh-CN.md)

[![CI](https://github.com/Ant1mage/relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/relay/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex decides what to delegate. Relay controls how it runs.**

Relay is a local runtime and control plane for Codex. Codex delegates a bounded
task to an external coding agent; Relay resolves the runtime, applies policy,
starts and supervises the worker process, and reports the result back into the
same Codex session. You stay in Codex instead of moving tasks between CLIs by
hand.

Relay is not another AI IDE: no editor, no chat window, no agent loop of its own.
Planning, review and orchestration stay in Codex.

## Current MVP

The only verified end-to-end slice is **macOS + Codex + DeepSeek**:

| | |
| --- | --- |
| Platform | macOS, Apple silicon |
| Host | Codex (CLI or IDE extension) |
| Worker runtime | DeepSeek Harness (`dsh`), installed and authenticated |

Adapters for other runtimes are in the tree, but they have no end-to-end
verification yet — see [Supported Runtimes](#supported-runtimes).

## Features

- **Delegate from Codex** — a `$relay` skill plus a local MCP server; no task
  copying, no extra terminal.
- **Runtime detection** — finds installed CLIs on this machine with their
  version and health, and lists only the model/reasoning values the CLI itself
  reports.
- **Agent profiles** — bind a runtime, capabilities and instructions once
  (`deepseek-code`, `deepseek-research`, …) and reuse it from Codex.
- **Policy** — access mode (`read_only` / `propose` / `write`), global and
  per-workspace limits, concurrency caps for runs and writers, worktree
  isolation for parallel writers.
- **Lifecycle** — a delegation is a Run with Steps and iterations; cancel,
  resume, and hand results back to Codex as `awaiting_host` for review.
- **Observability** — append-only local event log, live menu bar status, and a
  read-only web inspector for sessions, runs, console output and file changes.
- **Local by default** — loopback HTTP only, no account, no telemetry; all state
  lives in `~/.relay`.

## Quick Start

Requirements: macOS on Apple silicon, [Codex](https://github.com/openai/codex),
and the DeepSeek CLI (`dsh`) installed and authenticated.

**1. Install and run Relay**

Download `Relay-<version>-arm64.dmg` from
[Releases](https://github.com/Ant1mage/relay/releases), or build one from a
checkout:

```bash
pnpm install
pnpm pack:mac      # → dist/Relay-<version>-arm64.dmg
```

Relay is a menu bar app: launch it once and its icon stays in the menu bar, with
the local daemon running behind it.

**2. Let Relay detect DeepSeek**

```bash
dsh --version
```

Menu bar → **Runtimes** shows the detected DeepSeek runtime with its executable,
version and health. If your `dsh` lives outside the usual locations, register
the executable by hand in the same tab.

**3. Configure a DeepSeek agent**

Menu bar → **Control panel…** (⌘,) → **Agents** → new profile. Pick the DeepSeek
runtime, choose the model and reasoning values the CLI reports, set capabilities
and optional instructions, then enable the profile.

**4. Install the Codex integration**

Control panel → **Codex integration** → **Install**. Relay writes a local plugin
(skill + hooks) and registers the `relay` MCP server with Codex; all five checks
should turn green. If one doesn't, see
[docs/codex-integration.md](docs/codex-integration.md).

**5. Use it inside Codex**

```text
$relay use DeepSeek to review the current implementation and report potential issues.
```

Codex keeps planning and reviewing; Relay runs the worker under policy and
reports back into the session.

## How It Works

```text
Codex  ──  $relay / MCP  ──►  Relay Core  ──►  Runtime adapter  ──►  dsh (DeepSeek CLI)
  ▲                                │                                     │
  └────  result, changes, status ──┘◄────────────────────────────────────┘

Menu bar       ──►  configuration and runtime control
Web inspector  ◄──  execution state, events, logs
```

Codex owns planning and orchestration. Relay owns runtime execution, policy,
lifecycle and observability. Each delegation becomes a Run with Steps and
WorkerSessions, and every observable action is appended to a local event log that
the menu bar and the inspector read from. Native CLIs keep their own agent loop;
Relay never re-implements one.

Full architecture: [docs/architecture.md](docs/architecture.md).

## Supported Runtimes

| Runtime | CLI | Adapter | Status |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `@relay/adapter-deepseek` | **Supported / MVP** |
| Kimi Code | `kimi` | `@relay/adapter-kimi` | Planned |
| Antigravity CLI | `agy` | `@relay/adapter-antigravity` | Planned |
| Grok Build | `grok` | — | Planned |
| Z.ai / GLM | `zai-cli` | `@relay/adapter-zai` | Planned |

Only DeepSeek is verified end to end. The Kimi, Antigravity and Z.ai adapters
detect, compile and run against parser fixtures, but they are not part of the
supported MVP; Grok Build has no adapter yet. Antigravity CLI supersedes Gemini
CLI, so Google is listed once.

## Development

Node.js 24 and pnpm 10.

```bash
pnpm install
pnpm typecheck
pnpm test
pnpm build
```

`pnpm dev` starts a development session (daemon + menu bar); add `--web` for the
inspector's dev server.

## Documentation

- [docs/architecture.md](docs/architecture.md) — processes, ownership
  boundaries, data model, events.
- [docs/codex-integration.md](docs/codex-integration.md) — what Relay installs
  into Codex, session lifecycle, repair and troubleshooting.

## License

Released under the [MIT License](LICENSE).
