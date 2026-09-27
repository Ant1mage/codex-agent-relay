# Relay — 面向 OpenAI Codex 的外部 Agent 运行时

[English](README.md) · [简体中文](README.zh-CN.md)

[![CI](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex 决定委派什么，Relay 负责怎么运行。**

Relay 是 Codex 的本地运行时与控制平面。Codex 把一个有边界的任务交给外部编码
Agent，Relay 负责解析运行时、执行策略、启动并监管 worker 进程，并把结果送回同
一个 Codex 会话。

Relay 不是另一个 AI IDE：没有编辑器、没有聊天窗口、没有自己的 agent loop。
规划、审查与编排始终留在 Codex。

## 技术栈

| 层 | 实现 |
| --- | --- |
| Desktop | Tauri 2（Rust），仅菜单栏 |
| UI | Leptos + Trunk → WebAssembly |
| Relay Core | Rust（`crates/relay-core`） |
| 守护进程（`relayd`） | Rust + Axum（HTTP/SSE） |
| MCP server（`relay-mcp`） | Rust + rmcp（stdio） |
| 存储 | SQLite（`rusqlite`） |
| 配置 | TOML（`~/.relay/config.toml`） |
| Worker 运行时 | 外部 CLI，独立进程 |

整个仓库不含 Node.js、Electron、Chromium、npm 或 Vite —— 运行时和构建时都没有。

## 当前 MVP

已验证的端到端链路是 **macOS + Codex + DeepSeek**：

| | |
| --- | --- |
| 平台 | macOS，Apple silicon |
| Host | Codex（CLI 或 IDE 扩展） |
| Worker 运行时 | DeepSeek Harness（`dsh`），已安装并已登录 |

Kimi、Z.ai、Antigravity 的 adapter 已在仓库中并有解析 fixture，但尚未端到端验证。

## 构建与运行

需要：macOS Apple silicon、Rust stable，以及用于 UI 的
[Trunk](https://trunkrs.dev)。

```bash
cargo build --workspace                 # core、daemon、MCP server、desktop
cargo test --workspace                  # 单元测试 + 委派端到端测试
cargo install trunk --locked
(cd apps/relay-desktop/ui && trunk build --release)

cargo run -p relayd                     # 只启动 daemon（127.0.0.1:7352）
(cd apps/relay-desktop && cargo tauri dev)   # 菜单栏应用（自行启动 relayd）
```

`cargo tauri build` 产出 `Relay.app` 与 DMG。签名、公证等完整说明见
[docs/development.md](docs/development.md)。

## 快速开始

1. **启动 Relay**：菜单栏出现图标，daemon 在其后运行。`relayd` 会打印带一次性
   token 的 inspector 地址。
2. **检查 Runtimes**：菜单栏 → Runtimes。检测只负责*发现* CLI，不会替你创建
   agent。若 `dsh` 不在常见位置，可在同一页手动登记可执行文件。
3. **创建 Agent**：控制面板 → Agents → New agent。选择 runtime、选择 CLI 自己
   公布的模型与推理档位、设置能力，然后保存。
4. **安装到 Codex**：控制面板 → Codex → Install。Relay 会生成本地插件
   marketplace，把 `relay` MCP server 指向自己的 Rust 二进制，并显示五项检查。
5. **在 Codex 中委派**。

```text
$relay 用 DeepSeek 审查当前实现并报告潜在问题。
```

## 工作方式

```text
Codex ──$relay / MCP──► relay-mcp ──► RunController ──► adapter ──► dsh
  ▲                          │                                       │
  └──── result.summary ──────┘◄──────── Relay 事件流 ◄───────────────┘

relayd     ──► 配置、运行时扫描、Codex 集成、HTTP/SSE
菜单栏      ──► 状态、快捷操作、配置
inspector  ◄── 执行状态、事件、日志
```

Relay 负责运行时执行、策略、生命周期与可观测性；Codex 负责规划与审查。原生 CLI
保留自己的 agent loop，Relay 不会重新实现一个。

完整架构见 [docs/architecture.md](docs/architecture.md)。

## 支持的运行时

| 运行时 | CLI | Adapter | 状态 |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | **支持 / MVP** |
| Kimi Code | `kimi` | `kimi-code` | 计划中 |
| Antigravity CLI | `agy` | `antigravity-cli` | 计划中 |
| Z.ai / GLM | `zai-cli` | `zai-cli` | 计划中 |

不同 `dsh` 版本暴露的能力不同：带 `--json` 流的版本走结构化模式，不带的走有界的
纯文本模式。Relay 会如实报告检测到的能力，而不是假设。

## 文档

- [docs/architecture.md](docs/architecture.md) — crate、进程、数据模型、事件。
- [docs/codex-integration.md](docs/codex-integration.md) — Relay 向 Codex 安装什么、检查与修复。
- [docs/development.md](docs/development.md) — 构建、测试、发布、签名。

## 许可

基于 [MIT License](LICENSE) 发布。
