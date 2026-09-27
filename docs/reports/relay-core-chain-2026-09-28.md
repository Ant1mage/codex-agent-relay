# Relay Delegation Verification — 2026-09-28

## Summary

The live read-only delegation succeeded: Codex received the agent's result and
included its marker and verified README heading in the final reply. Relay does
not post a separate chat message; Codex reads results through MCP and decides
what to tell the user.

## Results

| Scenario | Relay result | User-facing Codex result |
| --- | --- | --- |
| Live agent reads `README.md` | PASS | PASS — findings appeared in Codex's final reply |
| MCP delegate → wait → review → accept | PASS | Integration test verifies tool responses, not host rendering |
| Search → write → review using fixture CLI | PASS | Test verifies file contents, event order, summary, and accepted state |
| MCP call without `CODEX_THREAD_ID`, with identity in request metadata | PASS | Regression test passes; live host metadata path remains unverified |
| Unsupported `worktree` request | PASS — successful `not_dispatched`, `fallback: "codex"`; no worker created | Skill directs Codex to take over without shared-mode retry |
| Cancellation, worker failure, worker surviving MCP exit, two MCP servers, missing daemon | PASS | Covered by integration tests |

## Fixes

- **Session identity:** MCP dispatch now checks caller metadata as well as tool
  arguments and environment variables. Previously, a missing `CODEX_THREAD_ID`
  could make ordinary calls fail before delegation.
- **Worktree safety and fallback:** Relay does not create Git worktrees. The core
  rejects direct write/worktree requests before launching a worker. MCP returns
  a successful `not_dispatched` handoff for explicit worktree requests, so
  Codex can continue the original task without waiting or silently switching to
  the shared workspace.
- **Result handling:** The Relay skill instructs Codex to review worker output,
  continue orchestration when needed, and answer the user's original request.
  `accept_agent` closes a reviewed run but does not send a chat reply.

## Verification

- `cargo test -p relay-mcp --test delegation` — 9 passed.
- `cargo test -p relay-core` — 39 passed.
- `cargo fmt --all --check` and `git diff --check` — passed.
- Relay plugin update — integration checks passed; installed skill matches the
  repository source.

## Limits

The current MCP transport in this Codex host returns `Transport closed`, so the
new metadata and worktree-fallback paths are verified through MCP integration
tests, not a fresh host-rendered reply. The earlier live README inspection is
the direct proof that a Relay result appeared in a Codex response. Real worktree
creation remains unimplemented; write runs must not be retried in shared mode
unless that access is explicitly intended.
