# electron-app（归档分支）

这个分支保存 Relay 的 **Electron 桌面端**：`apps/desktop`（Electron Main + preload + React renderer + onboarding/settings + 内嵌菜单栏）。

`main` 已经改成"菜单栏 + 浏览器 Inspector"的架构，`apps/desktop` 在 main 上被删除，取而代之的是：

| 现在的位置 | 作用 |
|---|---|
| `apps/menu-bar` | Electron **只做 Tray**：无窗口、无 preload、无 renderer |
| `apps/relayd` | 日志 daemon：只读投影 + HTTP API + SSE + 静态托管（127.0.0.1:7352） |
| `apps/web` | Web Inspector（React + Vite + shadcn），由 daemon 托管 |
| `packages/relay-api` | 三者共用的传输契约与客户端 |

本分支的最后提交是 main 上最后一个包含完整 Electron 应用的提交：

```
f707641  merge: add the macOS menu bar
```

用法：

```bash
git checkout electron-app
pnpm install
pnpm desktop:dev      # electron-vite dev
pnpm desktop:build
```

注意：这个分支的 `pnpm desktop:dev` 在设置了 `ELECTRON_RUN_AS_NODE=1` 的环境里需要用 `env -u ELECTRON_RUN_AS_NODE` 清掉该变量，否则 Electron 会以纯 Node 启动。

新架构的说明见 main 上的 `docs/inspector.md` 与 `docs/menu-bar.md`。
