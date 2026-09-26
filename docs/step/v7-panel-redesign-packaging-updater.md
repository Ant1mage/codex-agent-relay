# v7 · 面板重做 + 打包 + electron-updater

## 目标
1. 重做配置面板 UI（Codex 集成 tab 超宽）；2. 智能体支持添加；3. 运行时同理；4. Codex 集成要有卸载；5. 添加/删除跳转到已有面板而不是开新面板；6. 接入 electron-updater；7. 修掉 v6 发现的问题。

## 遇到的问题
1. **Codex 集成 tab 超宽。** 无头浏览器实测：内部 div scrollWidth 816px > clientWidth 418px。根因是检查项的 detail（很长的路径）没有 min-w-0，把 ScrollArea 撑开。
2. **"智能体不能添加"。** 按钮其实在，但编辑器渲染在列表下方，在 640px 面板里落到折叠线以下，点了像没反应。
3. **运行时只能扫描不能增删**；Codex 集成的"从 Codex 移除"同样在折叠线以下。
4. **菜单每次都 loadURL 重载面板**，没有"就地跳到某个表单"的机制。
5. **面板用 file:// 加载失败**：模块与样式被 CORS 拦，Origin 也过不了 daemon 的同源校验。改成由 daemon 托管 /panel/。
6. **托盘主进程打成 ESM 后启动即崩**（electron-updater 的依赖用动态 require，ESM bundle 提供不了）：
```
Error: Dynamic require of "fs" is not supported
    at …/app.asar/out/main.js
```
7. **electron-updater 没有默认导出**，import default 拿到 undefined：
```
TypeError: Cannot destructure property 'autoUpdater' of 'import_electron_updater.default'
```
8. **打包 App 启动即退出。** 两个原因叠加：托盘图标路径在包内不存在（icon.isEmpty() 触发 app.quit()），以及上面的 ESM 崩溃。另外测试环境带 ELECTRON_RUN_AS_NODE=1，直接运行二进制其实是以 Node 跑，看起来像秒退。
9. **better-sqlite3 只有符号链接**，装机后 MCP 必然起不来；Node 与 Electron 的 ABI 还不一样。
10. **目录模式打包不生成 app-update.yml**（没有 publish 目标），更新器会报错而不是说"需要安装版"。
11. **两个 electron-builder 并行会互相卡死**（都写 dist/），实测双双挂 20 分钟。

## 怎么解决
- 面板统一"单列 + min-w-0"：四个 tab 溢出全部为 none；Agents/Runtimes 改成列表 ⇄ 编辑器两个视图；Codex tab 把检查与四个动作分区，卸载独立成红色整行。
- 运行时支持手动添加：adapter + 可执行文件路径，先用 --version 探测再保存（POST /api/runtimes/probe、PUT/DELETE /api/config/runtimes/:id）；daemon 与 relay-mcp 都把手动项叠加在自动扫描之上。
- 菜单项带意图（new-agent / edit-agent:id / add-runtime / codex-actions）；面板已打开时通过 preload IPC 就地导航，永不重载、永不开第二个窗口。
- 配置损坏不再静默：profiles/settings/runtimes.json 解析失败会进 /api/config 的 warnings，并在面板顶部显示红色横幅。
- 打包：electron-builder 出 zip+dmg(arm64)、hardened runtime entitlements、LSUIElement、extraResources 带 relayd/mcp/codex 源/图标；脚本 pack:dir / pack:mac / release:mac。
- 托盘主进程改打 CJS（.cjs 后缀，ESM 源码 + define import.meta.dirname=__dirname），updater 用命名空间 import 取 autoUpdater。
- 托盘图标从 Resources/appicon 解析；打包态用 ELECTRON_RUN_AS_NODE=1 启动 Resources/relayd/serve.js 并传 RELAY_RESOURCES_DIR；写进 Codex 的 MCP 入口也用同一 flag（codex mcp add --env ELECTRON_RUN_AS_NODE=1）。
- 版本单一来源：根 package.json 由 tools/define-version.mjs 注入三个 bundle，Electron 版本由 electron-builder 写 Info.plist；托盘比对 health.version 与 app.getVersion()，不一致就提示"日志服务版本 X（App Y）—— 建议重启"；退出/安装更新时只停自己拉起的 daemon。
- 原生模块根除：三个 SQLite store 改用 node:sqlite（Electron 44 自带 Node 24.21，实测可用），MCP bundle 自包含，无 ABI 问题，删除 link-native-deps。
- 更新区块与 daemon 状态无关（daemon 挂了也能看到"检查更新…"）；没有 feed 的构建报"需要安装版"，不报错。

## 验证
- 面板：四个 tab overflow none；新建智能体 → 编辑器出现；添加运行时按钮存在；卸载按钮可见。
- 打包：dist/mac-arm64/Relay.app 含 Resources/{relayd,web,panel,mcp,codex,appicon}，LSUIElement=true，版本 0.1.0。
- 打包 App 真机：启动后自动从 Resources 拉起 daemon（ps 显示 Relay 二进制 + Resources/relayd/serve.js），server.json 与 health 版本一致，/panel/ 与 / 都返回 200；退出 App 后 daemon 被停掉、端口释放。
- 打包态检查正确报 relay-mcp stale（旧入口指向开发态 node + out/mcp/stdio.js）—— 证明"升级 App 不等于升级集成"成立。
- app-update.yml 在 publish 构建里正确生成（provider generic + feed URL）。
- 128 测试通过，typecheck 干净。

## 遗留
1. 签名与公证：现在 notarize=false，用机器上的 Apple Development 身份；正式发布需要 Developer ID + APPLE_ID/APPLE_APP_SPECIFIC_PASSWORD/APPLE_TEAM_ID。
2. 真实 feed URL（当前用本地 127.0.0.1:8099 联调，docs/updates.md 有复现步骤）。
3. 未完成的验证：打包态"发现新版本"菜单文案只验证到"检查更新…"入口；全量重建后打包态的 Codex 修复没再跑一次（源文件与 RELAY_RESOURCES_DIR 都已确认在包内）。
