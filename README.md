# Relay — Local MCP Agent Delegation for OpenAI Codex

[简体中文](README.zh-CN.md) · English

[![CI](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Ant1mage/codex-agent-relay?label=release)](https://github.com/Ant1mage/codex-agent-relay/releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex chooses what to delegate. Relay runs and supervises the external agent.**

Relay is a local MCP agent runtime and desktop control plane for delegating
coding, research, and review tasks from OpenAI Codex to agent CLIs such as
DeepSeek Harness. It manages agent profiles, runtime discovery, permissions,
worker processes, and results while keeping planning and orchestration in Codex.

Relay is not an AI IDE or a second agent loop. It connects Codex to the agent
runtimes already available on your machine.

## What Relay does

- Adds a `$relay` skill and MCP tools to Codex for bounded task delegation.
- Runs external agent CLIs as supervised local processes.
- Stores model, reasoning, and capability settings in reusable agent profiles.
- Applies workspace and session policies for read-only, propose, and write tasks.
- Tracks runs, worker output, and file changes in a local inspector and control panel.

## Status

The current release is **v0.1.1**. The verified end-to-end setup is
**macOS Apple silicon + Codex + DeepSeek Harness**.
Relay itself is written in Rust, with a Tauri menu bar app and a local daemon.

| Agent runtime | CLI | Adapter | Status |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | Supported; end-to-end verified |
| Kimi Code | `kimi` | `kimi-code` | Experimental; not end-to-end verified |
| Antigravity CLI | `agy` | `antigravity-cli` | Experimental; not end-to-end verified |
| Z.ai / GLM | `zai-cli` | `zai-cli` | Experimental; not end-to-end verified |

## Quick start

1. Install and sign in to Codex and the agent CLI you want to run.
2. Launch Relay. In **Runtimes**, check that Relay detects your CLI; register its
   executable manually if needed.
3. In **Agents**, create a profile and choose its runtime, model, reasoning level,
   and capabilities.
4. In **Codex**, install the Relay plugin and MCP integration.
5. Ask Codex to delegate a bounded task:

   ```text
   $relay Use DeepSeek to review the current implementation and report potential issues.
   ```

Codex reads the worker result from Relay for review, decides whether more work is
needed, and writes the final response.

## How it works

```text
Codex + $relay skill
        │ MCP
        ▼
   relay-mcp ── local HTTP ──► relayd ──► runtime adapter ──► agent CLI
                                  │                            │
                                  └──── run events/results ◄───┘
```

Codex owns task planning, delegation, and result review. Relay owns runtime
execution, policy, process lifecycle, and local observability. Agent CLIs retain
their own model calls and internal agent behavior.

## Build and test

Relay currently targets macOS on Apple silicon. Building from source requires
stable Rust, the `wasm32-unknown-unknown` target, Trunk, and the Tauri CLI.

```bash
cargo build --workspace
cargo test --workspace
./scripts/dev.sh
```

See [Development](docs/development.md) for desktop builds, packaging, and release
requirements.

## Documentation

- [Architecture](docs/architecture.md) — components, delegation flow, policy, and security.
- [Codex integration](docs/codex-integration.md) — plugin, MCP tools, setup, and troubleshooting.
- [Development](docs/development.md) — build, test, run, and package Relay.

## License

MIT. See [LICENSE](LICENSE).
