# Relay — Local MCP Agent Delegation for OpenAI Codex

[简体中文](README.zh-CN.md) · English

[![CI](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Ant1mage/codex-agent-relay?label=release)](https://github.com/Ant1mage/codex-agent-relay/releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex chooses what to delegate. Relay runs and supervises the external agent.**

Relay is a local MCP agent runtime and desktop control plane for delegating
coding, research, and review tasks from OpenAI Codex to supported agent CLIs:
DeepSeek Harness, Grok Build, and Antigravity CLI. It manages agent profiles,
runtime discovery, permissions, worker processes, and results while keeping
planning and orchestration in Codex.

Relay is not an AI IDE or a second agent loop. It connects Codex to the agent
runtimes already available on your machine.

## What Relay does

- Adds a `$relay` skill and MCP tools to Codex for bounded task delegation.
- Runs external agent CLIs as supervised local processes.
- Gives every runtime the same Codex-facing tools for starting, waiting on,
  reviewing, resuming, and cancelling tasks where the runtime supports them.
- Stores runtime, model, reasoning, and capability choices in reusable agent
  profiles.
- Applies global, workspace, and session policies for access, command and network
  permissions, and concurrency.
- Streams worker updates into one session timeline and keeps separate tasks and
  their history visible instead of replacing earlier runs.
- Shows final responses, observable tool activity, file changes, and raw events
  in the local inspector; the menu bar app manages profiles, runtimes, policy,
  and Codex integration.

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
| Antigravity CLI | `agy` | `antigravity-cli` | Supported; file writes and allowlisted commands verified |
| Grok Build | `grok` | `grok-cli` | Supported; end-to-end verified |

Relay reuses the supported CLI's existing login and model catalogue where the
runtime exposes them. Profiles select the CLI and its supported model and
reasoning options; Relay does not replace the provider's own agent loop.

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

1. Download the Apple silicon DMG from the [latest GitHub Release](https://github.com/Ant1mage/codex-agent-relay/releases/latest),
   move Relay to Applications, and launch it. The app is ad-hoc signed and not
   notarized; macOS may ask you to approve it on first launch.
2. Install and sign in to Codex and the agent CLI you plan to use.
3. In Relay's **Runtimes** page, confirm the CLI is detected. In **Agents**, make
   a profile and choose its runtime, model, reasoning level, and capabilities.
4. In **Codex** inside Relay, install the plugin and MCP integration. The status
   checks show whether Codex, MCP, the skill, plugin, and hooks are current.
5. Ask Codex to delegate a bounded task:

   ```text
   $relay Use DeepSeek to review the current implementation and report potential issues.
   ```

Codex reads the worker result from Relay for review, decides whether more work is
needed, and writes the final response.

### A typical delegation

```text
You ── task request ──► Codex
                          │ $relay: bounded task + profile
                          ▼
                       Relay ──► local agent CLI
                          ▲             │
                          └── progress, tools, final result
                          │
                     Codex reviews result
                          │
                          ▼
                    final response to you
```

Each delegation is recorded as its own run under the originating Codex session.
The inspector lets you follow progress, review the answer and file changes, and
see why a task succeeded, failed, or was denied. Codex remains responsible for
deciding whether to accept the result or delegate follow-up work.

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
cargo clippy --workspace --all-targets -- -D warnings
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

## Local data and security

Relay keeps its configuration, event history, and Codex integration under
`~/.relay` by default. The daemon listens on loopback only and protects its local
API with a per-start token. Runtime permission enforcement depends on each CLI's
capabilities: Relay refuses access modes it cannot enforce instead of claiming
that an unsupported sandbox is active. For example, Antigravity Write runs
approve file edits with `--mode accept-edits`, while shell commands continue to
follow Antigravity's own permission rules.

## License

MIT. See [LICENSE](LICENSE).
