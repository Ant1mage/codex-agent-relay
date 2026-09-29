# Antigravity CLI 接入与控制台重复报告修复（2026-09-29 验证）

## 2026-09-29 补充：headless 文件写入端到端修复

本机 `agy 1.2.13` 的 headless 默认执行模式是 `default`，文件编辑需要确认 diff；无交互界面时真实 `write_to_file` 返回 `ERROR`，CLI 却继续给出 `result SUCCESS`、空答复和退出码 0，目标文件没有创建。此前把这类拒绝归因为用户 permission 策略过于笼统；对照实测确认，文件编辑应由 `--mode accept-edits` 处理。shell 等命令的权限仍由用户的 CLI permission 规则决定。

修复：Relay 以 Write 模式启动 Antigravity 时传入 `--mode accept-edits`。该 CLI 模式免除文件编辑/创建的交互 diff 确认；shell 命令和工作区外文件访问仍走 CLI permission 规则。启动前 Relay 还要求本机 `agy --help` 明确列出 `--mode` 和 `accept-edits`，不支持时在派发前给出明确的 `ADAPTER_FAILURE`，避免静默回退到会拒绝文件编辑的 headless 默认模式。没有添加 `--dangerously-skip-permissions`，没有改写用户权限设置。

验证：

- 本机 CLI 1.2.13 原生 NDJSON 对照：默认模式下 `write_to_file` 是 `ERROR`、`result SUCCESS`、exit 0，文件不存在；加 `--mode accept-edits` 后同一类任务的工具状态为 `DONE`，result 为 `SUCCESS`、exit 0，文件内容与指定 sentinel 完全一致。
- 最新源码构建的临时 Relay daemon + 临时 `Antigravity E2E` Write profile + 真实 HTTP `/api/runs`：写入 run `a84e38f1-42cf-4f6d-bfd2-060f26ddeda0` 产生 `tool/edit`、`tool/result(DONE)`、一条 `worker/message(final)` 和 `worker/completed`；`target/antigravity-cli-e2e/relay-write-final.txt` 内容精确匹配要求，review 后 accepted/completed。
- 同一临时 daemon 另跑 `pwd`：工具被原生权限策略拒绝，Relay 以 `worker/failed` 结束，并报告 Antigravity tool permission denial。证明 `accept-edits` 没有开放 shell 命令权限。
- 命令执行权限复核：同一隔离 daemon 临时启用 Relay profile 的 `execute_commands`，经真实 `/api/runs` 再运行本机既有精确白名单命令 `cargo test -p relay-adapters --lib antigravity`。事件流记录 `tool/command` 与 `tool/result`，worker 由 `/Users/ant1mage/.local/bin/agy` 启动，退出码 0，25 项测试通过；run 经 host accept 后为 `completed`。测试后关闭隔离 daemon 并恢复临时 profile 的 command 开关；没有新增 Antigravity permission grant 或更改正式 Relay 配置。说明 headless CLI 可以执行已授权命令，之前 `pwd` 的失败是该命令未获授权，不是命令执行普遍不可用。
- 工作区外写入边界：原生 CLI 一次性 `/tmp` workspace 探测和 Relay run `f0258bdc-9158-4f43-b358-8538ad719a9f` 都以 `--mode accept-edits` 尝试写入工作区外路径，`write_to_file` 为 `ERROR`、目标未创建；Relay 将后者记为 `worker/failed`。本机 `allowNonWorkspaceAccess` 未设置（官方默认 `false`）。这是当前 CLI 配置提供的边界，Relay adapter 自身 `enforcement.workspace` 仍为 `false`，所以不声称 Relay 可以覆盖用户显式放宽的 Antigravity 文件访问设置。
- 公开汇总证据保存在 `target/antigravity-cli-e2e/e2e-summary.json`（ignored）；临时 daemon 使用独立 `RELAY_HOME`，没有改正式 Relay 配置。

依据：Antigravity 官方 [headless 权限文档](https://www.antigravity.google/docs/cli/headless/) 说明 headless 不能显示确认弹窗，受限工具会被 soft-deny；官方 [execution mode 文档](https://antigravity.google/docs/cli/modes/) 说明 `accept-edits` 自动批准文件编辑，而 command 权限仍受 `/permissions` 规则控制。本机实际 CLI/Relay 运行与以上行为一致。

历史初始实现位于 `codex/antigravity-cli`（基于本地 `main` 的 `5b7d64c`）。
本次 1.2.13 headless 修复在当前 `feat/unified-desktop-ui` 工作树上完成，
基线为 `07508d0`。
实现与离线测试完成于 2026-09-28，端到端验证完成于 2026-09-29
（用户本地 Asia/Shanghai 日期；证据时间戳为 UTC `2026-09-28T16:3xZ`）。
这段历史报告最初记录于发布前，所述“不提交、不推送、不改版本号”是当时的
状态；后续已在本分支完成验证并准备随 Relay `v0.1.3` 发布。
ZCode 保留在 `codex/zcode-glm` 的 `5705bce`，未切换、未改动。

最后一轮实测 review 修复：`TargetFile`/`CommandLine` 参数归一化，以及“终态工具
ERROR + 空 SUCCESS 被判为完成”的空 SUCCESS 修复；由 subagent 改产品代码，主
Agent 仍只规划/review。

上面“初始实现轮”记录的是 2026-09-28 当时的验证范围：不在沙箱内做
Antigravity 登录或鉴权，不读取凭证，也没有运行真实 `agy` 的
`models`/`print`/`stream` 或在线测试。真实 CLI 样本由主 Agent 在本机正常环境
采集，公开脱敏证据位于
`target/antigravity-host-review/`；当前会话端到端证据位于
`target/antigravity-e2e-review/`。产品代码、离线测试和文档全部在本仓库完成。
不新增 `--dangerously-skip-permissions`，不自动更改或绕过用户既有 CLI 权限，
仅复用本机登录。

## 一、控制台重复最终报告修复

调研 run `cdb52f90-9ea8-444e-b370-89df63632b4e` 的 `seq4`
`worker/message(kind=final,text)` 与 `seq5` `worker/completed(summary)` 正文
完全一致（均 13854 字符）。`apps/relay-desktop/ui/src/format.rs` 之前把两者
都完整展开，控制台重复显示同一份长回复。

修复方式（最小且不丢信息）：

- 仅当 `worker/completed` 的 `summary` 与**同一 worker** 之前某条
  `worker/message`（`kind=final`）的 `text` **逐字节相同**时，才把该完成事件
  的 Console 行改显示为简短状态 `Completed`。
- 同一 worker 的多个轮次各自保留；不同 worker 的相同文本互不抑制；文本不同
  时完成事件保留自己的 summary；`kind=delta` 等非最终消息不参与配对。
- 原始事件不受影响：`visible_events`（Raw 视图）仍包含 `worker/message` 与
  `worker/completed` 两条完整记录，`native_event` 也未改动；MCP 结果与工具
  结果事件完全不参与该逻辑。

新增回归测试见 `format.rs` 的 `tests` 模块：

- 复现本条 run 的正例（13854 字符只出现一次，完成行显示 `Completed`，Raw 保留两条）。
- 反例：不同 worker、文本不同、同一 worker 多轮、孤立完成事件、delta 消息。

## 二、AGY 适配器校正

`crates/relay-adapters/src/antigravity.rs` 及 `antigravity/catalog.rs`。

### 输入 / 参数

- 启动参数改为 `--input-format stream-json --output-format stream-json`，
  不再携带 `-p` prompt（会被忽略）。
- 任务与 profile 指令经现有 `instructions::enveloped` 封装后，作为一条私有
  NDJSON `{"event":"user","message":{"content":"…"}}` 写入 stdin，随后关闭
  stdin；任务不进进程参数表。
- 输入门控（本轮 review 修复）：`crates/relay-adapters/src/cli.rs` 的
  `StreamSpec` 新增可选 `stdin_gate`。Antigravity 启动后先等待 CLI 在没有任何
  user 输入前发出的 `init`（本机 `host-stdin-handshake.json` 实测
  `anyUserInputSent=false`、先观测到 init），校验其为 UUID 且与请求的续接 ID
  一致后，才把任务写入 stdin。缺失 init、非 UUID、ID 不一致或等待超时（默认
  30s）都会关闭 stdin、SIGTERM 终止并回收自己启动的进程，任务绝不送达；
  门控期间进程已按 worker key 注册，首条输出前仍可取消。其它适配器
  `stdin_gate: None`，行为不变。`fake` CLI 已改成与实测一致：先打印 init，
  再读取 stdin。
- `step_update` 的 `agent_response.text_delta` 先累积，最终回复以 `result.response`
  为准，只发一条 `worker/message(kind=final)`，不再逐字产生碎片。

### 事件与完成判定

- 已按本机样本校正 `init`（`conversation_id`）、`step_update`
  （`conversation_id`/`step_index`/`state`/`step_type`/`text_delta`/`usage`）、
  `result`（`conversation_id`/`status`/`response`/`duration_seconds`/`num_turns`/`usage`）。
- 工具步骤归一化为 `tool/read|search|edit|command|result`。`host-tools.ndjson`
  实测 `view_file` 只以 `tool_info.parameters.AbsolutePath` 发布路径；
  `host-write-summary.json` / `host-workspace-write-summary.json` 实测
  `write_to_file` 只以 `TargetFile` 发布路径。本轮把 `AbsolutePath` 与
  `TargetFile` 都归一化为规范 `path`（Changes/console 读取的键）。命令面按官方
  headless 文档 *Tool calls in the stream* 的真实 `run_command` 样本
  `{"CommandLine":"echo hello_headless_demo"}` 归一化 `CommandLine` 为
  `command`；原始 `parameters` 对象原样保留，不编造未验证字段。
- `usage` 只保留公开 token 计数字段；`result` 的 native 负载也做同样的公开字段
  收敛，provider 签名等私有字段不进入 Relay 事件。
- 成功条件：`result.status == SUCCESS` 且进程退出码为 0、无错误/漂移，且
  `spawn_error.is_none()`（本轮补上：读管道失败后即使出现 SUCCESS + exit0 也
  判失败）；缺失 `result`、非零退出、错误状态、截断或 malformed JSON 一律失败。
- 空 SUCCESS 修复（本轮）：当 CLI 已有记录在案的**终态工具 ERROR（公开
  `error` 对象）**、且最终 `result.status == SUCCESS` 的 `response` 与合并文本
  均为空时，不再判为完成，而给出对应的 `WorkerFailed` 原因。工具权限拒绝
  （error 含 `permission` + `denied`）与其它工具错误各有独立措辞，明确区别于
  登录失败；失败原因不引用原始错误文本，凭证/私有输出不泄漏。终态工具失败的
  `ToolResult` 事件照常保留。该判定只在“无任何回复”时生效：工具错误后重试
  成功并产出实际非空结果仍是成功；已完成工具且无错误的空回复不被改判；后来的
  干净工具终态会覆盖更早的工具失败（可恢复）。
- 会话一致性：`init` 必须存在、是 UUID 且与请求的续接 ID 相同；任何
  `step_update`/`result` 中途切换 `conversation_id` 均判失败；一旦拒绝，
  后续帧不能把错误 ID 恢复成合法会话。该中途检查只判本轮失败，不撤销任务
  送达后已写入 stdin / 已执行的内容；任务不送达仅适用于 `init` 门控阶段。
- 取消：`result.status` 为 `CANCELED/CANCELLED/INTERRUPTED`，或进程确实被
  `SIGTERM/SIGINT/SIGKILL` 杀死，或退出码 130/143，才判定为 cancelled。
  本机 `host-cancel-summary.json` 实测：SIGTERM 后真实 CLI 约 1.056s 自行收尾，
  输出 `result ERROR` + exit1，并非信号终止。因此普通 `ERROR` 不会被误判为
  取消；用户取消的权威状态来自核心 RunController 的 `cancel_requested`
  （daemon 实测取消后应为 cancelled 且进程回收）。Relay 因握手失败而回收进程时，
  已设置显式错误，同样不会读成取消。
- 进程在 spawn 后立即以 worker key 注册，首条输出前即可取消；只终止 Relay
  自己启动的进程（沿用 `ProcessSupervisor`）。

### 模型 / effort

- 只探测无 flag 的 `agy models`（已知 `--output-format json` 返回 exit1，不假设
  JSON 支持），严格解析两列 `<id>\t<label>` 目录：非 `id<TAB>label` 的行
  （横幅、错误、日志、空列）一律丢弃，目录失败也不会把任意文本当成 model。
- 仅当 `agy --help` 真实声明 `--model` 时才提供目录中的模型，并通过该 flag
  应用。effort 只取 `--effort` 在真实 help（`host-help.txt`，输出在 stderr、
  exit0）中枚举的四个值 `low|medium|high|max`；已删除从模型 ID
  `-low/-medium/-high` 后缀推断 effort 的回退及其文档声明。
- `report_options` 只使用注册运行时指定的可执行文件（`probe_target`），尊重
  手工登记路径。

### 继续会话

- 使用 `--conversation <uuid>`，启动前校验原生 UUID；任务在 `init` 校验通过前
  不会写入 stdin（见“输入门控”）。`init` 返回的 conversation 必须存在、是 UUID
  且与请求一致，否则该轮失败并回收子进程。`host-missing-conversation.ndjson`
  实测无效 UUID 会被 CLI 静默当成新会话执行，本轮门控确保该任务绝不送达。
- 不扩展 core 的 `ResumeInput` 契约；继续会话沿用原生会话自身设置，不强行
  附加 model/reasoning。

### 访问模式与隔离

- `--sandbox` 不是 Relay 的只读开关。Write run 使用经过当前 CLI 实测的
  `--mode accept-edits`；ReadOnly/Propose 仍无法被强制，直接以 `ADAPTER_FAILURE`
  诚实拒绝，不再虚报隔离。
- `enforcement` 三项保持 `false`；`send:false` 保留；`child_sessions` 仅在 CLI
  报告 `subagent_info` 时映射 child 事件。

### 权限策略与空 SUCCESS 修复（本轮实测 review；文件编辑行为见本报告顶部补充）

- Relay 复用本机 CLI 登录，不添加 `--dangerously-skip-permissions`。Write run
  明确覆盖执行模式为 `accept-edits`，自动批准当前 run 的文件编辑；这不是全工具
  permission grant。shell 命令仍遵循用户已有 permission 规则，不自动重登或绕过。
- 官方出处：<https://antigravity.google/docs/cli/headless/> 的 *Permissions in
  headless mode* —— 权限默认继承用户 settings；headless 需要确认但无法获取时，
  soft-denied 仍可能 exit 0。主 Agent 已在本机正常环境成功读取该官方网页；本轮
  改动由 subagent 完成，其自身 agent 沙箱**未抓取**该网页（`web fetch failed`），
  故以上官方内容依据主 Agent 的读取记录，subagent 未据此编造字段。
- 旧样本记录了默认执行模式下文件写入和 `run_command` 被拒、文件未创建；该样本
  没有比较 `--mode`，因此不能单独证明文件拒绝来自用户 permission 设置。1.2.13
  对照复测确认 `accept-edits` 修复文件编辑；`pwd` 实测仍因原生 command policy
  被拒。
- 拒绝不触发自动重登录或自动绕过：CLI 后续仍返回 `result SUCCESS`、空
  `response`、exit 0。Relay 据此给出的 `WorkerFailed` 明确区分“原生 CLI 工具
  权限拒绝”与登录失败，不泄漏凭证/原始私密输出，并保留 `ToolResult` 失败事件。
- 回归样本：`host-write-summary.json` 的 `TargetFile` + 终态 `ERROR` +
  `result SUCCESS` 空回复，以及 `host-coding-tools.ndjson` 的 `CommandLine`
  `run_command` 拒绝，均已固化为离线 fixture；另加“拒绝后重试成功且回复非空仍
  成功”“已完成工具、无错误、空回复不改判”两类反例。

### 代理

- 抽出最小共享 helper `crates/relay-adapters/src/environment.rs`：
  `child_environment` 合并大小写 `NO_PROXY` 并强制排除
  `localhost,127.0.0.1,127.0.0.0/8,::1,[::1],.localhost`；macOS 下无显式代理
  覆盖时读取系统代理。
- Antigravity 子进程与模型发现子进程设置 `AGY_CLI_DISABLE_AUTO_UPDATE=1`，
  只把代理传给这些子进程；父进程环境不变，Relay 本地 HTTP 仍 `.no_proxy`，
  更新客户端继续使用系统/环境代理。
- Grok 的 `launch_environment` 改为调用同一 helper，行为不变（Grok 既有回归
  测试全部通过），并保留 `GROK_DISABLE_AUTOUPDATER=1`。

## 三、使用的证据

本机真实样本（`target/antigravity-host-review/`，只读）：

- `host-echo.ndjson`：真实的 `init` / `user_input` / `agent_response` /
  `result` 事件序列。
- `host-help.txt`：完整的 `agy --help`（输出在 stderr、exit 0），实测
  `--effort` 枚举 `low|medium|high|max`。
- `host-models.txt`：`agy models` 的 14 行 `<id>\t<label>` 两列表格
  （643 字节，exit 0）。
- `host-tools.ndjson`：真实 `view_file` 工具步骤，路径字段为
  `tool_info.parameters.AbsolutePath`，终态 SUCCESS。
- `host-write-summary.json`：真实 `write_to_file` 步骤，路径字段为
  `TargetFile`，`skipPermissions=false`；默认执行模式下记录到 `TOOL_ERROR`
  `permission check failed` / `user denied permission`，文件未创建，CLI 仍
  `result SUCCESS` + 空 `response` + exit0（`stderrErrorKinds=["denied"]`）。
- `host-workspace-write-summary.json`：同一拒绝发生在仓库 `target/` 内
  （`workspaceWithinRepositoryTarget=true`、`model=gemini-3.8-flash-high`），
  记录了当时工作区内文件也未创建；那次运行没有对照 `--mode`，具体文件编辑
  根因见顶部补充。它不代表 `accept-edits` 会放开当前 CLI 配置下的工作区外权限。
- `host-coding-tools.ndjson` 与 `host-coding-tools-summary.json`：真实
  `run_command` 步骤只发布 `CommandLine`，同样被权限拒绝，`response` 为空。
- `host-resume.ndjson`：成功续接同一 UUID `11ca403f-…`。
- `host-missing-conversation.ndjson` 与
  `host-missing-conversation-summary.json`：请求一个不存在的 UUID 时，CLI
  静默新建另一个会话并返回 SUCCESS。
- `host-stdin-handshake.json`：未发送任何 user 输入即观测到 init，
  `anyUserInputSent=false`；SIGTERM 后约 1.273s 回收，无需 SIGKILL。
- `host-cancel-summary.json`：SIGTERM 后约 1.056s 自行收尾，输出
  `result ERROR` + exit1，并非信号终止。
- `host-child-environment.json`：Antigravity 子进程环境含
  `AGY_CLI_DISABLE_AUTO_UPDATE=1` 与大小写一致的 `NO_PROXY`/`no_proxy`，
  回环排除列表完整。
- `host-conversation-comparison.json`：请求不存在的 UUID 时 CLI 实际新建了
  另一个会话（`silentlyCreatedDifferentSession=true`），证明必须在 init 后
  校验会话 ID 才能发送任务。
- `probe-summary.json`：`nativeHostEchoExit=0`、`stderrAuthFailure=false`、
  `agentSandbox=false`。

经 Relay 的当前会话端到端证据（`target/antigravity-e2e-review/`，只读）：

- `e2e-evidence-public.json`：`nativeHost=true`、`agentSandbox=false`，
  本机 agy 1.2.12 已有登录；`models=14`、`--effort` 枚举
  `low/medium/high/max`；`seq4` 的 `nativeEvent.init.model` 为
  `gemini-3.8-flash-low`（该 `nativeEvent` 顶层仅
  `conversation_id`/`event`/`init`）；effort 请求为 `low`（临时 profile 的
  `reasoning`），事件流不含 effort 字段；宿主会话
  `codex:01a0e757-6512-71a3-a17a-8a6c954b28fd`（真实显示名“接入 Zcode CLI
  扫描”）；run `825d2d86-d516-49d2-8e3b-d7ed3e5af1c9` 两轮标记、同一
  conversation `24c9dccc-00c8-4e0a-87c6-21c2ce28c8f1`、`SUCCESS`/exit0；
  cancel run `09d4a936-bb02-4785-b5ff-bbae75c7e121` 为 `cancelled` 且原生
  进程已回收；read_only 在原生 spawn 前失败；无效 model 为 `failed`；
  `activeWorkers=0`、`awaitingHost=0`。
- `runtime-options-public.json`：`--model` 目录 14 项、`--effort` 四档
  `low|medium|high|max`、`source=cli`。
- `e2e-events-public.json`：`run/created` → `worker/started` → `init` →
  `user_input` → `worker/message(final)` → `worker/completed` →
  `run/awaiting_host`，随后第二轮续接 → `run/accepted`；父进程 PID 44870。
- `lease-state.json`：`testPid=44870`、`testPort=7432`、
  `originalPid=53511`、`restored=true`、`routeChangedExternally=false`。
- `config.toml`：临时 write profile（`gemini-3.8-flash-low`、`low`、
  `manual:antigravity-host-review`），正式配置未改动。
- 起源会话 transcript（路径见 `originTranscript`）：
  `~/.codex/sessions/2026/09/28/rollout-2026-09-28T17-27-39-01a0e757-6512-71a3-a17a-8a6c954b28fd.jsonl`，
  工具响应实际出现在第 3428、3440/3446 行。

CLI 随附官方文本指南（本机安装目录内，只读）：

- `~/.gemini/antigravity-cli/builtin/skills/antigravity_guide/`（sitemap 与
  离线子文档）

官方文档地址：

- <https://antigravity.google/docs/cli/headless/>
- <https://antigravity.google/docs/cli/reference>
- <https://antigravity.google/docs/permissions>
- <https://antigravity.google/docs/sandbox>

本轮 review 修复涉及 headless 文档的 *Tool calls in the stream*（真实
`run_command` 样本 `{"CommandLine":"echo hello_headless_demo"}`）与
*Permissions in headless mode*（权限默认继承 settings，headless 无法获取确认时
soft-denied 可仍 exit0）。主 Agent 在本机正常环境已成功读取官方网页；本报告的
代码与离线测试由 subagent 完成，其自身 agent 沙箱**未抓取**该网页
（`web fetch failed`），因此官方内容依据主 Agent 的读取记录，subagent 未据此
编造字段，也不存在“整个本轮无法读取官方网页”的情况。

## 四、自动验证（离线）

- `cargo test -p relay-adapters --lib`：115 通过，1 个既有 DSH 在线测试忽略
  （含 Grok、`cli.rs` shared-cli 与 Antigravity 回归）。
- 本轮新增/加强的离线回归：
  - 输入门控：fake CLI 先打印 init 再读 stdin；正常新启动/续接成功；无效
    UUID、缺失 init、超时、取消均不把任务写入 stdin、不触发任务文件写入，
    并且进程被回收（`cli.rs` 与 `antigravity.rs` 双层）。
  - 真实 `host-help.txt` fixture：报告四档 effort 并验证 `--effort` 实际参数
    应用；真实 `host-models.txt` 严格解析 14 行 id/tab/label。
  - 真实 `host-tools.ndjson` fixture：`AbsolutePath` 归一化为 `path`，原
    `parameters` 保留。
  - 真实 `host-write-summary.json` fixture：`TargetFile` 归一化为 `path`，终态
    工具 `ERROR` 被记录为 permission denial，`result SUCCESS` + 空回复判
    `WorkerFailed` 且原因是权限拒绝、非登录失败；失败 `ToolResult` 保留。
  - 真实 `host-coding-tools.ndjson` fixture：`CommandLine` 归一化为 `command`，
    权限拒绝状态被记录。
  - `cli.rs` shared-cli 回归：最后一个终态工具状态决定合并结果，干净重试清除
    更早的失败。
  - 端到端 fixture：拒绝后空 SUCCESS 判失败；拒绝后干净重试 + 非空回复仍成功。
  - 反例：已完成工具、无错误、空回复保持完成；工具错误但有实际回复仍成功。
  - 真实 `host-missing-conversation.ndjson`：无效 UUID 被拒绝且后续帧不能
    恢复成合法会话；中途切换 `conversation_id` 失败。
  - 终态成功必须 `spawn_error.is_none()`；普通 `ERROR`+exit1 判失败而非取消；
    Relay 握手失败回收进程不读成取消。
  - 模型目录拒绝错误/任意文本，不再从模型 ID 后缀推断 effort。
- UI 控制台去重回归保持通过（`apps/relay-desktop/ui` 下 `cargo test` 7 通过，
  本轮未改 UI）。
- `cargo fmt --all --check`、`git diff --check` 通过；`cargo clippy -p
  relay-adapters --all-targets` 无告警。
- 主 Agent 独立复测（首轮）：`relay-adapters` 109 通过 / 1 ignored、UI 7 通过；
  `relayd` native debug build 与 Trunk UI release build 通过。
- 首轮 `cargo test --workspace` 240 通过 / 2 ignored；本轮按改动范围只跑
  定向测试，不重复全量打包。

## 五、当前会话端到端验证（2026-09-29）

由主 Agent 在本机正常环境（非 agent sandbox）用真实 Codex 当前会话经 Relay
驱动，公开脱敏证据见 `target/antigravity-e2e-review/`。

- 环境：本机 `agy` 1.2.12，已有登录，正常 HOME 与原生进程；未在 agent
  sandbox 内登录或鉴权。宿主会话
  `codex:01a0e757-6512-71a3-a17a-8a6c954b28fd`（真实显示名“接入 Zcode CLI
  扫描”）。
- 目录：`agy models` 14 个模型；`--effort` 官方枚举 `low|medium|high|max`。
  应用模型由 `e2e-events-public.json` 的 `seq4` 中 `nativeEvent.init.model`
  （`gemini-3.8-flash-low`）证实；effort 请求为 `low`（临时 profile 的
  `reasoning`），事件流不含 effort 字段，不据此声称 CLI 端已应用。
- 原生 MCP 全流程：`list_agents`、`run_agent`、`wait_agent`、`resume_agent`、
  `accept_agent`、`cancel_agent` 均返回成功。
- 运行：run `825d2d86-d516-49d2-8e3b-d7ed3e5af1c9` 两轮分别得到
  `RELAY_AGY_CODEX_E2E_OK` 与 `RELAY_AGY_CODEX_RESUME_OK`，同一原生
  conversation `24c9dccc-00c8-4e0a-87c6-21c2ce28c8f1`，`SUCCESS` + exit0，
  每条 final 消息仅一条且与完成 summary 一致，run 最终 accepted/completed。
- 取消：run `09d4a936-bb02-4785-b5ff-bbae75c7e121` 状态为 `cancelled`，
  原生 PID 48045 已实际回收。
- 错误路径：read_only run `bb1465c8-88ec-4410-9992-80e917d4d39a` 在启动真实
  worker 前即被拒绝；无效 model run `ef4fd414-1bea-4427-ba8e-d04e11570aaa`
  状态为 `failed`，没有被误判为成功。
- 验证范围：echo 与 read-file 任务、以及当前会话的 `run`/`resume`/`cancel`/
  `accept` 已端到端通过；2026-09-29 补充的 1.2.13 E2E 已通过 Relay 创建文件并
  返回最终答复，另以 `pwd` 验证 shell 命令仍受用户 permission 策略约束。
- 回到起源会话：当前 Codex transcript 的工具响应实际落在第 3428、3440/3446
  行（路径见证据 `originTranscript`）。
- 隔离与恢复：测试 daemon 为当前分支 0.1.2 开发构建，PID 44870、端口 7432，
  并非已发布 0.1.2 DMG 中的新增功能。临时原生 MCP 路由已自动恢复，原安装
  daemon 0.1.1（PID 53511）身份 nonce 复核通过；正式配置未改，未提取或重登
  任何 CLI 凭证。

## 六、保留的限制与未验证项

- headless Antigravity 没有可强制的只读模式：ReadOnly/Propose 会被拒绝，
  `enforcement` 三项仍为 `false`，`send` 为 `false`。
- 已实现 child 事件映射，但真实 child-agent 生命周期尚未在线验证。
- 无效会话的 stdin 门控与“任务不送达”由真实 init 先于输入证据及离线 fault
  regression 证明；该故障注入只验证拒绝路径，不会向无效会话发送任务。
- 文件编辑通过 `accept-edits` 作为 Relay Write 模式执行，并已在线验证。未 allow
  的 shell 命令仍会被拒绝；拒绝不会触发自动重登录或全局权限绕过，Relay 如实
  判失败。
- 空 SUCCESS 修复只依据公开的终态工具 `error` 对象与空回复判定；它在真实拒绝
  样本上复现、在离线回归中覆盖，但不替代对某次具体工具执行结果的在线核验。
- 错误 probe 在 init 前即被 CLI 拒绝，Relay 目前给出通用
  `worker exited before it reported a session`（`cli.rs`）诊断，后续可细化。
- 控制台重复报告修复在本分支的新 UI 生效；该分支现随 Relay `v0.1.3` 发布，
  安装用户可从 GitHub Release 下载新包。
