# macOS 菜单栏 (Menu Bar)

Relay 的桌面端是可选控制台，大部分时间驻留在菜单栏（docs/README.md）。本文定义菜单栏是什么、和 Clash 的对应关系、以及它明确不做什么。实现落在 `apps/desktop/src/main/menu-bar*.ts`。

![Relay 菜单栏](images/menu-bar.png)

## 1. 定位

菜单栏是**同一个 projection 的第二种呈现**，不是第二个工作台：

```text
SQLite event log → projectRun → DesktopSnapshot ─┬→ 窗口 (React)
                                                 └→ 菜单栏 (NSMenu)
```

因此菜单里出现的每一条信息，窗口里都能看到；菜单里能做的每个动作，窗口里也都有对应入口。菜单栏只补充窗口给不了的东西：**不打开窗口就能看到有没有 worker 在跑，并能立刻停掉它。**

与窗口一致的三条硬性约束（docs/architecture.md 1.1）：

- 不解析原厂 stdout，不推导状态，只读 projection。
- 不选择 Profile、不拆任务、不重试——那是 Codex 的职责。
- 不展示隐藏推理和 Runtime 内部 child agent。

## 2. 与 Clash 的映射

Clash 的菜单栏是"本机网络状态 + 少量开关 + 二级节点选择"。Relay 借用同样的形状，但把每一项换成真实存在的概念：

| Clash 菜单栏 | Relay 菜单栏 | 说明 |
|---|---|---|
| 图标 + 当前状态 | 图标 + `Relay · N 个运行中` | 状态行是菜单第一行，只读 |
| 打开面板 | 打开 Relay（⌘O） | 双击图标同样回到窗口 |
| 系统代理（开关） | 隐藏 Dock 图标 / 登录时启动 | 都是"本机如何呈现自己"的开关；Relay 没有代理概念 |
| 模式：规则 / 全局 / 直连 | **无对应项** | 委派给谁、允不允许写，属于 Codex 与 policy，菜单栏不提供"模式"开关 |
| 策略组 → 节点 | 进行中的委派 → 单个 worker | 二级子菜单：在 Relay 中查看 / 取消 |
| 配置 / 订阅列表 | 会话 → Codex HostSession | 会话就是 Relay 的"节点来源" |
| 复制终端命令 | 复制会话 ID / 复制诊断信息 | |
| 退出 | 退出 Relay（⌘Q） | |

结论：形状像 Clash，语义完全属于 Relay。**没有为了像 Clash 而发明的开关。**

## 3. 菜单结构

```text
Relay · 2 个运行中                     ← 状态行 (disabled)
打开 Relay                        ⌘O   ← 恢复 / 聚焦窗口
──────────
进行中的委派 (2)                  ▸    ← 仅当有 worker 在跑
    会话名 — DeepSeek Code · 任务摘要 ▸
        在 Relay 中查看
        取消 worker
    会话名 — …(同一会话 ≥2 个 worker 时才有) 停止所有 worker · 会话名
──────────
会话                             ▸     ← 最近 8 条 Codex 会话
    会话名                      ▸
        在 Relay 中查看
        停止所有 worker          ← 该会话无活跃 worker 时置灰
        打开工作区
        复制会话 ID
智能体                            ▸     ← Profile 列表即状态, 不是开关
    DeepSeek Code                     ← 可用: 点击进入 Settings › Agents
    Kimi Code · 需要认证               ← 置灰并给出原因
Codex 集成                        ▸
    已连接 / 未配置                    ← disabled
    ✓ 已检测到 Codex                   ← 三项真实检查
    ✗ 已配置 Relay MCP — <路径>         ← 失败项带路径, 便于手工配置
    安装到 Codex…
──────────
隐藏 Dock 图标                    ✓     ← 仅 macOS
登录时启动                        ✓     ← macOS / Windows
──────────
设置…                            ⌘,
复制诊断信息
──────────
退出 Relay                       ⌘Q
```

折叠规则（避免机器越忙菜单越长）：

| 项 | 上限 | 超出时 |
|---|---|---|
| 活跃 worker | 12 | 追加 disabled 行 `+N` |
| 会话 | 8 | 直接截断（窗口里还有完整列表） |
| worker 标签里的任务 | 48 字符 | 单行化 + `…` |

## 4. 图标与状态

**模板图标（template image）**：macOS 只用模板图的 alpha 通道，因此一个资源同时适配浅色与深色菜单栏。Relay 的浅色导出本来就是"透明底 + 深色图形"，正好就是模板需要的遮罩，于是：

- 直接复用 `assets/appicon/png/light/relay-icon-16.png`，并叠加 32px 作为 @2x 表示；
- 不新增 tray 专用资源，也不引入图标构建脚本（assets/appicon/README.md 的约定保持不变）。

**状态行优先级**（图标本身不承载状态，避免彩色/动画图标）：

1. 有 worker 在跑 → `Relay · N 个运行中`
2. 否则有 Run 在 `awaiting_host` → `Relay · N 个等待 Codex`
3. 否则 `就绪` / `需要配置` / `未检测到 Runtime`

"就绪"的判定与窗口右上角的环境指示一致：至少一个 Runtime 可用 + 至少一个启用且可用的 Profile + Codex 集成配置完成。

## 5. 行为规则

| 规则 | 行为 | 原因 |
|---|---|---|
| 单击图标 | 打开原生菜单 | 和所有 macOS 菜单栏工具一致 |
| 双击图标 | 打开 / 聚焦窗口 | Clash 式快捷回到面板 |
| 关闭窗口 | 隐藏窗口，进程与菜单栏继续 | 菜单栏应用里 "关闭窗口 ≠ 退出"；退出只走菜单或 ⌘Q |
| 隐藏 Dock 图标 | `setActivationPolicy('accessory')` | 无 Dock 图标、无应用菜单，只剩菜单栏；再次关闭即恢复 `regular` |
| 登录时启动 | 读/写系统 login items | 状态以操作系统为准，Relay 不存副本 |
| 语言 | 跟随窗口语言，初始取系统语言 | 窗口切换语言后通过 `relay:locale:set` 通知菜单重建 |
| 刷新 | 2s 轮询 projection；Codex 探针 60s 缓存 | 与窗口共享同一份 snapshot 缓存（TTL 500ms）；序列化后的模型不变则不重建 NSMenu |
| 取消 | 沿用既有 control queue | 菜单只写 `relay_control_commands`，与实际 kill 的解耦方式与窗口一致 |

## 6. 明确不做

- **不做"暂停委派"总开关。** Relay 不是执行侧策略的最终裁决者：要停就 cancel（已有、可审计），要改规则去 Settings/policy。加一个菜单栏开关会绕开 policy 的 scope 解析。
- **不做 Agents 启停开关。** Profile 的启用状态影响的是 Codex 能发现什么，属于设置面；菜单里只显示状态并作为入口。
- **不显示 child agent / 隐藏推理。** 与 Console 相同（docs/ui.md 12.4 的约束依然成立）。
- **不做通知与 badge。** 见第 9 节。

## 7. 实现结构

| 文件 | 职责 |
|---|---|
| `src/main/menu-bar-model.ts` | 纯函数：snapshot → `MenuBarView` → `MenuBarItem[]`。不 import Electron，因此可单测 |
| `src/main/menu-bar.ts` | Electron 侧：Tray 图标、model → `MenuItemConstructorOptions`、轮询、action 派发 |
| `src/main/diagnostics.ts` | 纯函数：版本/路径/Runtime/Codex 检查 → 可粘贴的诊断文本 |
| `src/main/index.ts` | 唯一的接线点：`MenuBarHost` 实现、窗口显示、Dock 策略、IPC |
| `src/shared/api.ts` | `DesktopMenuBarPrefs`、`DesktopNavigateRequest`（含 `notice`） |
| `src/preload/index.ts` | `onNavigate` / `setLocale` 两个新桥接 |

菜单 → 窗口方向只有一条通道：主进程把 `DesktopNavigateRequest` 推给 renderer，renderer 只做"选中某个会话/Run"或"打开 Settings 的某一页"。菜单拿不到也不需要 renderer 的状态。

设置持久化：`~/.relay/settings.json` 增加 `menuBar.hideDockIcon`。读取时只接受布尔值，其余一律回落为 off，因此手改坏的文件不会让应用进入 UI 表达不出来的状态。

## 8. 验证

- 单测：`apps/desktop/test/menu-bar.test.ts`（20 条）覆盖状态行优先级、任务截断、子菜单结构、上限折叠、置灰原因、macOS-only 开关、中英标签；`apps/desktop/test/diagnostics.test.ts` 覆盖诊断文本。
- 冒烟（真实 NSMenu + 真实数据）：用 `RELAY_DB_PATH` / `RELAY_SETTINGS_PATH` 指向一份带活跃 Run 的库，然后

  ```bash
  env -u ELECTRON_RUN_AS_NODE RELAY_MENU_BAR_PREVIEW=1 \
    RELAY_MENU_BAR_PREVIEW_DELAY_MS=10000 pnpm --filter @relay/desktop dev
  ```

  `RELAY_MENU_BAR_PREVIEW` 只在未打包的构建里生效，用于截图与人工检查（上面的图就是这么来的）。
  注意：在 DSH 这类设置了 `ELECTRON_RUN_AS_NODE=1` 的环境里，必须用 `env -u` 清掉它，否则 Electron 会以纯 Node 方式启动并报 `does not provide an export named 'BrowserWindow'`。

## 9. 后续

1. **`awaiting_host` 视觉提示**：需要一张"标记 + 圆点"的模板图（现有资源只有纯标记），因此本轮只做状态行与计数。
2. **打开菜单时即时刷新**：macOS 在 mouse-down 时就弹出菜单，无法先刷新再显示；当前靠 2s 轮询，必要时可改用 `tray.on('mouse-down')` 触发一次 force refresh。
3. **打包相关的默认值**：`LSUIElement`、开机自启、通知权限都取决于打包配置，目前仓库还没有打包配置。
4. **诊断导出到文件**：现在只写剪贴板，后续可加"导出到桌面"并在 Finder 中显示。
5. **Windows / Linux 托盘**：模型已经按平台裁剪开关，但托盘图标（非模板语义）与关闭窗口的约定需要分别验证。
