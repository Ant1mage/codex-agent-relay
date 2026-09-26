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

运行时版本号以仓库根 `package.json` 为准。`tools/define-version.mjs` 把它注入 relayd、MCP 与菜单栏 bundle（`__RELAY_VERSION__`）；electron-builder 的 App 包版本必须与之同步。CI 的 tag 校验和产物校验会阻止错版发布。因此 `/api/health`、MCP 握手与 App 版本不一致时，说明包版本漂移或旧进程尚未重启（见 §4）。

## 2. 打包（electron-builder）

配置在 `apps/menu-bar/package.json` 的 `build` 段：

| 项 | 值 | 原因 |
|---|---|---|
| `mac.target` | `zip` + `dmg` (arm64) | electron-updater 在 macOS 上只能从 **zip** 更新；dmg 给人装 |
| `extraResources` | `relayd` / `mcp` / `codex` / `appicon` | 装进 `Contents/Resources`，托盘通过 `RELAY_RESOURCES_DIR` 告诉 daemon |
| `extendInfo.LSUIElement` | `true` | 菜单栏应用不进 Dock、不进程序切换器 |
| `hardenedRuntime` + entitlements | `build-resources/entitlements.mac.plist` | Electron 需要 JIT / 未签名可执行内存；daemon 与 MCP 是子进程，需要 `disable-library-validation` |
| `files` | `out/**/*`，排除 `*.map` | asar 里只放运行需要的东西 |
| `publish` | GitHub Releases（`Ant1mage/relay`） | `latest-mac.yml`、zip 与 dmg 来自同一个真实 release |

```bash
pnpm pack:dir     # 快速本地构建，dist/mac-arm64/Relay.app
pnpm pack:mac     # zip + dmg，不发布
pnpm verify:release # 检查 App 内资源、zip、dmg、latest-mac.yml 与 sha512
pnpm release:mac  # 签名、公证并发布到 GitHub Releases（需要 GH_TOKEN 与 Apple 凭据）
```

签名与公证：本地构建会用机器上的签名身份（`CSC_IDENTITY_AUTO_DISCOVERY=false` 可关闭）。正式发布需要 Developer ID + 公证凭据：

```bash
export CSC_LINK=… CSC_KEY_PASSWORD=…
export APPLE_API_KEY=/path/to/AuthKey.p8 APPLE_API_KEY_ID=… APPLE_API_ISSUER=…
# apps/menu-bar/package.json 已启用 notarize；缺少完整凭据的正式发布会被 CI 预检拒绝。
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
- 普通退出只停止**托盘自己拉起的** daemon；明确点击“重启并安装”时会停止任何通过 nonce 验证的 relayd，保证新 App 重启后不会继续连接旧 daemon。
- MCP 配置指向 App bundle 内的固定入口，因此新 Codex 会话会启动更新后的 MCP；更新前已经存活的 stdio MCP 进程不能被 App 安全替换，需要重启对应 Codex 会话。
- Codex plugin/skill/hooks 仍是独立生命周期。App 更新后状态检查会把旧副本标成 `outdated`，用户从面板执行“更新”完成闭环，不在 App updater 中暗改 Codex 配置。

## 5. 本地验证更新链路

```bash
# 未打包开发态仍可用 RELAY_UPDATE_FEED 指向本地只读 feed 联调状态机。
# 正式链路由 tag vX.Y.Z 触发 .github/workflows/release.yml：
# 1) 校验 tag 与根 package.json 版本一致
# 2) test + typecheck
# 3) Developer ID 签名 + Apple notarization
# 4) 发布 zip + dmg + latest-mac.yml 到 GitHub Release
# 5) verify:release 校验 bundle 资源与更新 sha512 元数据
```
