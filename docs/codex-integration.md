# Codex Integration

Relay enters Codex through a **local plugin marketplace plus an MCP server**.
Installing is a lifecycle with checks, not a one-time file copy: the same code
path handles install, repair, update and removal, and every step is idempotent
and reported individually.

The MCP entry points at Relay's own Rust binary. There is no Node entry point, no
`process.execPath`, no `ELECTRON_RUN_AS_NODE` and no Electron runtime to reach.

## 1. What gets installed

| Component | What it is | Where it goes |
| --- | --- | --- |
| **MCP server** | `relay-mcp` (agent control + session sync) | `[mcp_servers.relay]` in `~/.codex/config.toml` |
| **Plugin** | `relay@relay`: skill + hooks + metadata | `~/.relay/codex-plugin` (a local marketplace Relay generates) |
| **Skill** | `skills/relay/SKILL.md`, which teaches Codex to compress context into a bounded task | ships with the plugin |
| **Hooks** | `hooks/hooks.json`: `SessionStart` → `sync_session`, `SessionEnd` → `relay-mcp --session-end-hook` | ships with the plugin (Codex asks you to trust hooks once) |

In a packaged Relay the entry is the bundled binary at
`Relay.app/Contents/Resources/relay-mcp`. In a source checkout it is the binary in
`target/debug` or `target/release`, resolved as the sibling of the running
process. The daemon remembers which entry it expects: if the configured entry
differs or the file is gone, the MCP check reports **stale** instead of a vague
"configured".

## 2. Checks

Five checks describe the state of the integration:

```text
✓ Codex detected        the codex CLI, or the copy shipped with the IDE extension
✓ Relay MCP             expected entry matches ~/.codex/config.toml and the file exists
✓ Relay Skill           installed content matches the current Relay version
✓ Relay Plugin          marketplace registered, plugin installed and enabled
✓ Relay Hooks           hooks.json present and matching the current version
```

Each check carries a `status` and a `hint`:

| Status | Meaning | Typical action |
| --- | --- | --- |
| `ok` | Correct and current | — |
| `missing` | Not installed | Install |
| `stale` | Points at an old path or an entry that no longer exists | Repair |
| `outdated` | Installed content differs from this Relay version | Update |
| `legacy` | Left over from a manually copied skill | Repair (removes it) |

The plugin version is the Relay version plus a hash of the installed skill and
hooks, so an edited skill shows up as `outdated` rather than silently staying
behind.

## 3. Actions

| Action | What happens |
| --- | --- |
| **Install** | Generate the plugin tree → `codex plugin marketplace add` → `codex plugin add relay@relay` → `codex mcp add relay -- <relay-mcp>` → remove an old manually copied skill |
| **Repair** | The install path, forced: rewrites the plugin tree and the MCP entry. Use it for `stale` and `legacy` |
| **Update** | Regenerates the plugin at the current version and reinstalls it, so Codex picks up skill and hook changes |
| **Remove from Codex** | `codex plugin remove` → `codex plugin marketplace remove` → `codex mcp remove` → delete `~/.relay/codex-plugin` and the legacy skill copy |

Removal is complete: Relay never leaves an MCP entry, skill or hook behind in
Codex.

## 4. Session lifecycle

- **Start** — the `SessionStart` hook calls `sync_session` with the Codex thread
  id. Relay queries the local Codex app-server (`thread/read`) for the real
  thread name and cwd and registers a HostSession. Relay never invents a session
  name or alias.
- **Fallback** — `sync_session` also runs on the first MCP tool call, so
  delegation works even before hooks have been trusted.
- **End** — the `SessionEnd` hook runs `relay-mcp --session-end-hook` in one-shot
  cleanup mode: the session is marked ended without depending on a live stdio MCP
  connection, and its session-scoped policy is cleared.
- **Resolution** — if `codex` is not on `PATH` (common with the IDE extension),
  Relay walks the known install locations before giving up.

## 5. MCP tool surface

| Tool | Purpose |
| --- | --- |
| `list_agents` | Enabled profiles available to this session |
| `run_agent` | Start one bounded task; access mode and isolation are optional |
| `get_agent_status` / `wait_agent` | Read or await the projected worker state |
| `send_agent` | Follow up on a live worker — only for runtimes that support it |
| `cancel_agent` | Cancel the worker process |
| `accept_agent` / `resume_agent` | Close a Run after review, or continue the same Step with feedback |
| `sync_session` / `end_session` | Session registration and teardown (used by hooks) |

`wait_agent` returns the projection, including `result` — the data carried by the
terminal worker event — which is what Codex reviews before calling
`accept_agent`.

The surface is generic — one tool set for every profile — so adding a runtime
never changes what Codex sees.

## 6. Configuration propagation

The MCP process is long-lived, so configuration changes do not require restarting
Codex:

- `list_agents` and `run_agent` re-read `config.toml` and sync the differences
  into the profile registry.
- Policy is resolved per workspace before every run.
- A change written by the daemon takes effect on the next MCP call, and the menu
  bar and inspector receive the new projection over SSE.
- One exception: after an **app update**, an MCP process that is already running
  keeps the old code. Restart the Codex session to pick up the new build.

## 7. Where things live

```text
integrations/codex/                    sources in this repository
├── plugin.json                        plugin metadata
├── hooks/hooks.json                   SessionStart / SessionEnd
└── skills/relay/SKILL.md              the skill Codex reads

~/.relay/codex-plugin/                 generated local marketplace
├── .agents/plugins/marketplace.json
└── plugins/relay/
    ├── .codex-plugin/plugin.json      version = Relay version + content hash
    ├── hooks/hooks.json
    └── skills/relay/SKILL.md

~/.codex/config.toml                   [mcp_servers.relay]
~/.codex/skills/relay/                 legacy manually copied skill (removed by install/repair)
```

## 8. Troubleshooting

| Symptom | Check | Action |
| --- | --- | --- |
| `$relay` does nothing in Codex | Plugin check | Install or repair; confirm hooks are trusted |
| The MCP check is `stale` | Expected entry vs `config.toml` | Repair — the app moved or the bundle path changed |
| Skill or hooks are `outdated` | Content hash | Update |
| `legacy` skill reported | `~/.codex/skills/relay` still exists | Repair removes it |
| Sessions appear without names | Codex CLI not resolvable | Make sure `codex` runs in a terminal, then repair |
| Delegation still uses old code after an app update | Long-lived MCP process | Restart the Codex session |
| Nothing works and you want a clean slate | — | Remove from Codex, then install again |

Useful inspections:

```bash
codex plugin list                 # is relay@relay installed and enabled?
codex plugin marketplace list     # which root is the relay marketplace?
grep -A3 '\[mcp_servers.relay\]' ~/.codex/config.toml
```

The control panel's Codex tab shows the same five checks with their status and
hint, and the Status tab exports a diagnostics report for bug reports.
