# Relay

[English](README.md) · [简体中文](README.zh-CN.md)

[![CI](https://github.com/Ant1mage/relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/relay/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex 决定委派什么，Relay 决定它怎么运行。**

Relay 是 Codex 的本地运行时与控制面。Codex 把一个边界清晰的任务委派给外部
Coding Agent；Relay 负责解析运行时、落实策略、启动并监管 worker 进程，再把结果
回传到同一条 Codex 会话。你始终留在 Codex 里，不用在多个 CLI 之间手工搬运任务。

Relay 不是另一个 AI IDE：它没有编辑器、没有聊天窗口，也没有自己的 Agent Loop。
规划、验收与编排始终属于 Codex。

## 当前 MVP

当前唯一完成端到端验证的组合是 **macOS + Codex + DeepSeek**：

| | |
| --- | --- |
| 平台 | macOS，Apple silicon |
| Host | Codex（CLI 或 IDE 扩展） |
| Worker 运行时 | DeepSeek Harness（`dsh`），已安装并完成认证 |

其他运行时的 Adapter 代码已在仓库中，但尚未完成端到端验证，见
[支持的运行时](#支持的运行时)。

## 功能

- **从 Codex 委派** —— 一个 `$relay` skill 加一个本地 MCP server；不用复制任务，
  也不用额外开终端。
- **运行时探测** —— 发现本机已安装的 CLI 及其版本与健康状态，模型 / reasoning
  选项只列出 CLI 自己报告的值。
- **Agent Profile** —— 把运行时、能力与 instructions 绑定一次
  （`deepseek-code`、`deepseek-research`……），之后在 Codex 里反复使用。
- **策略** —— access mode（`read_only` / `propose` / `write`）、全局与按
  workspace 的限制、run 与 writer 的并发上限、并行 writer 的 worktree 隔离。
- **生命周期** —— 一次委派是一条 Run，包含若干 Step 与 iteration；支持取消、
  继续，并以 `awaiting_host` 把结果交回 Codex 验收。
- **可观测性** —— append-only 本地事件日志、菜单栏实时状态，以及只读的 Web
  Inspector（会话、Run、Console 输出与文件改动）。
- **本地优先** —— 只绑定回环地址，没有账号体系，没有遥测；所有状态都在
  `~/.relay`。

## 快速开始

环境要求：Apple silicon 上的 macOS、[Codex](https://github.com/openai/codex)，
以及已安装并完成认证的 DeepSeek CLI（`dsh`）。

**1. 安装并运行 Relay**

从 [Releases](https://github.com/Ant1mage/relay/releases) 下载
`Relay-<version>-arm64.dmg`，或者直接构建：

```bash
pnpm install
pnpm pack:mac      # → dist/Relay-<version>-arm64.dmg
```

Relay 是菜单栏应用：启动一次后图标常驻菜单栏，本地 daemon 在后台运行。

**2. 让 Relay 发现 DeepSeek**

```bash
dsh --version
```

菜单栏 → **运行时** 会显示探测到的 DeepSeek 运行时、可执行文件、版本与健康状态。
如果 `dsh` 装在非常规位置，可以在同一页手动登记可执行文件。

**3. 配置一个 DeepSeek Agent**

菜单栏 → **打开配置面板…**（⌘,）→ **智能体** → 新建 Profile：选择 DeepSeek
运行时，选择 CLI 报告的 model 与 reasoning 取值，设置 capabilities 与可选的
instructions，然后启用。

**4. 安装 Codex 集成**

配置面板 → **Codex 集成** → **安装**。Relay 会生成一个本地插件（skill + hooks）
并把 `relay` MCP server 注册到 Codex；五项检查应当全部变绿。若有检查未通过，见
[docs/codex-integration.md](docs/codex-integration.md)。

**5. 在 Codex 里使用**

```text
$relay use DeepSeek to review the current implementation and report potential issues.
```

Codex 继续负责规划与验收；Relay 按策略运行 worker，并把结果回传到会话里。

## 工作原理

```text
Codex  ──  $relay / MCP  ──►  Relay Core  ──►  Runtime adapter  ──►  dsh (DeepSeek CLI)
  ▲                                │                                     │
  └────  结果、改动、状态 ─────────┘◄────────────────────────────────────┘

菜单栏          ──►  配置与运行时控制
Web Inspector  ◄──  执行状态、事件、日志
```

Codex 拥有规划与编排；Relay 拥有运行时执行、策略、生命周期与可观测性。每次委派
都是一条 Run，包含 Step 与 WorkerSession，所有可观察动作都会追加到本地事件日志，
菜单栏与 Inspector 都从这份日志读取。原厂 CLI 保留自己的 Agent Loop，Relay 不会
再实现一套。

完整架构见 [docs/architecture.md](docs/architecture.md)。

## 支持的运行时

| 运行时 | CLI | Adapter | 状态 |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `@relay/adapter-deepseek` | **已支持 / MVP** |
| Kimi Code | `kimi` | `@relay/adapter-kimi` | Planned |
| Antigravity CLI | `agy` | `@relay/adapter-antigravity` | Planned |
| Grok Build | `grok` | — | Planned |
| Z.ai / GLM | `zai-cli` | `@relay/adapter-zai` | Planned |

只有 DeepSeek 完成了端到端验证。Kimi、Antigravity、Z.ai 的 Adapter 具备探测、
编译与 parser fixture 测试，但不属于当前 MVP 的支持范围；Grok Build 尚无
Adapter。Antigravity CLI 已取代 Gemini CLI，因此 Google 只列最新的一个。

## 开发

需要 Node.js 24 与 pnpm 10。

```bash
pnpm install
pnpm typecheck
pnpm test
pnpm build
```

`pnpm dev` 会启动一个开发会话（daemon + 菜单栏）；加 `--web` 会同时启动
Inspector 的开发服务器。

## 文档

- [docs/architecture.md](docs/architecture.md) —— 进程、职责边界、数据模型与事件。
- [docs/codex-integration.md](docs/codex-integration.md) —— Relay 装进 Codex 的
  内容、会话生命周期、修复与排查。

## 许可证

基于 [MIT License](LICENSE) 发布。
