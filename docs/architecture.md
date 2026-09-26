# Relay 架构与数据模型

## 1. 概念模型

Relay 必须分清 `Runtime`、`Agent Profile`、`TaskRun`、`Step` 与 `WorkerSession`。它们不是同一个概念。

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

Worker 是某个 Step 中实际启动的执行者/进程。它是运行态对象，不是长期配置对象。原厂 Harness 内部自行创建的 child agent 属于 Runtime 内部行为，只作为弱元数据或原始事件保留，不提升为 Relay 的顶层 Step。

### Subagent

对 Codex 来说，Agent Profile 是一个可委派的 subagent 能力；对 Relay 来说，它解析为 Runtime 上的一次 Run；对原厂 Harness 来说，它可能进一步 fan-out 多个 child。三个层级不能混为一谈。

```text
Codex：能力编排
  ↓ Agent Profile
Relay：约束、生命周期、Session 绑定、记录与传输
  ↓ WorkerSession
原厂 Harness：具体执行，必要时内部多 Agent
```

Relay 不负责选择 Profile、拆解计划、验收结果或自动重试；这些智能决策始终由 Codex 完成。

## 2. Session / TaskRun / Step / WorkerSession / Event

建议采用五层模型：

```text
HostSession
└── TaskRun
    ├── Step 1
    │   └── WorkerSession · iteration 1..n
    └── Step 2
        └── WorkerSession · iteration 1..n
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

### TaskRun

TaskRun 是 Relay 面向 Host 的任务生命周期。worker 报告完成时，TaskRun 进入 `awaiting_host`，而不是自动变成已完成；只有 Codex review 后调用 accept，任务才完成。worker completion 不等于 task completion。

```ts
type Run = {
  id: string
  hostSessionId: string
  title: string
  cwd: string
  status: 'queued' | 'running' | 'awaiting_host' | 'completed' | 'failed' | 'cancelled' | 'interrupted' | 'orphaned'
  createdAt: string
  updatedAt: string
}
```

### Step

Step 是一次顶层 Codex → Relay 委派，是主界面横向导航的最小单位。同一个 worker 被 Codex 驳回后继续执行，仍属于同一个 Step，只增加 `iteration`；切换 Profile/Runtime 或创建新的并行委派才是新 Step。Runtime 内部 tool call 或 child agent 不是 Step。

```ts
type Step = {
  id: string
  runId: string
  profileId: string
  task: string
  accessMode: 'read_only' | 'propose' | 'write'
  isolation: 'shared' | 'worktree'
  status: 'queued' | 'starting' | 'running' | 'awaiting_host' | 'completed' | 'failed' | 'cancelled' | 'interrupted' | 'orphaned'
  iteration: number
  createdAt: string
  updatedAt: string
}
```

### WorkerSession

映射原厂 Runtime 的 session/conversation/process，不能和 HostSession ID 混用。

```ts
type WorkerSession = {
  id: string
  runId: string
  stepId: string
  iteration: number
  runtimeId: string
  nativeSessionId?: string
  parentWorkerSessionId?: string
  processId?: number
  status: 'starting' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted' | 'orphaned'
  startedAt: string
  endedAt?: string
}
```

一个 Step 的每次 iteration 对应一个顶层 WorkerSession。Relay 可以记录原厂 Harness 暴露的 child/subagent 数量或原始事件，但不建立需要跨 provider 保持一致的 child tree。

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
    | 'run/awaiting_host'
    | 'run/accepted'
    | 'step/created'
    | 'step/iteration_started'
    | 'worker/started'
    | 'worker/message'
    | 'worker/status'
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
    | 'worker/interrupted'
    | 'worker/orphaned'
  data: unknown
}
```

最低约束：同一 TaskRun 内 `seq` 单调递增；原始 provider event 以有界、可截断形式保留供排障，同时生成稳定的 normalized event 供 GUI 使用。Raw Output 指 stdout、stderr 和结构化 native event 等完整可观察输出，不包含隐藏思维链。

## 3. 生命周期

```text
queued → starting → running → awaiting_host → completed
                      │             └─ Codex reject/resume → next iteration
                      ├─ failed
                      ├─ cancelled
                      ├─ interrupted
                      └─ orphaned
```

- `cancel`：立即停止，不承诺生成进度摘要。
- `awaiting_host`：worker 已交回可观察结果，等待 Codex 验收。
- `accept`：Codex 验收通过，TaskRun 才进入 `completed`。
- `resume`：Codex 给同一 worker 反馈，Step 的 iteration 加一；不支持原生 resume 时采用显式降级并记录新 native session。
- `interrupted`：进程、连接或 Host 非正常中断，仍可能恢复。
- `orphaned`：Relay 重启后无法重新绑定仍在运行或状态未知的进程。

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
Relay + Settings gear
Sessions (exact Codex names, most recently used first)
  └─ Task title
      ├─ horizontal Step navigator
      └─ Console
          ├─ Changes inspector
          └─ Raw Output inspector
```

单个 Step 的视图应直接回答：谁在做什么、已经多久、改了什么、测试怎样、是否需要 Codex 处理。

```text
Step 2 · DeepSeek Code · RUNNING · iteration 1 · 03:21

00:18  Read       AuthManager.swift
00:31  Edit       OAuthService.swift
00:42  Command    swift test
00:58  Test       42 passed, 2 failed
```

GUI 所有状态来自 Event projection。Renderer 不直接读取 worker stdout，也不包含 Runtime 业务逻辑。界面不显示虚假百分比进度、隐藏推理、工作流图或垂直 agent 树；浅色主题优先，深色主题使用同一套语义 token。

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
