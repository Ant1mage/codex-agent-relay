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

## 2. Processes and ownership

| Process | Role | Owns | Must not |
| --- | --- | --- | --- |
| `relay-mcp` | stdio MCP server started by Codex | Execution: RunController, worker processes, event writes, the SQLite schema | Render UI, own configuration (it re-reads config on every call) |
| `relayd` | Local daemon on `127.0.0.1` (default port 7352) | Configuration files (single writer), runtime scan, Codex integration lifecycle, read-only projection and SSE, control queue, static hosting (inspector and control panel), diagnostics | Create schema, start or retry workers, parse native stdout |
| `menu-bar` | macOS menu bar app (Electron tray + panel window) | Status, quick actions, configuration forms, opening the inspector, daemon start/stop, app updates | Touch the database or configuration files directly, derive state |
| `web` | Inspector page served by `relayd` | Sessions, runs, console, changes, cancel, copy diagnostics | Write configuration or events |

The daemon and the menu bar hold no execution state, so either can restart at any
time: running delegations continue inside `relay-mcp`, and pages resync from the
event log.

Ownership rules that matter in practice:

- **The MCP process is the only writer of runs, workers and events, and the only
  owner of the schema.** `relayd` reads; a `CREATE TABLE IF NOT EXISTS` on the
  daemon side would break the version-gated migration in `@relay/core`. Until the
  MCP has opened the store, the daemon reports an empty projection and answers
  cancellation with `accepted: false`.
- **`relayd` is the only writer of configuration.** The panel edits what the
  daemon serves; the MCP re-reads the files on every `list_agents` / `run_agent`.
- **Cancellation crosses processes through a control queue in SQLite.** The daemon
  appends a command, the MCP claims it within ~250 ms and applies it to the worker
  process it owns.
- **Workers are children of the MCP process.** A worker that outlives its owner is
  projected as `orphaned` instead of appearing to run forever.

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

`relayd` projects the same event log for the menu bar and the inspector. Neither
surface talks to a worker directly.

## 4. Host adapter (Codex)

Relay attaches to Codex through two channels, and keeps them separate:

- **Hooks supply identity.** A `SessionStart` hook calls `sync_session` with the
  Codex thread id; Relay resolves the real thread name and cwd from the local
  Codex app-server and registers a HostSession. `SessionEnd` ends it. Relay never
  invents a session name.
- **MCP supplies control.** The tool surface stays generic and stable —
  `list_agents`, `run_agent`, `get_agent_status`, `wait_agent`, `send_agent`,
  `cancel_agent`, `accept_agent`, `resume_agent` — instead of one tool per
  profile, so adding a runtime never changes what Codex sees.

Worker completion does not complete a task: a finished worker moves its Run to
`awaiting_host`, and only `accept_agent` after a Codex review closes it.

See [codex-integration.md](codex-integration.md) for installation, checks and the
full lifecycle.

## 5. Relay Core

| Piece | Responsibility |
| --- | --- |
| `AdapterRegistry` | Registered runtime adapters; unregistering disposes them |
| `RuntimeRegistry` | Runtimes found on this machine plus hand-registered ones |
| `ProfileRegistry` | The Agent Profiles exposed to Codex; enabled flag is enforced server-side |
| `PolicyResolver` | Resolves the effective policy for a workspace / session and rejects requests the policy forbids |
| `RunController` | Starts workers, enforces concurrency and isolation, maps adapter events into the event log, handles cancel / resume / accept |
| `projectRun` | Pure projection: events in, current Run / Step / WorkerSession state out |
| Event store | `SqliteEventStore` (durable) and `MemoryEventStore` (tests); both append-only |

Core contains no provider branches: everything runtime-specific lives behind the
adapter contract in section 6.

## 6. Runtime adapters

An adapter is the only place that knows a specific CLI:

```ts
interface AgentAdapter {
  detect(): Promise<DetectionResult>
  capabilities(): AdapterCapabilities
  start(input: StartInput): Promise<WorkerSessionHandle>
  send?(sessionId: string, message: string): Promise<void>
  cancel(sessionId: string): Promise<void>
  resume?(sessionId: string, input: ResumeInput): Promise<WorkerSessionHandle>
}
```

An adapter discovers the executable and its version, reports capabilities
(`nonInteractive`, structured stream, `resume`, `send`, `cancel`,
`childSessions`), turns a normalized StartInput into native arguments and stdin,
normalizes native events into `RelayEvent`, and keeps the native session id. It
does not choose profiles, decide permissions, decompose tasks or retry them.

Capabilities are declared, not assumed: Relay only offers `send_agent` or
`resume_agent` when the adapter reports that the CLI supports it, and every
mapped event keeps the original native payload.

**Runtime ≠ Agent Profile.** A runtime is a CLI on this machine; a profile is a
user-facing capability on top of it. One runtime can back several profiles with
different capabilities (`deepseek-code`, `deepseek-research`).

## 7. Configuration and policy

Configuration is a file contract shared by both processes
(`@relay/config`), stored under `~/.relay` (override with `RELAY_HOME`):

| File | Contents |
| --- | --- |
| `profiles.json` | Agent Profiles: runtime, description, instructions, capabilities, enabled |
| `settings.json` | Global policy plus per-workspace overrides |
| `runtimes.json` | Runtimes registered by hand, merged with detected ones |
| `relay.sqlite` | Event log and control queue |

Writes are atomic (temp file + rename). Unparseable files are reported as
warnings and never silently overwritten — Relay keeps running on defaults, and
the panel says so.

Policy resolves global → workspace → session, most specific wins. It covers
access mode (`read_only` / `propose` / `write`), write / command / network
permission, concurrency caps for runs and writers, and the worktree requirement
for parallel writers. Policy is enforced before an adapter starts anything.

## 8. Events and storage

The event log is the single source of truth. Current state, history, the
inspector and the menu bar are all projections of it — nothing keeps a second
copy of "progress" or "last message".

- Append-only rows in `relay_events`, ordered per Run by `seq`.
- Each row keeps the normalized event and, when available, the original native
  event.
- The schema is created by version-gated migrations (`PRAGMA user_version`) that
  run in the MCP process only.
- SQLite is accessed through Node's built-in driver (`node:sqlite`), so the
  bundled MCP server needs no native addon.
- The daemon pushes projections over SSE with a per-run sequence cursor; clients
  deduplicate by `seq`, so "connect first, backfill history" neither duplicates
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
recorded as native metadata, not promoted into Relay's own hierarchy.

## 10. Surfaces

- **Menu bar** — configuration and runtime control: status, active delegations
  with cancel, profiles, policy, runtimes, Codex integration, app updates.
- **Web inspector** — observation: sessions, runs, console of observable actions,
  file changes, raw events, cancellation.

Both read the same projection; neither is required for a delegation to run.

## 11. Security boundaries

- The daemon binds `127.0.0.1` only and never exposes a remote interface.
- Requests must present a per-start random token; the host must be loopback and
  a request's `Origin`, when present, must match (DNS-rebinding and
  cross-site-read protection).
- `server.json` (pid, port, token, nonce, 0600) identifies a running daemon;
  liveness is verified by nonce, never by PID. Restarts only ever signal a
  verified daemon.
- The DeepSeek adapter writes the delegated task to the worker's stdin, so it
  does not appear in the process list.
- No accounts, no telemetry, no remote sync: everything stays in `~/.relay`.

## 12. Adding a runtime

1. Implement `AgentAdapter` for the CLI and register it in the runtime
   environment used by both the daemon scan and the MCP process.
2. Declare capabilities honestly — Relay degrades (no `send`, no `resume`) instead
   of pretending.
3. Add parser fixtures for the native event stream; keep the native payload.
4. Ship default profiles if the runtime deserves them.
5. Only then mark it supported: an adapter that compiles but has no end-to-end
   verification stays "Planned" in the README.
