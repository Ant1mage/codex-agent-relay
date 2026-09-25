# MVP 范围、技术栈与开发顺序

## 1. MVP 成功标准

第一版只需证明一条完整链路：

1. Relay 检测到一个本机 Runtime。
2. 用户基于它创建 Agent Profile。
3. Codex 在当前 session 中发现并调用该 Profile。
4. Relay 按 policy 启动 worker，持续记录结构化事件。
5. 用户可在 GUI 中查看 Run 与实时日志，并可取消。
6. worker 结果回到原 Codex session，由 Codex review 或继续工作。
7. 退出重启后仍能查看历史 Run。

## 2. MVP 范围

### 必须有

- Host：Codex。
- Codex 插件：Skill + Hooks + stdio MCP 配置。
- 通用工具：list、run、status/wait、send（Runtime 支持时）、cancel。
- Runtime 扫描：路径、版本、可执行、认证/健康状态、能力矩阵。
- Agent Profile：name、runtime、description、capabilities、instructions（可选）。
- DeepSeek Harness Adapter 的完整垂直切片。
- HostSession → Run → WorkerSession → Event 数据链路。
- SQLite append-only Event Store 和基础 projection。
- Sessions、Run 列表、Live Log、Agents、Settings 五个最小页面。
- 英语与简体中文界面；默认跟随系统语言，并允许用户切换后持久化。
- Global / Workspace policy；Session 临时 override 可做最小版本。
- Read-only 与 write capability；并行 writer 的保守冲突保护。
- cancel；handoff 可作为 MVP 后半程能力。

### 明确不做

- 代码编辑器、项目树、终端、Git UI、聊天主界面。
- 自研 Agent Loop、Workflow Engine、Prompt Assembly。
- Relay 内部自动规划、自动 review、自动 retry 闭环。
- Agent 之间 peer-to-peer 通信。
- 拓扑图编辑器。
- 复杂成本优化、模型市场、统一 API 计费。
- 首版同时支持大量 backend 或 Host。
- 多 writer 自动合并与复杂文件 reservation 系统。

## 3. 推荐技术栈

MVP 采用全栈 TypeScript，优先降低本地进程与协议集成的复杂度。

| 层 | 推荐技术 |
|---|---|
| Desktop | Electron |
| UI | React + TypeScript + Vite |
| UI state | Zustand |
| UI components | shadcn/ui + Tailwind CSS |
| Runtime/Core | Node.js + TypeScript |
| CLI 管理 | `child_process.spawn()` |
| MCP | 官方 MCP TypeScript server/stdio packages |
| 内部 IPC | Unix Domain Socket / Windows Named Pipe，使用 Node `net` |
| 持久化 | SQLite + `better-sqlite3` |
| Schema | Zod |
| i18n | 类型安全的内置资源（`en` / `zh-CN`），Renderer 与 Core 共用翻译键 |
| Monorepo | pnpm workspace |
| 打包 | Electron Forge 或 electron-builder（二选一，早期不并存） |
| Logging | 自定义、版本化的 `RelayEvent` |
| 测试 | Vitest；Adapter 增加 fixture-based parser tests |

选择 Electron 而不是 Tauri，是因为 Relay 的难点集中在 Node 生态：MCP、stdio、子进程、JSON event stream 和各类 Node-based CLI。Tauri 会额外引入 Rust → Node sidecar 层，而 Electron Main 可直接承载 Core。Renderer 不承担进程管理。

建议 monorepo：

```text
relay/
├── apps/
│   └── desktop/          # Electron Main + React Renderer
├── packages/
│   ├── core/             # registry, policy, runs, events
│   ├── mcp/              # 极薄 stdio bridge
│   ├── protocol/         # Zod schemas + shared types
│   ├── adapter-sdk/      # AgentAdapter contract + test kit
│   └── adapters/
│       ├── deepseek/
│       ├── antigravity/
│       └── kimi/
└── integrations/
    └── codex/            # Plugin, Skill, Hooks
```

## 4. 开发顺序

### Phase 0：契约先行

- 固化术语：HostSession、Runtime、Profile、Run、WorkerSession、Event。
- 定义 Zod schema、状态机、错误码与 capability matrix。
- 定义 Adapter contract 和 native-event fixture 测试方式。
- 明确 Core 不依赖任何具体 provider。

完成标准：用 fake adapter 在内存中跑通一个 Run 和事件序列。

### Phase 1：Core 垂直切片

- SQLite schema 与 append-only Event Store。
- Run lifecycle、projection、cancel。
- Registry 与 Global/Workspace/Session policy resolver。
- 并发和 write-conflict 的保守规则。

完成标准：fake worker 可启动、产生日志、完成/失败/取消，并在重启后恢复历史视图。

### Phase 2：第一个真实 Adapter

- DeepSeek Harness 检测、启动、session 映射、事件归一化、取消。
- 保留 native event，建立 parser fixtures。
- 如果 Harness 暴露 child event，则映射 WorkerSession tree。

完成标准：命令行测试程序能在指定 cwd 发起真实任务并得到稳定 RelayEvent。

### Phase 3：Codex 集成

- 独立 `relay-mcp` stdio package。
- `list_agents`、`run_agent`、`get/wait`、`send`、`cancel`。
- SessionStart/End 注册 HostSession。
- PreToolUse 注入可信 host/session/turn identity。
- Skill 教 Codex 将当前上下文压成 bounded task spec，而不是整段对话转发。

完成标准：用户不离开 Codex 即可委派、等待结果并继续 review。

### Phase 4：最小 GUI

- Sessions 列表和 HostSession 详情。
- Run 卡片、状态、耗时、worker tree。
- 实时 event timeline、错误和取消按钮。
- Agents 页面：扫描 Runtime、创建/编辑 Profile。
- Settings：默认 policy 与 workspace override。

完成标准：GUI 对运行态的展示完全来自 projection，不解析原厂 stdout。

### Phase 5：第二、第三 Adapter

- Antigravity 与 Kimi Adapter。
- 用同一 Adapter conformance suite 验证 detect/start/event/cancel/resume。
- 补齐 provider-specific capability 降级逻辑。

完成标准：新增 Adapter 不修改 Core 数据模型、Policy 或 GUI 主流程。

### Phase 6：可靠性与发布

- 崩溃恢复、孤儿进程处理、超时和日志脱敏。
- macOS/Windows socket/pipe 与打包验证。
- 明确升级与 schema migration。
- 菜单栏/托盘状态、诊断导出。

## 5. 延后项

在真实用户验证基础链路前，不提前实现：

- Claude Code 等第二 Host。
- 自动 worktree 合并、文件 reservation。
- 成本/额度智能路由。
- Profile 市场和第三方 Adapter 插件系统。
- 高级 handoff/retry/approval pipeline。
- 基于历史数据的自动路由推荐。

这些都应建立在稳定的 Adapter、Event 与 HostSession 边界之上，而不是反过来驱动 MVP 架构。
