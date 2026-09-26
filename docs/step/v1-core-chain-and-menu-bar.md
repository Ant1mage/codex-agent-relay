# v1 · 核心链路验证 + macOS 菜单栏落地

## 目标
确认 Codex → relay-mcp → SQLite → relayd → Web Inspector 链路是通的，然后做一个 Clash 那样的 macOS 菜单栏。

## 遇到的问题
1. **链路各段都在，但没人端到端验证过。** MCP server、RunController、event store、daemon、Web 各自有单测，没有一次"真实 Codex 会话真的委派出去又回来"的记录。
2. **Electron 被当成 Node 启动。** DSH 的 shell 里带 ELECTRON_RUN_AS_NODE=1，直接跑 electron . 会以 Node 身份启动，报下面这条，必须 env -u 清掉（exec 是 shell 内建，不能写在 env -u 后面）：
```
SyntaxError: does not provide an export named 'BrowserWindow'
```
3. **better-sqlite3 的 ABI 与运行时 Node 不匹配**（DSH 自带 node 是 ABI 148，nvm node 24.14 是 ABI 137）：
```
Error: The module was compiled against a different Node.js version
NODE_MODULE_VERSION 137 vs 148
```
4. **菜单栏没有可复用图标路径。** Tray 要 16pt + @2x 模板图（只取 alpha 通道），仓库里只有应用图标。
5. **菜单逻辑与 Electron 耦死**，无法无 GUI 测试。

## 怎么解决
- 菜单栏没有窗口、没有 renderer、没有 HTML：只有 Tray + NSMenu，通过 relayd 的 HTTP API 读投影，点会话用 Edge/Chrome 打开 Inspector URL。
- 图标复用 assets/appicon/png/light/relay-icon-16.png 并 setTemplateImage(true)，@2x 用同目录 32px 叠加，不新增 tray 专用资源。
- 菜单模型抽成纯函数 menu-model.ts（MenuBarView → MenuBarItem[]），不 import Electron，因此可无 GUI 单测。
- 测试统一用 nvm node 24.14：PATH 前置 $HOME/.nvm/versions/node/v24.14.0/bin 再跑 pnpm test。

## 验证
- 链路：真实 MCP stdio 客户端（initialize → tools/list → sync_session → list_agents → run_agent → wait_agent）跑通一次真实委派。
- 菜单栏：真实 NSMenu 截图（RELAY_MENU_BAR_PREVIEW=1 让托盘在开发态自动弹菜单）。

## 遗留
- Worker 只到"能跑"，失败重试与取消后清理未覆盖。
- 菜单栏只有状态与跳转、没有任何配置能力 —— v4 记成 Critical。
- Web Inspector 当时承担"安装 Codex 集成"的动作，边界不对，v5 才纠正。

## 本轮记住的环境事实
| 现象 | 处理 |
|---|---|
| ELECTRON_RUN_AS_NODE=1 已在环境里 | 跑 Electron 一律 env -u ELECTRON_RUN_AS_NODE |
| better-sqlite3 ABI 不匹配 | 用 nvm node 24.14（v7 改用 node:sqlite 根治） |
| Codex exec 连不上网 | 代理指向 127.0.0.1:7890，NO_PROXY 放行本机 |
| Codex 用量受限 | 改走 MCP stdio 边界驱动真实链路 |
