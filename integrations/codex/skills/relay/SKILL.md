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
concurrency under the configured policy.

Relay cannot currently provide `worktree` isolation. If `run_agent` returns
`status: "not_dispatched"` with `fallback: "codex"`, no Relay run was started:
do not call `wait_agent`, `accept_agent`, or retry with shared isolation. Take
over the original task in Codex, preserving the requested isolation. If Codex
cannot safely continue with that isolation, explain the limitation and ask the
user only when their choice is needed.

After starting a worker, use `wait_agent` for its terminal result. Use
`get_agent_status` to check progress; if it is still running, keep waiting or
continue other useful work. Review `result.summary` and relevant workspace
changes, and verify important claims where needed. Use `cancel_agent` when the
work is no longer needed. `send_agent` is available only for runtimes that
support live follow-up.

Treat Relay results as input to Codex's reasoning, not messages to forward
automatically. Continue orchestration or verification while the user's request
still needs work. Once the request is complete, answer the user's original
question with the verified findings or deliverable; do not reply with only a
worker status such as "completed". If the task cannot proceed, explain the
blocker and what remains. During a long run, give concise progress updates when
the user would otherwise see no activity. `accept_agent` marks a reviewed Relay
run complete; it does not send a chat reply.

Relay session names always come from Codex. Never invent or rename a Relay
session independently.
