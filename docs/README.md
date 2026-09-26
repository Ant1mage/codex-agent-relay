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

用户主入口保持为 Codex。Relay 的桌面端是可选控制台，大部分时间可以驻留在菜单栏或系统托盘。

Relay 不画复杂 Agent 拓扑图，也不把自己包装成另一个 AI 工作台。桌面端采用本地 macOS 工具的克制信息架构：

1. 左侧只有按最近使用排序的 Codex Sessions，名称与 Codex 完全一致。
2. 主区域以横向 Step 导航和占据大部分空间的 Console 为核心。
3. Console 只展示 Read、Search、Edit、Command、Test、Result、Error、Status 等可观察动作。
4. Changes 与 Raw Output 是按需打开的检查器，不是常驻顶级页面。
5. Runtime、Profile、Policy 与语言统一收进齿轮入口的 Settings。

## 文档导航

- [architecture.md](architecture.md)：概念模型、进程架构、数据模型、日志、Routing 与安全边界。
- [integrations-and-adapters.md](integrations-and-adapters.md)：Codex 集成、Adapter 设计、DeepSeek Harness 借鉴与 backend 优先级。
- [codex-integration.md](codex-integration.md)：Codex session 对齐、名称同步与 stdio MCP 工具面。
- [menu-bar.md](menu-bar.md)：macOS 菜单栏的定位、Clash 式信息架构、行为规则与实现边界。
- [mvp-roadmap.md](mvp-roadmap.md)：MVP 范围、技术栈和开发顺序。

## 一句话架构

```text
Codex
  ├─ Hooks：提供可信的 Host Session identity
  └─ stdio MCP：list/run/resume/accept/send/wait/cancel
                 ↓
              Relay Core
       Registry · Policy · Runtime · Event Store
                 ↓
        DeepSeek / Antigravity / Kimi / Gemini / GLM CLI
```
