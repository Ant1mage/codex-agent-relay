# Relay 设计文档

Relay 是一个面向 Codex 等主开发 Agent 的本地 **Subagent Controller**：它把不同厂商的可脚本化 Coding Agent CLI 统一成可发现、可调用、可观察、可约束的 worker。

核心原则只有一句：

> Codex owns intelligence. Relay owns worker lifecycle, session binding, observability and transport. Native CLIs own their internal agent behavior.

## 产品定位

Relay 解决的是开发者目前在多个 Agent 之间手工搬运任务的流程：

```text
Codex 规划 → 人工复制任务 → 切到另一个 CLI → 等执行 → 切回 Codex review
```

目标体验是：

```text
用户始终留在 Codex
        ↓
Codex 通过 Relay 委派 bounded task
        ↓
第三方原厂 Harness / CLI 执行
        ↓
结果、修改和状态回到 Codex
        ↓
Codex review、继续委派或自己完成
```

Relay 是：

- Agent Tool Registry：管理 Codex 当前可调用的 Agent Profile。
- Runtime Controller：管理 CLI 发现、进程、会话、取消与恢复。
- Policy Enforcement Point：落实权限、并发与 workspace 隔离规则。
- Activity Console：按 Host Session 展示 Step、WorkerSession 和可观察事件。

Relay 不是：

- IDE、代码编辑器、终端或 Git 客户端。
- 新的主 Agent 或 LLM orchestrator。
- Agent Loop、Prompt Assembly、上下文压缩或模型重试层。
- Workflow Engine；任务编排由 Codex 和各原厂 Harness 自己完成。
- 多模型 API 聚合器。

## 产品边界

用户主入口保持为 Codex。Relay 没有应用窗口：桌面存在感是一个菜单栏图标（状态 + 配置面板），日志在一个浏览器页面里。**配置属于菜单栏，日志属于浏览器**——Web 不承担 Settings 职责。

```text
menu-bar（Tray + 控制面板）── 启动/停止 ──► relayd（本地 Control Plane，127.0.0.1:7352）
        │                                     ├─ 配置（Agent / Policy）
        │                                     ├─ Runtime 扫描 / Codex 集成生命周期
        │                                     └─ 托管 /panel/（配置）与 /（日志）
        └─ 点击会话 → Edge/Chrome 打开 ──► Web Inspector（React + shadcn，只读）
```

Relay 不画复杂 Agent 拓扑图，也不把自己包装成另一个 AI 工作台。界面沿用本地 macOS 工具的克制信息架构：

1. 左栏只有按最近使用排序的 Codex Sessions，名称与 Codex 完全一致。
2. 中间是 Run 卡片条，下面是占据大部分空间的 Console。
3. Console 只展示 Read、Search、Edit、Command、Test、Result、Error、Status 等可观察动作。
4. Changes 与 Raw Output 是同一页里的标签，不是常驻顶级页面。
5. Runtime、Profile 与 Codex 集成的状态由 daemon 探测后随投影一起展示。

## 文档导航

- [architecture.md](architecture.md)：概念模型、进程架构、数据模型、日志、Routing 与安全边界。
- [integrations-and-adapters.md](integrations-and-adapters.md)：Codex 集成、Adapter 设计、DeepSeek Harness 借鉴与 backend 优先级。
- [codex-integration.md](codex-integration.md)：Codex session 对齐、名称同步与 stdio MCP 工具面。
- [inspector.md](inspector.md)：日志服务 relayd 的端口、API、SSE、安全边界与 Web Inspector。
- [menu-bar.md](menu-bar.md)：macOS 菜单栏的定位、信息架构、行为规则与实现边界。
- [mvp-roadmap.md](mvp-roadmap.md)：MVP 范围、技术栈和开发顺序。

## 一句话架构

```text
Codex
  ├─ Hooks：提供可信的 Host Session identity
  └─ stdio MCP：list/run/resume/accept/send/wait/cancel
                 ↓
              Relay Core（在 relay-mcp 进程内）
       Registry · Policy · Runtime · Event Store
                 ↓
        DeepSeek / Antigravity / Kimi / Gemini / GLM CLI

事件日志（~/.relay/relay.sqlite）
  └─ relayd（127.0.0.1:7352）──► menu-bar + Web Inspector（只读投影、可取消）
```
