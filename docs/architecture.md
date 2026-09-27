# Relay Architecture

Relay keeps one boundary intact:

> **Codex owns planning and orchestration. Relay owns runtime execution, policy,
> lifecycle and observability. Native CLIs own their internal agent behavior.**

Everything below follows from that split. This document describes the stable
architecture — not a plan, and not a change history.

## 1. Responsibilities

| Layer | Owns | Does not do |
| --- | --- | --- |
| Codex (host) | Planning, profile choice, task decomposition, result review, retries | Process control, policy, event storage |
| Relay | Runtime discovery, profile configuration, policy, worker lifecycle, session binding, event log, projections | Reasoning, prompt assembly, workflow orchestration, acceptance |
| Native CLI (runtime) | Its own agent loop, model calls, internal child agents | Cross-provider contracts |

"Subagent" therefore means three different things at three layers: a capability
for Codex, a Run for Relay, possibly several internal children for the CLI. Relay
does not merge these levels.

## 2. Crates and processes

```text
crates/
├─ relay-core/        domain, events, projection, policy, run lifecycle, adapter trait
├─ relay-adapters/    DeepSeek Harness, Kimi, Z.ai, Antigravity (+ shared CLI plumbing)
├─ relay-storage/     SQLite: event log, host sessions, control queue
├─ relay-config/      config.toml: agent profiles, policy, manual runtimes
├─ relay-api/         wire contract + the daemon's HTTP/SSE surface + projections
└─ relay-codex/       Codex thread identity and the plugin/MCP integration lifecycle

apps/
├─ relayd/            the daemon (composition root)
├─ relay-mcp/         the MCP server Codex starts over stdio
└─ relay-desktop/     Tauri 2 shell (src-tauri) + Leptos UI (ui)
```

`relay-core` is the stable part. It understands Relay's own domain only —
Runtime, AgentProfile, Run, Step, WorkerSession, Capability, Policy, AccessMode,
Isolation, RelayEvent, RunProjection — and knows nothing about Tauri, the Codex
plugin layout, MCP tool names, SQL, DeepSeek flags or HTTP routes. Changing any
of those does not change Core.

| Process | Role | Owns | Must not |
| --- | --- | --- | --- |
| `relay-mcp` | stdio MCP server started by Codex | Execution: RunController, worker processes, event writes, the SQLite schema | Render UI, own configuration (it re-reads the config file on every call) |
| `relayd` | Local daemon on `127.0.0.1` (default port 7352) | Configuration file (single writer), runtime scan, Codex integration lifecycle, read-only projection and SSE, control queue, static hosting (inspector and control panel), diagnostics | Create schema, start or retry workers, parse native stdout |
| `relay-desktop` | Tauri menu bar app | Status, quick actions, panel and inspector windows, daemon start/stop, app updates, launch at login | Touch the database or the configuration file directly, derive state |
| UI (WASM) | Inspector and control panel served by `relayd` | Sessions, runs, console, changes, cancel, configuration forms, diagnostics | Write events, derive state |

The daemon and the desktop shell hold no execution state, so either can restart
at any time: running delegations continue inside `relay-mcp`, and pages resync
from the event log.

Ownership rules that matter in practice:

- **The MCP process is the only writer of runs, workers and events.** `relayd`
  reads; it never creates a run.
- **`relayd` is the only writer of `config.toml`.** The panel edits what the
  daemon serves; the MCP process re-reads the file on every `list_agents` /
  `run_agent`.
- **Cancellation crosses processes through a queue in SQLite.** The daemon
  appends a `cancel-worker` command, the MCP process claims it within ~250 ms and
  applies it to the worker process it owns.

## 3. Delegation path

```text
Codex session
  │  SessionStart / SessionEnd hooks  →  trusted session identity (id, name, cwd)
  │  $relay skill + MCP tools
  ▼
Relay Core (inside relay-mcp)
  ├─ AdapterRegistry · RuntimeRegistry · ProfileRegistry
  ├─ PolicyResolver        global → workspace → session, most specific wins
  ├─ RunController         start, supervise, cancel, resume, accept
  └─ Event store           append-only → ~/.relay/relay.sqlite
  ▼
Runtime adapter  →  native CLI process (dsh …)
```

`relayd` projects the same event log for the menu bar, the panel and the
inspector. Neither surface talks to a worker directly.

## 4. Host adapter (Codex)

Relay attaches to Codex through two channels, and keeps them separate:

- **Hooks supply identity.** A `SessionStart` hook calls `sync_session` with the
  Codex thread id; Relay resolves the real thread name and cwd from the local
  Codex app-server and registers a HostSession. `SessionEnd` runs the MCP binary
  in one-shot cleanup mode. Relay never invents a session name.
- **MCP supplies control.** The tool surface stays generic and stable —
  `list_agents`, `run_agent`, `get_agent_status`, `wait_agent`, `send_agent`,
  `cancel_agent`, `accept_agent`, `resume_agent`, `sync_session`, `end_session` —
  so adding a runtime never changes what Codex sees.

Worker completion does not complete a task: a finished worker moves its Run to
`awaiting_host`, and only `accept_agent` after a Codex review closes it.

## 5. Relay Core

| Piece | Responsibility |
| --- | --- |
| `AdapterRegistry` | Registered runtime adapters |
| `RuntimeRegistry` | Runtimes found on this machine plus hand-registered ones; `sync` replaces the set without disturbing active workers |
| `ProfileRegistry` | The Agent Profiles exposed to Codex; the enabled flag is enforced server-side |
| `PolicyResolver` | Resolves the effective policy for a workspace / session and rejects requests the policy forbids |
| `RunController` | Starts workers, enforces concurrency and isolation, maps adapter events into the event log, handles cancel / resume / accept |
| `project_run` | Pure projection: events in, current Run / Step / WorkerSession state out |
| `EventStore` | `SqliteEventStore` (durable) and `MemoryEventStore` (tests); both append-only |

Core contains no provider branches: everything runtime-specific lives behind the
adapter trait in section 6.

## 6. Runtime adapters

```rust
#[async_trait]
trait AgentAdapter {
    fn id(&self) -> &str;
    fn capabilities(&self) -> AdapterCapabilities;
    async fn detect(&self) -> DetectionResult;
    async fn report_options(&self, runtime_id: &str) -> RuntimeOptions;
    async fn start(&self, input: StartInput) -> Result<WorkerHandle>;
    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle>;
    async fn send(&self, native_session_id: &str, message: &str) -> Result<()>;
    async fn cancel(&self, native_session_id: &str) -> Result<()>;
    async fn dispose(&self);
}
```

An adapter discovers the executable and its version, reports capabilities
(`nonInteractive`, structured stream, `resume`, `send`, `cancel`, `childSessions`,
`modelSelection`), turns a normalized `StartInput` into native arguments and
stdin, normalizes native events into `AdapterEvent`s, and keeps the native
session id. It does not choose profiles, decide permissions, decompose tasks or
retry them.

Capabilities are declared, not assumed: Relay only offers `send_agent` or
`resume_agent` when the adapter reports that the CLI supports it, and every
mapped event keeps the original native payload.

**Runtime ≠ Agent Profile.** A runtime is a CLI on this machine; a profile is a
user-facing capability on top of it. One runtime can back several profiles.
Detection only *finds* runtimes — it never creates a profile. The control panel
offers coding and research presets as templates the user applies.

## 7. Configuration and policy

Configuration is a TOML file shared by both processes, stored under `~/.relay`
(override with `RELAY_HOME`):

| Path | Contents |
| --- | --- |
| `config.toml` | Agent Profiles, global policy, per-workspace overrides, hand-registered runtimes |
| `relay.sqlite` | Event log, host sessions, control queue |
| `server.json` | How to reach a running daemon (pid, port, token, nonce), mode 0600 |

Writes are atomic (temp file + rename). A file that fails to parse is reported as
a warning and never silently overwritten — Relay keeps running on defaults, and
the panel says so.

Policy resolves global → workspace → session, most specific wins. It covers
access mode (`read_only` / `propose` / `write`), write / command / network
permission, concurrency caps for runs and writers, and the worktree requirement
for parallel writers. Policy is enforced before an adapter starts anything.

## 8. Events and storage

The event log is the single source of truth. Current state, history, the
inspector and the menu bar are all projections of it — nothing keeps a second
copy of "progress" or "last message".

- Append-only rows in `relay_events`, ordered per Run by `seq`; the store rejects
  a gap.
- Each row keeps the normalized event and, when available, the original native
  event (bounded to 128 KiB).
- The schema is created by a version gate in `PRAGMA user_version`. Version 1 is
  the Rust baseline; a database written by an earlier implementation is reset
  rather than migrated, because Relay's old development data is not something a
  user keeps.
- The daemon pushes projections over SSE with a per-run sequence cursor; clients
  de-duplicate by `seq`, so "connect first, backfill history" neither duplicates
  nor drops events.

## 9. Data model

```text
HostSession          one Codex session (thread), never a directory
└── Run              one delegation lifecycle; ends at awaiting_host → completed
    └── Step         one top-level Codex → Relay delegation; retries bump iteration
        └── WorkerSession   one process/conversation in a native runtime
            └── Event       append-only observable fact
```

`HostSession` ids are derived from the host (`codex:<thread-id>`) and its display
name always comes from Codex. Child agents created inside a native runtime are
recorded as native metadata (`child/started`, `child/completed`), not promoted
into Relay's own hierarchy.

## 10. Surfaces

- **Menu bar** — configuration and runtime control, active delegations with
  cancel, profiles, policy, runtimes, Codex integration, app updates.
- **Inspector** — observation: sessions, runs, console of observable actions,
  file changes, raw events, cancellation.
- **Control panel** — configuration: agents, runtimes, policy, Codex
  integration, status and diagnostics.

Both windows load the daemon's own URLs over HTTP, so the same pages work in a
browser, and the UI never touches SQLite.

## 11. Security boundaries

- The daemon binds `127.0.0.1` only and never exposes a remote interface.
- Requests must present a per-start random token; the `Host` must be loopback and
  a request's `Origin`, when present, must match (DNS-rebinding and
  cross-site-read protection).
- `server.json` (pid, port, token, nonce, 0600) identifies a running daemon;
  liveness is verified by nonce, never by PID. Restarts only ever signal a
  verified daemon.
- Launch-at-login, the updater and clipboard access are the desktop shell's only
  privileged operations.
- No accounts, no telemetry, no remote sync: everything stays in `~/.relay`.

## 12. Adding a runtime

1. Implement `AgentAdapter` for the CLI in `crates/relay-adapters` and add it to
   `adapters()`.
2. Declare capabilities honestly — Relay degrades (no `send`, no `resume`)
   instead of pretending.
3. Add parser fixtures for the native event stream; keep the native payload.
4. Ship a profile preset if the runtime deserves one.
5. Only then mark it supported: an adapter that compiles but has no end-to-end
   verification stays "Planned" in the README.
