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

当前版本为 **v0.1.2**。已在 **Apple 芯片 Mac + Codex** 上验证
**DeepSeek Harness**、**Grok Build** 和 **Antigravity CLI** 的端到端派发；
其中 Antigravity 的在线验证范围为 echo/read-file 与会话
`run`/`resume`/`cancel`/`accept` 流程（详见下方范围说明）。
Relay 使用 Rust 实现，包含 Tauri 菜单栏应用和本地 daemon。

Antigravity CLI 接入属于当前开发分支，**尚未包含在已发布的 v0.1.2 DMG 中**：
下载 v0.1.2 还不会获得此接入。

| Agent 运行时 | CLI | Adapter | 状态 |
| --- | --- | --- | --- |
| DeepSeek Harness | `dsh` | `deepseek-harness` | 已支持并完成端到端验证 |
| Kimi Code | `kimi` | `kimi-code` | 实验性，尚未完成端到端验证 |
| Antigravity CLI | `agy` | `antigravity-cli` | 当前开发分支已支持（仅写入模式）；范围见下方说明 |
| Z.ai / GLM | `zai-cli` | `zai-cli` | 实验性，尚未完成端到端验证 |
| Grok Build | `grok` | `grok-cli` | 已支持并完成端到端验证 |

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
Relay 不添加 `--dangerously-skip-permissions` 或任何绕过参数，也绝不自动批准
被拒绝的工具。按官方 headless 文档 *Permissions in headless mode*：权限默认继承
用户 settings；headless 无法获取确认时可能 soft-denied，但进程仍可能 exit 0。
在本轮验证所用主机上，`request-review` 策略拒绝了测试写入——临时目录与仓库
`target/` 下的路径均被拒；shell 命令同样可能需要既有 allow 规则，实测的真实
`run_command` 样本即被拒绝。拒绝**不会**触发自动重新登录，也不会自动绕过权限。
由于没有在线完成任何写入或 shell 编码任务，本接入不声称整个工具生命周期均已
验证。工具被拒且最终 `SUCCESS` 回复为空时，不再误报为任务完成；该修复由实测
拒绝样本与离线回归测试覆盖。echo 与 read-file 任务，以及当前会话的
`run`/`resume`/`cancel`/`accept` 流程已端到端通过。由于 headless Antigravity
无法强制只读工作区，只读与建议模式会被诚实拒绝，仅提供写入模式。不提供运行中
发送消息；CLI 报告子 Agent 步骤时会映射为 child 事件，但子 Agent 生命周期
尚未完成端到端验证。实测范围与保留限制见
[Antigravity 验证报告](docs/reports/antigravity-cli-2026-09-28.md)。

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

## 许可

MIT，详见 [LICENSE](LICENSE)。
