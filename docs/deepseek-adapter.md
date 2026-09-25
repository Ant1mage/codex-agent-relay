# DeepSeek Harness Adapter

Relay controls the official `@deepseek-ai/dsh` launcher through its one-shot
headless profile. The process contract is:

```text
dsh --profile headless --json [--session-id <id>]
```

The task is written to stdin so it is not exposed in the process list. The
working directory is the worker workspace. Relay waits for the opening
`session` event, records its native session ID, maps the newline-delimited JSON
stream to `RelayEvent`, and determines success from both the process exit code
and terminal `turn_end` reason.

Current upstream JSON events are normalized as follows:

| Harness event | Relay event |
|---|---|
| `thinking` | `worker/reasoning` |
| `text`, `status`, `final`, `error` | `worker/message` |
| `tool_call` | `tool/read`, `tool/search`, `tool/edit`, or `tool/command` |
| `tool_result` | `tool/result` |
| successful process close | `worker/completed` |
| unsuccessful process close | `worker/failed` |

Every mapped event retains the original native event. The headless projection
does not currently expose child-session events or interactive follow-up, so the
Adapter reports `childSessions: false` and `send: false`. It does support later
one-shot continuation via `--session-id`.

For a live environment check, run:

```bash
pnpm deepseek:smoke -- "Inspect this repository and summarize it" /path/to/workspace
```

The smoke command prints normalized Relay events as JSON lines. It requires an
installed and authenticated `dsh`; fixture-based tests cover the same process
and parser boundary without making a model request.

References:

- [Official CLI application contract](https://github.com/deepseek-ai/deepseek-harness/tree/master/apps/cli)
- [Official headless profile contract](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/bundle/headless)
