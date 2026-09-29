# Relay Architecture

Relay keeps one boundary intact:

> **Codex owns planning and orchestration. Relay owns runtime execution, policy,
> lifecycle and observability. Native CLIs own their internal agent behavior.**

Everything below follows from that split.

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
├─ relay-adapters/    DeepSeek Harness, Kimi, Z.ai, Antigravity, Grok (+ shared CLI plumbing)
├─ relay-storage/     SQLite: event log, host sessions, startup reconciliation
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
| `relayd` | Local daemon on `127.0.0.1` (default port 7352) | **Execution**: RunController, worker processes, event writes, the SQLite schema — plus the configuration file (single writer), the runtime scan, the Codex integration lifecycle, the projection and SSE, static hosting (inspector and control panel) and diagnostics | Render UI, plan or reason |
| `relay-mcp` | stdio MCP server started by Codex | The MCP protocol: tool schema and request/response translation to the daemon's HTTP API | Own a worker, an adapter or a RunController; own configuration |
| `relay-desktop` | Tauri menu bar app | Status, quick actions, panel and inspector windows, daemon start/stop, app updates, launch at login | Touch the database or the configuration file directly, derive state |
| UI (WASM) | Inspector and control panel served by `relayd` | Sessions, runs, console, changes, cancel, configuration forms, diagnostics | Write events, derive state |

The desktop shell holds no execution state, so it can restart at any time and
pages resync from the event log.

Ownership rules that matter in practice:

- **`relayd` is the only process that owns a worker.** It starts the external
  Agent CLI, supervises the process, writes the events and cancels it. Nothing
  else can, which is why no queue, claim, lease or owner routing exists between
  processes: there is only one owner to route to.
- **`relayd` is the only writer of `config.toml`.** The panel edits what the
  daemon serves; the daemon re-reads the file before each delegation.
- **A front-end dying cannot take a worker with it.** `relay-mcp` is replaceable:
  kill the MCP server Codex started, start another, and the same runs are still
  there to query, wait on and cancel. Only the daemon stopping ends its workers,
  and it ends them deliberately on the way out.
- **A daemon that stopped mid-run is reconciled on the way up.** Runs left queued,
  starting or running have `worker/orphaned` (the process is gone) or
  `worker/interrupted` (it outlived the daemon that could read it) appended — never
  a rewritten history.

## 3. Delegation path

```text
Codex session
  │  SessionStart / SessionEnd hooks  →  trusted session identity (id, name, cwd)
  │  $relay skill + MCP tools
  ▼
relay-mcp (stdio)  ── loopback HTTP + token ──▶  relayd
                                                  ├─ AdapterRegistry · RuntimeRegistry · ProfileRegistry
                                                  ├─ PolicyResolver   global → workspace → session
                                                  ├─ RunController    start, supervise, cancel, resume, accept
                                                  ├─ Event store      append-only → ~/.relay/relay.sqlite
                                                  └─ Startup reconciliation
                                                        ▼
                                                  Runtime adapter  →  native CLI process (dsh …)
```

`relayd` projects the same event log for the menu bar, the panel and the
inspector. No surface talks to a worker directly, and every front-end reaches the
same runs through the same API.

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
    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions;
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
tool event keeps the original native payload.

### Assistant text

Runtimes disagree about how they report an answer, so Relay normalizes all of
them onto one contract on `worker/message`:

```json
{ "kind": "delta", "text": "<increment>", "messageStart": true }
```

`messageStart` opens a new assistant message; the increments after it extend that
message. `kind: "final"` stays the authoritative complete answer. Each adapter
parses its own JSON and keeps the provider frame (`nativeEvent`); `relay-core`
only ever sees the normalized contract, and no provider field name crosses that
boundary.

| Runtime | Native source | Mapped as |
| --- | --- | --- |
| DeepSeek Harness | one `--json` `text` frame per **committed message** (no token-level stream) | one delta with `messageStart` per frame |
| Antigravity | `step_update.agent_response.text_delta` | chunks; a new `step_index` opens a message |
| Grok | native `type: "text"` chunks | chunks; a new model response opens a message |
| Kimi, Z.ai | legacy `text` / `final` frames | read through the same contract |

`relay_core::worker_text::assistant_text` is the shared aggregation every surface
consumes: it merges the increments in order, lets an authoritative `final`
supersede the stream that spelled it out, and reports which `worker/completed`
summaries merely repeat a message. The console renders the merged messages as
assistant bubbles — never one row per chunk — and a completion that repeats an
answer is shown as a status instead of the same text twice. Grok copies only
public usage/result fields: signed usage blobs and cached authentication fields
never enter the event log.

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
| `relay.sqlite` | Event log and host sessions. The schema version is reset, not migrated, between releases. |
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
- The schema is versioned with `PRAGMA user_version`. An incompatible database
  is reset rather than migrated; back up `relay.sqlite` before upgrading across
  schema versions.
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
- Launch-at-login, release checks, native update prompts and clipboard access
  are the desktop shell's only privileged operations.
- No accounts, no telemetry, no remote sync: everything stays in `~/.relay`.

## 12. Adding a runtime

1. Implement `AgentAdapter` for the CLI in `crates/relay-adapters` and add it to
   `adapters()`.
2. Declare capabilities honestly — Relay degrades (no `send`, no `resume`)
   instead of pretending.
3. Add parser fixtures for the native event stream; keep the native payload.
4. Map assistant text onto the worker-text contract (section 6) instead of a
   surface-specific shape, and say whether the runtime publishes increments or
   whole committed messages.
5. Ship a profile preset if the runtime deserves one.
6. Only then mark it supported: an adapter that compiles but has no end-to-end
   verification stays "Planned" in the README.
