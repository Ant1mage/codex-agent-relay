# Relay — OpenAI Codex 本地 MCP Agent 委派运行时

[English](README.md) · 简体中文

[![CI](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml/badge.svg)](https://github.com/Ant1mage/codex-agent-relay/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Ant1mage/codex-agent-relay?label=release)](https://github.com/Ant1mage/codex-agent-relay/releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20arm64-black)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Codex 决定委派什么，Relay 负责启动和监管外部 Agent。**

Relay 是面向 OpenAI Codex 的本地 MCP Agent 运行时与桌面控制面板，可将编码、
研究和代码审查任务委派给 DeepSeek Harness、Grok Build 和 Antigravity CLI。
它管理 Agent 配置、运行时发现、权限策略、worker 进程和结果；任务规划与后续
编排仍由 Codex 负责。

Relay 不是 AI IDE，也不创建第二套 Agent loop；它将 Codex 与本机已有的 Agent
运行时连接起来。

## Relay 可以做什么

- 为 Codex 提供 `$relay` skill 和 MCP 工具，用于委派范围明确的任务。
- 在本机启动并监管外部 Agent CLI。
- 为不同运行时提供统一的 Codex 工具，用于派发、等待、审查、继续和取消任务
  （具体操作取决于运行时能力）。
- 通过可复用的 Agent 配置保存运行时、模型、推理强度和能力设置。
- 按全局、工作区和会话应用访问、命令、网络与并发策略。
- 将 worker 更新持续显示在会话时间线中；同一会话的多次任务及历史都会保留。
- 在本地 inspector 中查看最终答复、可观测的工具过程、文件变更和原始事件；
  通过菜单栏应用管理配置、运行时、策略与 Codex 集成。

## 项目状态

当前版本为 **v0.1.3**。已在 **Apple 芯片 Mac + Codex** 上验证
**DeepSeek Harness**、**Grok Build** 和 **Antigravity CLI** 的端到端派发；
Antigravity 文件写入、白名单命令执行及会话
`run`/`resume`/`cancel`/`accept` 流程均已通过验证（详见下方范围说明）。
Relay 使用 Rust 实现，包含 Tauri 菜单栏应用和本地 daemon。

Antigravity CLI 接入和统一桌面 UI 已包含在 **v0.1.3** 中。

| Agent 运行时 | CLI | Adapter | 状态 |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | 已支持并完成端到端验证 |
| Antigravity CLI | `agy` | `antigravity-cli` | 已支持；文件写入及白名单命令已验证 |
| Grok Build | `grok` | `grok-cli` | 已支持并完成端到端验证 |

在运行时支持的情况下，Relay 复用 CLI 已有的登录和模型目录。Agent 配置用于
选择运行时及其支持的模型和推理选项；Relay 不会替代 provider 自己的 Agent loop。

Grok 复用 CLI 已有的登录。模型和推理强度取自 `grok models` 及同版本的
原生模型元数据。macOS 下，未显式设置代理环境变量时，Relay 会把已启用的
系统代理传给 Grok。只读和建议模式使用只读沙箱，写入模式使用工作区沙箱。
子进程代理排除项始终包含本地回环地址，Relay 自身的本地连接直连，更新检查
使用系统代理。支持继续会话和取消任务。运行中发送消息和子 Agent 暂不提供。
实测范围见 [Grok 验证报告](docs/reports/grok-cli-2026-09-28.md)。

Antigravity CLI（`agy`）通过其原生 headless 流接入：任务作为一条私有
`user` 事件写入 stdin，CLI 的 `init`、`step_update`、`result` NDJSON 帧
转换为 Relay 事件；`agent_response.text_delta` 增量统一走 Relay 的 worker 文本
契约（每个 `agent_response` 步骤一条助手消息，`result` 帧仍是权威结果），
与 DeepSeek Harness、Grok 共用同一条展示路径。模型 ID 严格解析 `agy models`
的 `<id>\t<label>` 两列表格，通过 CLI 自身的 `--model` 应用；reasoning 取值
严格采用 CLI help 枚举的 `--effort`（`low|medium|high|max`），绝不从模型 ID
后缀推断。Relay 先等待并校验 `init` 的会话 UUID，通过后才把任务写入 stdin：
缺少 UUID、UUID 非规范、无法识别的旧会话 ID、或握手超时都会失败，此时关闭
stdin、不送达任务并回收子进程。任务送达后若会话 ID 发生变化，只会将该轮判为
失败，无法撤销已发送或已执行的内容。继续会话使用 `--conversation` 和原生
UUID。工具步骤会暴露真实文件路径（CLI 发布的 `AbsolutePath` 与 `TargetFile`
归一化为 `path`，供 Changes 视图使用）和命令（CLI 发布的 `CommandLine`
归一化为 `command`）。支持取消。真实 Codex 会话已通过 Relay 跑通原生
`list`/`run`/`wait`/`resume`/`accept`/`cancel` 全流程：续接轮次保持同一原生
会话，无效模型会失败而非误报成功，取消会回收原生进程。

**权限与验证范围。** 原生 headless 运行继承用户已有的 Antigravity 权限设置。
Relay 的 Write 任务使用 `--mode accept-edits`，允许文件编辑确认，但不授予
shell 或网络权限；不添加 `--dangerously-skip-permissions`，也不会自动批准
被拒绝的工具。经 Relay 创建文件、执行本机已有精确白名单中的命令均已通过端到端
验证；未获准的 `pwd` 按预期被拒绝。Headless 权限拒绝后 CLI 仍可能返回 exit 0，
因此工具失败且最终结果为空时 Relay 会判为失败。由于 headless Antigravity
无法强制只读工作区，只读与建议模式会被诚实拒绝，仅提供写入模式。不提供运行中
发送消息；CLI 报告子 Agent 步骤时会映射为 child 事件，但子 Agent 生命周期
尚未完成端到端验证。实测范围与保留限制见
[Antigravity 验证报告](docs/reports/antigravity-cli-2026-09-28.md)。

## 快速开始

1. 从[最新 GitHub Release](https://github.com/Ant1mage/codex-agent-relay/releases/latest)
   下载 Apple 芯片 Mac 的 DMG，将 Relay 拖到“应用程序”并启动。应用采用临时签名，
   尚未公证；首次启动时 macOS 可能要求在系统设置中批准。
2. 安装并登录 Codex，以及你计划使用的 Agent CLI。
3. 在 Relay 的 **Runtimes** 页面确认 CLI 已发现；在 **Agents** 中创建配置，选择
   运行时、模型、推理强度和能力。
4. 在 Relay 的 **Codex** 页面安装插件与 MCP 集成。状态检查会显示 Codex、MCP、
   skill、plugin 和 hooks 是否已安装且为最新版本。
5. 告诉 Codex 委派一个边界清晰的任务：

   ```text
   $relay 用 DeepSeek 审查当前实现并报告潜在问题。
   ```

Codex 从 Relay 读取 worker 结果并进行审查，决定是否继续编排，再向用户整理最终答复。

### 一次典型的委派

```text
你 ── 任务请求 ──► Codex
                     │ $relay：任务范围 + Agent 配置
                     ▼
                  Relay ──► 本机 Agent CLI
                     ▲             │
                     └── 进度、工具事件、最终结果
                     │
                Codex 审查结果
                     │
                     ▼
                  最终答复
```

每次委派都会作为独立任务保存在发起它的 Codex 会话中。Inspector 可用于跟踪进度、
审查答复和文件变更，并查看任务成功、失败或被拒绝的原因。Codex 决定是否接受结果，
以及是否继续派发后续任务。

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
cargo clippy --workspace --all-targets -- -D warnings
```

每次打包指定一种模式：

```bash
./scripts/package-app.sh --mode dev       # debug 版：Relay Dev.app
./scripts/package-app.sh --mode release   # release 版：Relay.app 和 DMG
```

完整的桌面构建、打包和发布要求见[开发文档](docs/development.md)。

## 文档

- [架构](docs/architecture.md) — 组件、委派链路、策略与安全边界。
- [Codex 集成](docs/codex-integration.md) — 插件、MCP 工具、安装与故障排查。
- [开发指南](docs/development.md) — 构建、测试、运行与打包。

## 本地数据与安全

Relay 默认将配置、事件历史和 Codex 集成保存在 `~/.relay`。Daemon 仅监听本机回环
地址，API 使用每次启动时生成的 token 保护。运行时权限由各 CLI 自身能力决定：Relay
无法强制的访问模式会直接拒绝，不会假称沙箱已启用。例如 Antigravity 的 Write 任务
通过 `--mode accept-edits` 确认文件编辑，shell 命令仍遵循 Antigravity 自己的权限规则。

## 许可

MIT，详见 [LICENSE](LICENSE)。
