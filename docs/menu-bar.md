# macOS 菜单栏与控制面板

Relay 的桌面存在感只有菜单栏：**一个状态菜单 + 一个配置面板**。菜单负责状态与快速动作，面板负责表单——两者都属于同一个菜单栏体系，配置不会被推给网页。

![Relay 菜单栏](images/menu-bar.png)

## 1. 边界

```text
relayd（本地 Control Plane，127.0.0.1:7352）
  ├─ 配置读写（Agent Profile / Policy）
  ├─ Runtime 扫描与 model/reasoning 发现
  ├─ Codex 集成生命周期（检测/安装/更新/修复/卸载）
  ├─ 取消、诊断、投影、SSE
  └─ 托管两个页面：Web Inspector（日志）与 Control Panel（配置）
        ▲                                   ▲
        │ 打开 /s/<session>                  │ 打开 /panel/
   menu-bar（Tray + Panel 窗口）        Edge / Chrome（只读查看器）
```

| 表面 | 负责 | 不负责 |
|---|---|---|
| 菜单栏（Tray） | 状态、快速取消、打开检查器/面板、启动 daemon、诊断、登录项 | 表单编辑 |
| 控制面板（Tray 的 popover 窗口） | Agent Profile、Model/Reasoning、Capabilities、Policy、Workspace 覆盖、Runtime 扫描、Codex 集成安装/修复/更新/移除 | 日志浏览 |
| Web Inspector | Session / Run / Step / Console / Changes / Raw / 详细诊断 | 任何配置写入 |

## 2. 菜单结构

```text
Relay · 2 个运行中                     ← 状态行 (disabled)
打开检查器                        ⌘O
打开配置面板…                     ⌘,    ← 控制面板
──────────
进行中的委派 (2)                  ▸    ← 仅当有 worker 在跑：在检查器中查看 / 取消
会话                             ▸
智能体                            ▸     ← 每个 profile 一行，点击进入编辑器
运行时                            ▸     ← adapterId · health · version + 重新扫描
Codex 集成                        ▸     ← 5 项检查 + 设置… + 安装/修复
──────────
刷新
复制诊断信息
登录时启动                        ✓
──────────
退出 Relay                       ⌘Q
```

daemon 不可达时换成自愈分支（启动 / 重启日志服务），并且只在**校验过身份**时才允许重启（见 §5）。

![daemon 未运行](images/menu-bar-daemon-down.png)

## 3. 控制面板

![控制面板](images/panel.png)

四个页签，全部通过 daemon 的 HTTP API 读写，**不直接碰文件**：

| 页签 | 内容 |
|---|---|
| 智能体 | Profile 列表 + 启用开关；编辑器含 name、description、Runtime、Model、Reasoning、Instructions、四项 Capabilities、启用；新建/删除 |
| 策略 | Global：最大并发 run/writer、worktree 要求、允许写/命令/网络；Workspace：选择工作目录后写入覆盖项或清除 |
| Codex 集成 | 5 项检查（状态 + 原因 + 修复提示）与 安装 / 修复 / 更新 / 从 Codex 移除 |
| 运行时 | 每个 runtime 的 adapter、可执行文件、版本、健康度、能力；重新扫描；打开 Inspector |

- Model / Reasoning 的选项来自 **CLI 自己报告的值**（`GET /api/runtimes/:id/options`），Relay 不发明模型名；CLI 不提供列表时明确写"该 CLI 未公开模型列表"。
- 面板由 daemon 托管（`/panel/`）而不是 `file://`：同源、无 module/CORS 限制，token 走 URL 片段，daemon 的 Origin/Host 校验不需要特殊分支。
- 面板窗口是无边框、无 Dock 条目的 popover：失焦即隐藏，锚在菜单栏图标下方。

## 4. 状态行

1. daemon 未运行 → `Relay · 日志服务未运行`
2. 正在启动 → `Relay · 正在启动日志服务…`（20 秒后仍不健康就回落）
3. 有 worker 在跑 → `Relay · N 个运行中`
4. 否则有 Run 在 `awaiting_host` → `Relay · N 个等待 Codex`
5. 否则 就绪 / 需要配置 / 未检测到 Runtime

最后一行还会显示最近一次失败（spawn 失败、Codex 动作失败），因为托盘是用户唯一剩下的界面。

## 5. daemon 生命周期安全

`~/.relay/server.json` 记录 `pid / port / url / token / nonce`，权限 0600。

- **存活判定 = 身份校验**：只有 `/api/health` 返回的 `pid` 与 `nonce` 都和文件一致，才算"daemon 在运行"。PID 会被复用，单凭 PID 判断既不安全也不准确。
- **不会误杀**：只有校验通过时，"重启日志服务"才会发 SIGTERM；stale 文件 + PID 复用不会波及无关进程。
- **不会被挡住**：stale 文件不会阻止新 daemon 启动（新进程绑定端口后覆盖记录）。
- **并发启动**：`~/.relay/daemon.lock` 独占创建 + 过期回收，避免两个 daemon 同时启动、后者抢占 server.json。
- 启动失败会以错误行的形式回到菜单，而不是抛出未捕获异常把菜单栏带崩。

## 6. 图标

复用 `assets/appicon/png/light/relay-icon-16.png`（叠加 32px 作为 @2x）并设置 `setTemplateImage(true)`：macOS 只用 alpha 通道，一个资源同时适配浅色与深色菜单栏。不新增 tray 专用资源，也没有图标构建脚本。

## 7. 明确不做

- **不做"暂停委派"总开关**：要停就 cancel（可审计），要改规则去 Policy。
- **不在菜单栏编辑器里手写模型名**：只列出 CLI 报告的取值。
- **不显示 child agent / 隐藏推理**：与 Console 相同。
- **不在 Web 里做配置**：Web 只显示 effective state。

## 8. 实现结构

| 文件 | 职责 |
|---|---|
| `apps/menu-bar/src/menu-model.ts` | 纯函数：`MenuBarView` → `MenuBarItem[]`，可单测 |
| `apps/menu-bar/src/main.ts` | Tray、模型转 NSMenu、轮询、动作派发、错误回显 |
| `apps/menu-bar/src/panel.ts` | 无边框 popover 窗口、定位、失焦隐藏 |
| `apps/menu-bar/panel/` | 控制面板（React + shadcn，独立 Vite build，由 daemon 托管） |
| `apps/menu-bar/src/daemon.ts` | server.json + nonce 校验、启动/停止 daemon、浏览器选择 |

## 9. 验证

- 单测：`apps/menu-bar/test/menu-model.test.ts`（菜单结构、状态行、上限折叠、中英标签）。
- 冒烟（真实 NSMenu + 真实数据）：

  ```bash
  pnpm build                                   # out/relayd + out/mcp + out/menu-bar
  node out/relayd/serve.js &
  env -u ELECTRON_RUN_AS_NODE \
    RELAY_MENU_BAR_PREVIEW=1 RELAY_PANEL_PREVIEW=1 \
    pnpm --filter @relay/menu-bar start
  ```

  `RELAY_MENU_BAR_PREVIEW` / `RELAY_PANEL_PREVIEW` 只在未打包构建里生效，用于截图与人工检查。
  在设置了 `ELECTRON_RUN_AS_NODE=1` 的环境（例如 DSH 的 shell）里必须用 `env -u` 清掉，否则 Electron 会以纯 Node 启动。
