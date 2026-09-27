# Relay — OpenAI Codex 本地 MCP Agent 委派运行时

[English](README.md) · 简体中文

[![CI](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Ant1mage/codex-agent-relay?label=release)](https://github.com/Ant1mage/codex-agent-relay/releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex 决定委派什么，Relay 负责启动和监管外部 Agent。**

Relay 是面向 OpenAI Codex 的本地 MCP Agent 运行时与桌面控制面板，可将编码、
研究和代码审查任务委派给 DeepSeek Harness 等 Agent CLI。它管理 Agent 配置、
运行时发现、权限策略、worker 进程和结果；任务规划与后续编排仍由 Codex 负责。

Relay 不是 AI IDE，也不创建第二套 Agent loop；它将 Codex 与本机已有的 Agent
运行时连接起来。

## Relay 可以做什么

- 为 Codex 提供 `$relay` skill 和 MCP 工具，用于委派范围明确的任务。
- 在本机启动并监管外部 Agent CLI。
- 通过可复用的 Agent 配置保存模型、推理强度和能力设置。
- 为只读、建议和写入任务应用工作区及会话策略。
- 在本地 inspector 和控制面板中查看运行状态、worker 输出和文件变更。

## 项目状态

当前版本为 **v0.1.0**。已验证的端到端组合是
**Apple 芯片 Mac + Codex + DeepSeek Harness**。Relay 使用
Rust 实现，包含 Tauri 菜单栏应用和本地 daemon。

| Agent 运行时 | CLI | Adapter | 状态 |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | 已支持并完成端到端验证 |
| Kimi Code | `kimi` | `kimi-code` | 实验性，尚未完成端到端验证 |
| Antigravity CLI | `agy` | `antigravity-cli` | 实验性，尚未完成端到端验证 |
| Z.ai / GLM | `zai-cli` | `zai-cli` | 实验性，尚未完成端到端验证 |

## 快速开始

1. 安装并登录 Codex，以及你计划运行的 Agent CLI。
2. 启动 Relay，在 **Runtimes** 中确认 CLI 已被发现；如有需要，可手动登记可执行文件。
3. 在 **Agents** 中创建配置，选择运行时、模型、推理强度和能力。
4. 在 **Codex** 页面安装 Relay 插件与 MCP 集成。
5. 告诉 Codex 委派一个边界清晰的任务：

   ```text
   $relay 用 DeepSeek 审查当前实现并报告潜在问题。
   ```

Codex 从 Relay 读取 worker 结果并进行审查，决定是否继续编排，再向用户整理最终答复。

## 工作方式

```text
Codex + $relay skill
        │ MCP
        ▼
   relay-mcp ── 本地 HTTP ──► relayd ──► runtime adapter ──► Agent CLI
                                  │                              │
                                  └──── 运行事件与结果 ◄─────────┘
```

Codex 负责任务规划、委派和结果审查；Relay 负责运行时执行、策略、进程生命周期与
本地可观测性。Agent CLI 保留自己的模型调用和内部 Agent 行为。

## 构建与测试

Relay 当前面向 Apple 芯片 Mac。源码构建需要 stable Rust、
`wasm32-unknown-unknown` target、Trunk 和 Tauri CLI。

```bash
cargo build --workspace
cargo test --workspace
./scripts/dev.sh
```

完整的桌面构建、打包和发布要求见[开发文档](docs/development.md)。

## 文档

- [架构](docs/architecture.md) — 组件、委派链路、策略与安全边界。
- [Codex 集成](docs/codex-integration.md) — 插件、MCP 工具、安装与故障排查。
- [开发指南](docs/development.md) — 构建、测试、运行与打包。

## 许可

MIT，详见 [LICENSE](LICENSE)。
