# Codex Integration

Relay treats the Codex thread as the authoritative HostSession. Its stable
identity is `codex:<thread-id>`, while its visible name is read from Codex and
stored as `displayName` with `nameSource: 'codex'`. Relay has no independent
session alias field.

The Codex hook provides the trusted native session ID at session start and end.
Before listing or running an agent, Relay reads the matching thread through the
local Codex app-server `thread/read` method. It uses `thread.name` exactly; when
Codex has not assigned an explicit name, it uses the same first-prompt preview
available on the thread. Re-syncing on MCP calls picks up later renames.

The stdio MCP surface is:

- `list_agents`
- `run_agent`
- `get_agent_status`
- `wait_agent`
- `send_agent`
- `cancel_agent`

`sync_session` and `end_session` are lifecycle tools used by the bundled hooks.
Host/session identity is taken from Codex-provided process or request metadata,
not from model-authored `run_agent` arguments.

The source-checkout plugin starts the bridge through the repository's pnpm
workspace. Release packaging replaces that development command with the bundled
executable.
