# 更新机制

Relay 有**两个互不相干的生命周期**，这是设计上的硬边界：

| 生命周期 | 谁负责 | 怎么更新 |
|---|---|---|
| **Relay App**（菜单栏 + relayd + MCP + Web/面板） | electron-builder + electron-updater | 下载签名后的 zip，退出时替换 `.app`，重启 |
| **Codex 集成**（MCP 入口 / plugin / skill / hooks） | Relay 自己 | 面板里的 安装 / 修复 / 更新 / 从 Codex 移除 |

**升级 App 不等于升级集成。** 新版本装好后，集成检查会独立给出结论：入口路径变了就是 `stale`，skill/hooks 内容变了就是 `outdated`。App 更新路径里没有任何代码会去改插件树。

## 1. 一次构建里的四个产物

```text
pnpm build
├── out/relayd/serve.js      ← daemon（含 web + panel 静态资源）
├── out/relayd/web|panel     ← Web Inspector 与配置面板
├── out/mcp/stdio.js         ← Codex 使用的 MCP server
└── out/menu-bar/main.cjs    ← 菜单栏（Electron 主进程）
```

版本号只有一个来源：仓库根 `package.json`。`tools/define-version.mjs` 把它注入三个 bundle（`__RELAY_VERSION__`），web/panel 由 Vite define 注入，Electron 的版本号由 electron-builder 从同一个字段写入 Info.plist。因此 `/api/health` 报的版本、面板显示的版本和 App 的版本永远一致 —— 不一致就说明有东西没重启（见 §4）。

## 2. 打包（electron-builder）

配置在 `apps/menu-bar/package.json` 的 `build` 段：

| 项 | 值 | 原因 |
|---|---|---|
| `mac.target` | `zip` + `dmg` (arm64) | electron-updater 在 macOS 上只能从 **zip** 更新；dmg 给人装 |
| `extraResources` | `relayd` / `mcp` / `codex` / `appicon` | 装进 `Contents/Resources`，托盘通过 `RELAY_RESOURCES_DIR` 告诉 daemon |
| `extendInfo.LSUIElement` | `true` | 菜单栏应用不进 Dock、不进程序切换器 |
| `hardenedRuntime` + entitlements | `build-resources/entitlements.mac.plist` | Electron 需要 JIT / 未签名可执行内存；daemon 与 MCP 是子进程，需要 `disable-library-validation` |
| `files` | `out/**/*`，排除 `*.map` | asar 里只放运行需要的东西 |
| `publish` | generic，URL 来自 `RELAY_UPDATE_URL` | 单一 feed，可换成 S3/GitHub |

```bash
pnpm pack:dir     # 快速本地构建，dist/mac-arm64/Relay.app
pnpm pack:mac     # zip + dmg，不发布
RELAY_UPDATE_URL=https://…/mac pnpm release:mac   # 构建并发布到 feed
```

签名与公证：本地构建会用机器上的签名身份（`CSC_IDENTITY_AUTO_DISCOVERY=false` 可关闭）。正式发布需要 Developer ID + 公证凭据：

```bash
export CSC_LINK=… CSC_KEY_PASSWORD=…
export APPLE_ID=… APPLE_APP_SPECIFIC_PASSWORD=… APPLE_TEAM_ID=…
# 然后把 apps/menu-bar/package.json 的 build.mac.notarize 改成 true（现在是 false）
```

electron-updater 只接受**已签名**的包，未公证的构建会被 Gatekeeper 拦下，所以这两项不是可选项。

## 3. 更新（electron-updater）

`apps/menu-bar/src/updater.ts` 只做接线，不自己实现下载/校验/替换/重启：

- `autoDownload = false`（先问用户），`autoInstallOnAppQuit = true`（下载完退出时安装）。
- 启动时检查一次，菜单里也可以手动「检查更新…」。
- 状态直接进菜单：检查中 / 发现新版本 x.y.z（可“下载更新”）/ 下载进度 / 已下载（可“重启并安装”）/ 已是最新 / 出错。
- 开发态与未打包构建报「需要安装版才能检查更新」，不会假装检查过；`RELAY_UPDATE_FEED` 可以指向本地 feed 用于联调。

## 4. App 与 daemon 的版本一致性

daemon 是独立进程（它是日志与控制面的后端，不随窗口开关），升级 `.app` 不会自动换掉正在运行的 daemon。因此：

- 托盘启动 **打包版 App 时**用 `ELECTRON_RUN_AS_NODE=1` 拉起 `Contents/Resources/relayd/serve.js`；只用 Electron 自带的 Node，不需要系统装 node/pnpm/corepack/tsx。
- 托盘**不信任 PID**：`server.json` 里的 pid + nonce 与 `/api/health` 回显一致才算在运行（docs/menu-bar.md 5）。
- 托盘把 `health.version` 与 `app.getVersion()` 比对，不一致就在菜单顶部提示「日志服务版本 X（App Y）— 建议重启」，避免新版托盘驱动旧版 daemon。
- 只有**托盘自己拉起的** daemon 会在退出/安装更新时被停止；用户自己启动的 daemon 不受影响。

## 5. 本地验证更新链路

```bash
# 1) 起一个支持 PUT 的本地 feed（electron-builder 发布会 PUT 上来）
python3 /tmp/relay-feed-server.py         # http://127.0.0.1:8099
# 2) 构建并发布
RELAY_UPDATE_URL=http://127.0.0.1:8099/ pnpm release:mac
# 3) feed 上会有 latest-mac.yml + zip；App 内会有 app-update.yml
# 4) 把 latest-mac.yml 的 version 改大（并指向同一个 zip）→ 应用菜单显示“发现新版本”
```
