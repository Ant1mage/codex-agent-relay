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

The current release is **v0.1.3**. End-to-end delegation is verified on
**macOS Apple silicon + Codex**, using **DeepSeek Harness**, **Grok Build**, or
**Antigravity CLI** — Antigravity file writes, allowlisted command execution,
and the session `run`/`resume`/`cancel`/`accept` flow have passed end-to-end
checks (see its scope note below).
Relay itself is written in Rust, with a Tauri menu bar app and a local daemon.

Antigravity CLI support and the unified desktop UI are included in **v0.1.3**.

| Agent runtime | CLI | Adapter | Status |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | Supported; end-to-end verified |
| Kimi Code | `kimi` | `kimi-code` | Experimental; not end-to-end verified |
| Antigravity CLI | `agy` | `antigravity-cli` | Supported; file writes and allowlisted commands verified |
| Z.ai / GLM | `zai-cli` | `zai-cli` | Experimental; not end-to-end verified |
| Grok Build | `grok` | `grok-cli` | Supported; end-to-end verified |

Grok reuses the CLI's existing login. Model and reasoning choices come from
`grok models` and its matching native model metadata. On macOS, Relay also
passes enabled system proxies to Grok when no proxy environment override is
present. Read-only/propose runs use Grok's read-only sandbox; write runs use
its workspace sandbox. Child proxy exclusions always include loopback addresses;
Relay's own local connections stay direct, while update checks use system proxies.
Resume and cancellation are supported; live message injection and child agents
are not offered. See the
[Grok validation report](docs/reports/grok-cli-2026-09-28.md) for the tested scope.

Antigravity CLI (`agy`) is driven through its native headless stream: the task is
written as one private `user` event on stdin, and the CLI's `init`, `step_update`
and `result` NDJSON frames become Relay events. `agent_response.text_delta`
chunks stream through Relay's unified worker-text contract — one assistant
message per `agent_response` step, with the `result` frame as the authoritative
answer — so they display through the same path as DeepSeek Harness and Grok.
Model ids are parsed from the `agy models` `<id>\t<label>` table
and applied with the CLI's own `--model` flag; reasoning levels are exactly the
`--effort` values the CLI's own help enumerates (`low|medium|high|max`) — Relay
never infers them from model id suffixes. Relay waits for the CLI's `init` and
validates the conversation UUID before writing the task to stdin: a missing or
malformed UUID, an unknown resume id, or a handshake timeout fails, closes stdin
without delivering the task, and reclaims the child. A conversation id that
changes after the task was delivered only fails that round; it cannot retract
what was already sent or executed. Continuing a run uses `--conversation` with
the native UUID. Tool steps expose their real file path (the CLI publishes
`AbsolutePath` and `TargetFile`, normalized to `path` for the Changes view) and
their command (the CLI publishes `CommandLine`, normalized to `command`).
Cancellation is supported. A live Codex session has driven the full native
`list`/`run`/`wait`/`resume`/`accept`/`cancel` flow through Relay: a resumed run
stayed on the same native conversation, an invalid model failed instead of
reporting success, and cancellation reaped the native process.

**Permission and validation scope.** Native headless runs inherit the user's
existing Antigravity permission settings. Relay uses `--mode accept-edits` for
Write runs, which enables file-edit confirmation without granting shell or
network permissions. It does not pass `--dangerously-skip-permissions` or
auto-approve denied tools. File creation through Relay and execution of a command
already present in the host's exact allowlist both passed end-to-end; an
unallowlisted `pwd` was denied as expected. A headless permission denial may
still leave the CLI process with exit code 0, so Relay treats a denied tool plus
an empty result as failure. Read-only and propose runs are refused because
headless Antigravity cannot enforce a read-only workspace, so only write runs
are offered. Live message injection is not offered; child-agent step updates are
mapped when the CLI reports them, but the child-agent lifecycle has not been
verified end to end. See the
[Antigravity validation report](docs/reports/antigravity-cli-2026-09-28.md) for
the tested scope and remaining limitations.

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

Package one app profile per invocation:

```bash
./scripts/package-app.sh --mode dev       # debug app: Relay Dev.app
./scripts/package-app.sh --mode release   # release app and DMG
```

See [Development](docs/development.md) for desktop builds, packaging, and release
requirements.

## Documentation

- [Architecture](docs/architecture.md) — components, delegation flow, policy, and security.
- [Codex integration](docs/codex-integration.md) — plugin, MCP tools, setup, and troubleshooting.
- [Development](docs/development.md) — build, test, run, and package Relay.

## License

MIT. See [LICENSE](LICENSE).
