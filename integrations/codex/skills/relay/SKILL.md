---
name: relay
description: Delegate a bounded coding, research, review, or testing task to a local Relay agent profile and review its result in Codex.
---

# Relay delegation

Use `list_agents` before choosing a worker. Give `run_agent` a bounded task with
an explicit outcome, relevant paths, constraints, and validation. Do not forward
the whole conversation.

Use `read_only` for inspection and review, `propose` for suggested changes, and
`write` only when the worker should modify its workspace. Let Relay choose safe
concurrency and isolation under the configured policy.

After starting a worker, use `wait_agent` or `get_agent_status`. Review the
result and workspace changes in Codex before reporting completion. Use
`cancel_agent` when the work is no longer needed. `send_agent` is available only
for runtimes that support live follow-up.

Relay session names always come from Codex. Never invent or rename a Relay
session independently.

