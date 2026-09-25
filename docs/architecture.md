# Relay 架构与数据模型

## 1. 概念模型

Relay 必须从第一天分清 `Runtime`、`Agent Profile`、`Run` 与 `WorkerSession`。它们不是同一个概念。

### Runtime

一个本机可执行的原厂 Agent/Harness，例如 DeepSeek Harness、Antigravity CLI 或 Kimi Code。Runtime 描述“怎么启动和控制一个 worker”，包含：

- 可执行文件路径、版本、认证/可用状态。
- 是否支持 non-interactive、结构化流、cwd、resume、send、cancel。
- 对应 Adapter。

扫描本机发现的是 Runtime，不是 Agent。

### Agent Profile

用户基于 Runtime 创建、暴露给 Codex 的能力配置。例如同一个 DeepSeek Runtime 可以创建：

- `deepseek-code`：读写文件、执行命令、跑测试。
- `deepseek-research`：只读代码、允许外部搜索、不写 workspace。

Profile 至少包含：

```ts
type AgentProfile = {
  id: string
  name: string
  runtimeId: string
  description: string
  instructions?: string
  capabilities: CapabilitySet
  enabled: boolean
}
```

角色名称不应硬编码成 Search/Code 二选一。UI 可以提供 Coding、Research、Review、Testing 模板，但真正的契约是结构化 capability 加自由描述。

### Worker

Worker 是某次 Run 中实际启动的执行者/进程。它是运行态对象，不是长期配置对象。原厂 Harness 内部自行创建的 child agent 也属于 worker 树的一部分，只用于观察和控制，不进入 Relay 的长期 Profile 配置。

### Subagent

对 Codex 来说，Agent Profile 是一个可委派的 subagent 能力；对 Relay 来说，它解析为 Runtime 上的一次 Run；对原厂 Harness 来说，它可能进一步 fan-out 多个 child。三个层级不能混为一谈。

```text
Codex：能力编排
  ↓ Agent Profile
Relay：路由、约束、生命周期、记录
  ↓ WorkerSession
原厂 Harness：具体执行，必要时内部多 Agent
```

## 2. Session / Run / WorkerSession / Event

建议采用四层模型：

```text
HostSession
└── Run
    └── WorkerSession
        └── Event[]
```

### HostSession

代表主 IDE/Host 中的一条会话，而不是项目目录。相同 repo 可以同时有多个 Codex 会话，日志和策略不能串线。

```ts
type HostSession = {
  id: string                 // 例如 codex:<native-session-id>
  host: 'codex'              // MVP 只做 Codex
  nativeSessionId: string
  displayName: string        // 始终使用 Codex 当前的线程名称
  nameSource: 'codex'        // Relay 不创建独立别名
  cwd: string
  model?: string
  status: 'active' | 'offline' | 'ended'
  startedAt: string
  endedAt?: string
}
```

### Run

每次 Codex 调用 `run_agent` 都创建一个 Run。Run 是 Relay 面向 Host 的任务生命周期。

```ts
type Run = {
  id: string
  hostSessionId: string
  profileId: string
  task: string
  cwd: string
  accessMode: 'read_only' | 'propose' | 'write'
  isolation: 'shared' | 'worktree'
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled' | 'handed_off'
  createdAt: string
}
```

### WorkerSession

映射原厂 Runtime 的 session/conversation/process，不能和 HostSession ID 混用。

```ts
type WorkerSession = {
  id: string
  runId: string
  runtimeId: string
  nativeSessionId?: string
  parentWorkerSessionId?: string
  processId?: number
  status: 'starting' | 'running' | 'completed' | 'failed' | 'cancelled'
  startedAt: string
  endedAt?: string
}
```

一个 Run 通常对应一个顶层 WorkerSession；如果原厂 Harness 暴露 child/subagent 事件，则用 `parentWorkerSessionId` 形成树。

### Event

Event 是 append-only 的唯一事实来源。当前状态、GUI 摘要、历史、恢复与调试都由事件投影得到，避免在多张表中重复维护 `progress`、`lastMessage`、`filesChanged` 等易漂移状态。

```ts
type RelayEvent = {
  id: string
  runId: string
  workerSessionId?: string
  seq: number
  timestamp: string
  type:
    | 'run/created'
    | 'worker/started'
    | 'worker/message'
    | 'worker/reasoning'
    | 'tool/read'
    | 'tool/search'
    | 'tool/edit'
    | 'tool/command'
    | 'tool/result'
    | 'test/result'
    | 'child/started'
    | 'child/completed'
    | 'worker/completed'
    | 'worker/failed'
    | 'worker/cancelled'
  data: unknown
}
```

最低约束：同一 Run 内 `seq` 单调递增；保留原始 provider event 便于排障，同时生成稳定的 normalized event 供 GUI 使用。

## 3. 生命周期

```text
queued → starting → running
                      ├─ completed
                      ├─ failed
                      ├─ cancelled
                      └─ handed_off
```

- `cancel`：立即停止，不承诺生成进度摘要。
- `handoff`：请求 worker 总结已完成、未完成、改动文件与风险，然后停止并交回 Codex。
- `detach` 可后置；MVP 中可先用 handoff/cancel 覆盖主要需求。

关闭某个 Profile 只禁止新 Run，不自动杀掉已经运行的 worker。停止运行中任务必须是独立动作。

## 4. Routing Policy

配置按“最具体者优先”解析：

```text
Global default
    ↓ override
Workspace policy
    ↓ override
HostSession temporary override
```

解析优先级为 `Session > Workspace > Global`。Session override 随 HostSession 生命周期清理，不应成为主要配置方式。

Policy 在 Adapter 之前执行：

```text
RunRequest
  → resolve profile and scope
  → enabled / permission check
  → concurrency limit
  → workspace conflict / isolation
  → Adapter.start()
```

Adapter 只回答“如何启动这个 Runtime”，不负责“当前是否允许启动”。

### 写入冲突与隔离

任务不同不等于不会改到同一个公共文件。建议默认规则：

- Read-only worker：共享当前 workspace。
- 单个 writer：可共享当前 workspace。
- 并行 writer：使用独立 git worktree，或要求明确不相交的文件范围。
- 必须修改同一文件的多个 writer：优先串行 handoff，不依赖 Agent 间自由通信。

Relay 不提供 peer-to-peer Agent chat。需要把 A 的信息交给 B 时，由 Codex 重新打包上下文并调用 `send_agent`/新 Run，保持 Codex 是唯一全局协调者。

## 5. 日志与 GUI

GUI 是 Subagent Activity Monitor，不是开发工作台。默认信息架构：

```text
Sessions
  └─ Runs
      └─ Worker tree + event timeline

Agents
  └─ Runtime detection + Profile configuration

Routing / Settings
```

单个 Run 的视图应直接回答：谁在做什么、已经多久、改了什么、测试怎样、是否需要处理。

```text
DeepSeek Code · RUNNING · 03:21
  ├─ analyze callsites        DONE
  ├─ update service           RUNNING
  └─ update tests             WAITING

00:18  Read       AuthManager.swift
00:31  Edit       OAuthService.swift
00:42  Command    swift test
00:58  Test       42 passed, 2 failed
```

GUI 所有状态来自 Event projection。Renderer 不直接读取 worker stdout，也不包含 Runtime 业务逻辑。

## 6. 进程边界

```text
Codex
  │ stdio MCP
  ▼
relay-mcp（薄桥接进程）
  │ Unix Domain Socket / Named Pipe
  ▼
Relay Desktop / Core（Electron Main）
  ├─ SessionManager
  ├─ WorkerManager
  ├─ RoutingPolicy
  ├─ EventStore
  └─ AdapterRegistry
  │ child_process.spawn + stdin/stdout/stderr
  ▼
Worker CLIs
```

三个通信面彼此独立，全程不需要 TCP 端口。Renderer 通过 Electron IPC 读取 Core projection。
