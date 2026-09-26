# Codex 集成、Adapter 与 Harness 借鉴

## 1. Codex 是主 IDE 与 orchestrator

MVP 只把 Codex 作为 Host。集成由两条链路组成：

```text
Codex Hooks → 可信的 session identity 与生命周期
Codex MCP   → Agent 控制命令
```

纯 stdio MCP 不应承担“猜当前是哪条 Codex 会话”的职责。Relay 的 Codex 插件使用 SessionStart / SessionEnd 注册会话，并在 MCP 调用前把 host、session、turn 等身份字段注入调用。身份字段来自 Hook，不由模型自行填写。

建议的 MCP surface 保持通用和稳定，避免为每个 Profile 动态创建一个 tool：

```text
list_agents()
run_agent(agent_id, task, options?)
accept_agent(worker_session_id)
send_agent(worker_session_id, message)
get_agent_status(worker_session_id)
wait_agent(worker_session_id)
cancel_agent(worker_session_id)
```

`list_agents()` 返回当前 scope 下可用的 Profile、描述与 capabilities，Codex 据此自行编排。一个 Profile 被禁用时，`run_agent` 仍需在服务端再次校验，不能只信任工具发现结果。

worker 给出结果后，`wait_agent` 返回 `awaiting_host`。Codex review 后调用 `accept_agent`；若要继续同一 Step，后续由 `resume_agent` 走运行时能力探测后的降级路径，而不是由 Relay 自行重试。

插件结构建议：

```text
relay-codex/
├── plugin.json
├── skills/relay/SKILL.md
└── hooks/
    ├── hooks.json
    └── relay-hook.js
```

## 2. Adapter 边界

Core 不出现 `if (runtime.type === 'kimi')` 一类 provider 分支。所有差异收敛到 Adapter：

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

Adapter 负责：

- 检测 CLI、版本、认证与控制能力。
- 把统一 StartInput 转为原厂 CLI 参数/协议。
- 管理进程和原厂 session ID。
- 把 native event 归一化成 RelayEvent。
- 实现可用的 send、resume、cancel。

Adapter 不负责：

- 选择该调用哪个 Profile。
- 权限、并发、成本或 workspace 冲突判定。
- 任务拆解、重试工作流或结果 review。
- 拼装主 Agent 的系统提示词。

### 可逆生命周期

所有注册动作返回可释放资源：

```ts
interface Disposable {
  dispose(): void | Promise<void>
}
```

Adapter 卸载时必须清理 runtime registry、parser、监听器、watcher 和持有的资源，避免热更新后的幽灵状态。

### Provider 与 Consumer 分离

`KimiAdapter` 是 provider：定义如何控制 Kimi Code。`kimi-code` / `kimi-research` Profile 是 consumer-facing configuration：定义 Codex 看到的用途、描述与权限。

因此：

```text
Runtime ≠ Agent Profile ≠ MCP Tool
```

同一个 Runtime 可服务多个 Profile，而 MCP tool surface 始终是通用控制协议。

## 3. 从 DeepSeek Harness 借鉴什么

Relay 借的是“内核边界”，不是 Agent 本身。

### 3.1 Capability / provider seam

Core 依赖 registry 与抽象能力，provider 通过 Adapter 注册。新增 Kimi、Antigravity 或其他 CLI 不应要求修改 Session、Policy 或 GUI 核心逻辑。

### 3.2 Event-sourced session

使用 append-only typed event log 作为 Run 的事实来源，UI、历史、恢复和调试都是 projection。SQLite 是 MVP 的持久化实现，不把实现细节泄漏成 Core 契约。

### 3.3 Scoped resolution

采用 Global → Workspace → HostSession 的配置覆盖模型，并由 Core 统一解析 `most-specific-wins`，不让 UI 或 Adapter 各自拼配置。

### 3.4 Registration equals lifecycle

注册 Runtime、parser、listener 与 watcher 的同时定义 dispose 行为，保证 Adapter 加载/卸载可逆。

### 3.5 Policy 与 implementation 分离

权限、deadline、并发、隔离等前置于 Adapter 执行；Adapter 专注原厂协议。

### 3.6 Provider 与 tool consumer 分离

底层 Runtime 控制和上层 Profile/tool 描述分开，使一个 CLI 能暴露成多个不同权限、不同用途的能力。

如果只保留三项，优先级是：

1. Event-sourced Session/Run。
2. Capability/provider seam。
3. Scope + reversible lifecycle。

## 4. 明确不从 Harness 复制什么

Relay 不实现：

- Agent Loop。
- LLM 请求、stream 驱动和 tool dispatch loop。
- System Prompt Assembly。
- 模型层、token/context management、compaction。
- Model retry 策略。
- Workflow Engine、workflow VM、model-written orchestration script。

原因不是这些能力没有价值，而是它们已经属于 Codex 或原厂 Harness。Relay 再实现一遍会出现两个 orchestrator，边界混乱：

```text
Codex（唯一上层 orchestrator）
  ↓
Relay（执行、约束、记录）
  ↓
原厂 Harness（自己的 Agent Loop，可自行管理内部 child）
```

## 5. 推荐 backend 优先级

优先级按“程序化控制与 Adapter 友好度”排序，不是模型能力排名。

| 优先级 | Backend | 角色 | 原因 |
|---|---|---|---|
| P0 | DeepSeek Harness | 核心 Coding / Research worker | 与产品起点一致；原生 session/event 与内部 subagent 能力可验证事件和 child tree 设计 |
| P0 | Antigravity CLI | Research、large-context、第二意见 | headless + structured stream，conversation/session 与 tool/subagent 事件适合日志归一化 |
| P0 | Kimi Code | 通用 Coding / Research | 非交互、结构化流、session/resume，国内用户认证与使用路径友好 |
| P0 | Gemini CLI | 通用 Coding / Research | 官方 headless JSONL 事件与原生 session resume；Relay 不默认开启 `--yolo` |
| P1 | GLM / Z.ai CLI | GLM 第二意见与限定分析 | 官方 `zai-cli chat` 默认 JSON 输出，当前作为无 workspace 写权限的 structured worker |
| P1 | MiniMax Code | 第四个通用 Coding worker | headless、ACP、resume、subagent 等控制能力完整，适合作为 Adapter 可扩展性验证 |
| P2 | GLM / ZCode | 后续补充 | 有完整 runtime，但第三方控制契约优先级低于前三个，不阻塞 MVP |
| P2 | Qwen Code | 后续通用 Harness | 能力强但与 Relay 编排面有部分重叠，认证路径也不是首版核心 |
| P2 | Grok Build | 海外用户扩展 | headless/structured output/ACP 方向匹配，但目标用户优先级靠后 |

MVP 不是“支持尽可能多的模型”，而是用三个差异明显的原厂 Runtime 验证：

> Relay 能否把不同厂商的 Coding Harness 稳定地标准化为 Codex 可编排的 Agent Profiles。

首个垂直切片只用 DeepSeek Harness 做真实端到端验证。Kimi、Antigravity、Gemini 与 GLM / Z.ai 的 CLI 接入可以先通过 detection、编译和离线 parser 完成；未配置账号或 key 时不得在测试中发起真实运行。
