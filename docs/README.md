# Relay 设计文档

Relay 是一个面向 Codex 等主开发 Agent 的本地 **Subagent Controller**：它把不同厂商的可脚本化 Coding Agent CLI 统一成可发现、可调用、可观察、可约束的 worker。

核心原则只有一句：

> Codex 决定“为什么、何时、把什么任务交给谁”；Relay 负责“有哪些 worker、如何安全启动、如何记录和停止它们”。

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
- Activity Console：按 Host Session 展示 Run、WorkerSession 和事件日志。

Relay 不是：

- IDE、代码编辑器、终端或 Git 客户端。
- 新的主 Agent 或 LLM orchestrator。
- Agent Loop、Prompt Assembly、上下文压缩或模型重试层。
- Workflow Engine；任务编排由 Codex 和各原厂 Harness 自己完成。
- 多模型 API 聚合器。

## 产品边界

用户主入口保持为 Codex。Relay 的桌面端是可选控制台，大部分时间可以驻留在菜单栏或系统托盘。

Relay 不画复杂 Agent 拓扑图。主界面围绕真实工作展开：

1. Sessions：当前和历史 Codex 会话。
2. Runs：每次委派的任务。
3. Live Logs：worker 正在做什么。
4. Agents：Runtime 检测与 Profile 配置。
5. Routing / Settings：全局、workspace、session 范围的策略。

## 文档导航

- [architecture.md](architecture.md)：概念模型、进程架构、数据模型、日志、Routing 与安全边界。
- [integrations-and-adapters.md](integrations-and-adapters.md)：Codex 集成、Adapter 设计、DeepSeek Harness 借鉴与 backend 优先级。
- [mvp-roadmap.md](mvp-roadmap.md)：MVP 范围、技术栈和开发顺序。

## 一句话架构

```text
Codex
  ├─ Hooks：提供可信的 Host Session identity
  └─ stdio MCP：list/run/send/wait/cancel
                 ↓
              Relay Core
       Registry · Policy · Runtime · Event Store
                 ↓
        DeepSeek / Antigravity / Kimi CLI
```

