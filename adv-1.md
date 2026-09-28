# Relay 统一桌面应用 UI 重构方案与技术评估 (`adv-1.md`)

> **重大架构决策**：彻底放弃原先“420×640 菜单栏小弹窗 + 全屏 Web 只读检查器”的双表面割裂设计，**合并为一套统一、现代的 macOS 桌面主应用（Single Unified Desktop App）**。

---

## 一、核心战略决策：双界面合二为一

### 1. 为什么必须合二为一？
* **旧架构的拧巴之处**：
  - **菜单栏面板（420×640 弹窗）**：空间极度逼仄，表单超长导致保存/删除按钮直接沉底滚出可视区；420px 宽度硬塞 5 个 Tab 拥挤不堪；窗口失焦自动隐藏（Hide on Blur）极易导致用户编辑中的配置丢失；路径与元数据大面积使用 10px 微型字体，可读性极差。
  - **Web 检查器（全屏）**：空间极其充裕，却只能“只读”看日志；委派任务被切分为 Run 卡片、Step 芯片、Console/Changes/Raw 子标签页，碎片化严重；无法直接调整 Agent 配置或策略，缺少操作闭环。
* **新形态（单窗口桌面原生工作台）**：
  - **菜单栏托盘（Tray）纯净化**：退居为最纯粹的系统常驻入口。左键点击一键平滑呼出/隐藏主桌面窗口；右键弹出 macOS 原生快捷菜单（展示当前活跃状态、一键复制诊断、退出应用）。彻底废弃复杂的悬浮窗坐标计算逻辑。
  - **统一主桌面窗口**：标准 macOS 窗口（默认尺寸 `1050 × 720`，支持自由拖拽缩放、全屏与位置记忆），容纳任务工作流与系统配置管理。
* **恢复 Dock 栏常驻（解除 `LSUIElement` 封印）**：
  - **解开系统附属限制**：清理 `Info.plist` 中的 `<key>LSUIElement</key><true/>`，并在 `shell.rs` 中将策略调整为 `ActivationPolicy::Regular`（标准桌面程序）；
  - **启用高精 Dock 图标**：直接激活项目中已预置的 `16~512px` 及原生 `.icns` 资源；
  - **标准桌面生命周期**：支持 `Cmd + Tab` 快捷切换与 Dock 图标唤起；点击窗口关闭按钮（红叉）仅隐藏窗口，后台 `relayd` 与菜单栏托盘保持静默运行，兼顾“独立桌面端操作便利”与“轻量后台驻留”。

---

## 二、统一主窗口整体交互蓝图

采用现代专业开发者工具（如 Docker Desktop、Linear、VS Code）的标准 **左侧主导航 Rail + 右侧核心工作区** 布局：

```text
┌───┬──────────────────┬────────────────────────────────────────────────────────────────────────┐
│ R │ [会话历史]       │  当前会话: 修复 JWT 鉴权漏洞                 [● Live]  [🌐 语言]  [复制诊断]  │
│ E ├──────────────────┼────────────────────────────────────────────────────────────────────────┤
│ L │ 🔍 搜索会话       │  [顶部 Step 进度锚点]                                                  │
│ A │                  │  ✓ 1.规划 (Codex) ──► ● 2.实现 (DeepSeek) ──► 3.Review (Grok) ──► 4.修复 │
│ Y │ ● 修复JWT漏洞    ├────────────────────────────────────────────────────────────────────────┤
│   │   2m 前 · 4步    │                                                                        │
│ ─ │   [🗑 级联删除]  │ ┌─ SECTION 1: Codex · 需求规划与分析 ──────────── [已完成 · 4s] ─────┐ │
│ 💬│                  │ │  分析鉴权流程，确定需在 src/auth/jwt.rs 引入新校验...                │ │
│   │ ✓ 重构连接池     │ └────────────────────────────────────────────────────────────────────┘ │
│ 🤖│   1h 前 · 2步    │                                                                        │
│   │                  │ ┌─ SECTION 2: DeepSeek · 代码实现 ─────────────── [执行中 ● 18s] ────┐ │
│ ⚙️│ ✓ 性能优化       │ │  💭 思考过程 (DeepSeek-R1, 耗时 12s, 已折叠)                          │ │
│   │   昨天 · 6步     │ │                                                                      │ │
│ 🛡️│                  │ │  已完成核心签名函数替换，正在验证编译状态。                           │ │
│   │                  │ │                                                                      │ │
│ 🔌│                  │ │  ▼ 终端命令执行 (Rust ansi_to_html 渲染):                            │ │
│   │                  │ │  ┌──────────────────────────────────────────────────────────────┐    │ │
│ 📊│                  │ │  │ $ cargo check --workspace                                    │    │ │
│ ─ │                  │ │  │    Finished dev [unoptimized + debuginfo] in 1.2s            │    │ │
│ 🌗│                  │ │  └──────────────────────────────────────────────────────────────┘    │ │
│   │                  │ │                                                                      │ │
│   │                  │ │  ▼ 改动代码审查 (内联 diff2html 渲染, 1 个文件改动):                 │ │
│   │                  │ │  ┌──────────────────────────────────────────────────────────────┐    │ │
│   │                  │ │  │ @@ -12,4 +12,6 @@ - legacy_verify()  + new_jwt_verify()       │    │ │
│   │                  │ │  └──────────────────────────────────────────────────────────────┘    │ │
│   │                  │ └────────────────────────────────────────────────────────────────────┘ │
│   │                  │                                                                        │
│   │                  │ ┌─ SECTION 3: Grok · 安全审查 ─────────────────── [排队中...] ────────┐ │
│   │                  │ └────────────────────────────────────────────────────────────────────┘ │
│   │                  │                                                                        │
│   │ [◀ 收起会话栏]   │                                                      [ ↓ 滚动到底部 ]  │
└───┴──────────────────┴────────────────────────────────────────────────────────────────────────┘
```

### 1. 左侧最窄导航栏（Main Rail，宽 ~56px）
* 顶部：Relay 品牌标识与 Codex 连通状态指示；
* 核心视图切换：
  - 💬 **任务流（Sessions）**：主工作区（会话列表 + AI 连续时间线）；
  - 🤖 **智能体（Agents）**：Profile 配置管理；
  - ⚙️ **运行时（Runtimes）**：CLI 自动探测与手动登记；
  - 🛡️ **安全策略（Policy）**：全局并发/沙箱与工作区规则；
  - 🔌 **Codex 集成（Codex）**：插件、MCP 状态与自检修复；
  - 📊 **系统诊断（Status）**：Daemon 状态与日志。
* 底部：深浅主题切换、多语言切换（中/英）、Daemon 运行绿灯。

### 2. 核心工作区一：💬 任务流（Sessions / Mission Stream）
* **会话侧栏（可折叠）**：
  - 宽 240px，提供底部 `[◀ / ▶]` 切换按钮，折叠后主时间线占满视口；
  - 增加**会话搜索过滤框**；
  - 状态呼吸灯（运行中绿色脉冲、完成灰色、失败告警）；
  - **会话级联删除**：鼠标悬浮显示垃圾桶，二次确认弹窗提示，级联清除 SQLite 历史事件。
* **顶部 Step 进度锚点**：
  - 显示阶段链路（如 `Codex · 规划` ➔ `DeepSeek · 实现` ➔ `Grok · Review`）；
  - 点击步骤触发 `element.scroll_into_view({ behavior: "smooth" })` 平滑定位。
* **单页连续工作流（按 Step 划分 Section）**：
  - **绝不按模型合并**，严格按 Step 作为 Section 划分，确保因果顺序连贯；
  - 普通文字渲染标准 Markdown；
  - 终端命令收纳为**可折叠终端胶囊**，展开查看由 `ansi_to_html` 转义的彩色终端流；
  - 代码改动就地嵌入 **内联 Diff 查看卡片**（集成 `diff2html`），具备红绿高亮与行号对比；
  - 深度思考收纳为 `💭 思考过程 (耗时 X 秒)` 折叠框。
* **实时流式传输与视觉滚动机制 (Streaming & Visual Scrolling)**：
  - **无需引入 WebSocket (WSS)**：完全复用现有的 `/api/stream` Server-Sent Events (SSE) 长连接（Axum Tokio 广播），单向低延迟推送，浏览器天然处理断线重连，无需增加复杂的双向 WSS 状态机；
  - **增量响应式追加 (Reactive Append)**：新事件到来时，通过 Leptos Signal 仅向当前 Step 末尾追加 DOM 节点，杜绝全量刷新或闪烁；
  - **智能吸底 (Smart Pin-to-Bottom)**：
    - **自动顺畅跟滚**：当用户滚动条位于底部（距离底端 `< 60px`）时，新内容输出时自动触发平滑滚动，页面像终端或 ChatGPT 一样平滑持续跟滚；
    - **视线锁定（防抢焦点）**：一旦用户手动向上滑动翻阅历史代码或 Diff，系统**立即冻结自动滚屏**，严禁将用户视线强行拽回底部；
    - **未读动态气泡**：滚动锁定期间若产生新事件，右下角浮现微动画气泡：`[ ↓ 产生了 N 条新动态 ]`，点击一键平滑跳转至最新进度并恢复自动吸底。

### 3. 核心工作区二：配置与管理视图（宽屏化改造）
* **🤖 智能体（Agents）**：
  - 告别单列堆叠，改为左右分栏（左侧 Profile 列表，右侧宽幅编辑表单）；
  - 提示词文本框高度提升至 8~10 行；
  - 4 个权限开关（读、写、Shell、网络）改为 **2×2 网格布局**；
  - **保存与删除按钮常驻视口底部（Sticky Footer）**，彻底告别滚屏寻找。
* **⚙️ 运行时（Runtimes）**：
  - 宽屏直接展示长达上百字符的可执行路径，不再强制 2 行折断与截断；
  - 运行时探针（Probe）输出和诊断日志可在右侧独立区域完整展示。
* **🛡️ 策略（Policy）**：
  - 左列为全局默认规则，右列为工作区覆盖规则，两栏并列，对比清晰；
  - 针对无原生沙箱的运行时，明确标出安全警示标签。

---

## 三、既有 UI/UX 缺陷与统一解决方案对照表

| 归属模块 | 既有缺陷现象 | 统一后的解决方案 |
| :--- | :--- | :--- |
| **窗口形态** | 420×640 弹窗失焦即关闭，容易误触丢失表单草稿 | 改为标准原生桌面窗口，常驻桌面，窗口位置与尺寸自动记忆 |
| **表单操作** | Agent 编辑表单超高，Save/Delete 按钮沉底滚出可视区 | 宽屏左右分栏布局 + 操作栏常驻吸底（Sticky），无需滚动寻找 |
| **Tab 导航** | 420px 宽度内强塞 5 个纯文字 Tab，平均仅 80px，极易换行与误触 | 左侧垂直 Rail 导航，带专属 Icon 与高对比度选中状态 |
| **审查能力** | Changes 仅显示 `+N / -N`，缺少代码前后对比 | 在 Step 时间线流内就地嵌入 `diff2html` 代码对比卡片 |
| **终端日志** | 命令输出与文本混杂，ANSI 转义符乱码，长日志撑爆页面 | 封装为可折叠终端胶囊，内置 `ansi_to_html` 输出彩色样式 |
| **模型思考** | Reasoning 过程直接平铺，稀释核心回复 | 默认收起为 `💭 深度思考` 抽屉，展示消耗时长 |
| **会话管理** | **无法删除历史 Session**，没有搜索，无底洞堆积 | 后端实现级联删除接口，前端悬浮垃圾桶 + 防误触确认弹窗 + 搜索框 |
| **排版字号** | 大量使用 `10px` 极小字体，深色模式对比度差 | 淘汰 10px，正文辅助信息提升至 12px，强化深色对比度 |
| **集成检测** | 全部项 OK 时，Codex 页面主按钮依然是黑底“安装” | 按钮状态感知自适应：已安装时变为 `✓ 已就绪` 状态 |

---

## 四、现成 UI 轮子与开发生态架构推荐

坚持使用原生 **Rust + Leptos (WASM)** 技术栈，通过引入成熟开源生态轮子，**彻底告别手写 1500 行原生 CSS 与手写 SVG**：

### 1. 核心轮子组合矩阵

| 场景需求 | 推荐开源轮子 | 引入方式 | 替代现有手写模块 / 收益 |
| :--- | :--- | :--- | :--- |
| **核心 UI 组件库** | [`thaw`](https://thawui.vercel.app/) (v0.4+) | Rust Crate (`Cargo.toml`) | **替代手写 `controls.rs`**。提供 `Button`、`Input`、`Select`、`Switch`、`Modal`、`Drawer`、`Tabs`、`Badge`、`Tooltip`，自带专业暗黑/明亮主题 |
| **矢量图标库** | [`leptos_icons`](https://github.com/carlosted/leptos-icons) | Rust Crate (`Cargo.toml`) | **替代手写 SVG Path**。直接使用 Lucide / Heroicons 数千个高清矢量图标 (`<Icon icon=icondata::LuTerminal />`) |
| **交互与滚动 Hook** | [`leptos-use`](https://leptos-use.rs/) | Rust Crate (`Cargo.toml`) | 相当于 Rust 版 `VueUse`。提供 `use_scroll`（吸底判断）、`use_clipboard`（带 copied 反馈复制）、`use_color_mode`（系统主题跟随）、`use_virtual_list` |
| **代码 Diff 审查** | [`diff2html`](https://diff2html.xyz/) | Web 静态引入 | 行业标杆，GitHub 样式代码前后高亮对比，支持 Unified/Split 双栏与折叠未改动行 |
| **终端 ANSI 转义** | [`ansi_to_html`](https://crates.io/crates/ansi_to_html) | Rust Crate (`Cargo.toml`) | **零 JS 依赖**。纯 Rust 在 WASM 端直接将终端控制字符与彩色转义码转为带样式的 HTML span |
| **Markdown 渲染** | [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark) | Rust Crate (`Cargo.toml`) | 官方最高性能的 CommonMark 解析引擎，极速安全 |
| **样式与原子化 CSS** | `Tailwind CSS` for Trunk | Trunk 预编译钩子 | 可选替换 1500 行手写 `styles.css`，直接采用 Tailwind 原子类与复制 Shadcn 风格组件 |

### 2. 依赖配置示例 (`apps/relay-desktop/ui/Cargo.toml`)

```toml
[dependencies]
leptos = { version = "0.8", features = ["csr"] }
# 核心 UI 组件库 (内置暗黑主题与完整表单/弹窗控件)
thaw = "0.4"
# 全量 Lucide 图标库
leptos_icons = { version = "0.4", features = ["lucide"] }
# 实用响应式工具 (吸底滚动 / 剪贴板 / 主题监听)
leptos-use = "0.14"
# 终端 ANSI 转义与 Markdown 引擎
ansi_to_html = "0.2"
pulldown-cmark = "0.12"
```

---

## 五、视觉美化与设计系统专项建议 (Visual Aesthetics & Design System)

为了让 Relay 呈现出一线 macOS 顶级开发者工具（如 Raycast、Linear、Ghostty、Cursor）的精致质感，建议在视觉层落实以下 6 项设计规范：

### 1. macOS 原生一体化顶栏（Titlebar Overlay & Traffic Lights）
* **红绿灯无缝融入**：在 Tauri 启用 `"titleBarStyle": "overlay"`，隐藏原生灰底标题栏。左上角红黄绿三色红绿灯直接内嵌于最左侧导航栏（Rail）上方，内边距对齐（`padding-top: 14px; padding-left: 14px;`）；
* **原生拖拽区**：导航栏与头部空白区域标记 `-webkit-app-region: drag`，按钮与交互元素标记 `no-drag`，实现随处拖拽窗口的原生 macOS 手感。

### 2. 现代暗色调色板与微毛玻璃材质（Palette & Vibrancy）
* **淘汰死黑，引入微蓝/石墨暗调**：
  - 底色从纯平死黑 `#09090b` 升级为具备深度层次的 **Zinc / Obsidian 系列**（如背景 `#0d0e11`、卡片底色 `#14161b`、悬浮高亮 `#1a1d24`）；
* **侧栏微毛玻璃（Vibrancy）**：
  - 左侧会话侧栏采用微弱透光背景：`backdrop-filter: blur(20px); background: rgba(18, 20, 26, 0.85);`；
* **极细发光边框（Hairline Borders）**：
  - 弃用粗糙实线边框，采用高精度单像素高光边框：`border: 1px solid rgba(255, 255, 255, 0.07);`，在深色背景下凸显卡片层次与质感。

### 3. 多模型专属视觉色彩标识（Model Identity Badges）
当前所有模型名字都是纯灰字，无法一眼区分角色。为各个外部 Agent 建立专有的**微饱和度色彩体系**：
* **Codex (Host)**：冷青蓝 / 祖母绿微光（`#10b981`，标志着统筹与中枢）；
* **DeepSeek (Coder)**：电光蓝（`#3b82f6`，高强度编码与推理）；
* **Claude / Grok (Reviewer)**：明艳琥珀色 / 珊瑚橙（`#f59e0b`，突出审查与警告）；
* **Antigravity (Tool Worker)**：量子紫（`#8b5cf6`）；
* **效果**：在顶部的 Step 进度条与下方的 Section 标题中，专属徽标搭配淡淡的半透明背景胶囊（如 `background: rgba(59, 130, 246, 0.12); color: #60a5fa;`），任务流由谁主导瞬间一清二楚。

### 4. 状态呼吸感与微动效（Breathing Glow & Micro-interactions）
* **运行态呼吸灯（Breathing Glow）**：
  - 正在运行的 Step 和 Worker 不再只是静态绿点，赋予它平缓的呼吸光晕：
    ```css
    @keyframes pulse-glow {
      0%, 100% { box-shadow: 0 0 0 0 rgba(74, 222, 128, 0.5); }
      50% { box-shadow: 0 0 0 6px rgba(74, 222, 128, 0); }
    }
    ```
* **思维链波浪光影（Thinking Wave）**：
  - DeepSeek-R1 思考中时，折叠框边缘呈现温和的流光微动效，呈现“正在深度推理”的生命感；
* **顺滑的折叠过渡**：
  - 终端胶囊和 Diff 展开收起时加入 `transition: max-height 250ms cubic-bezier(0.16, 1, 0.3, 1)`，避免生硬的 DOM 瞬切。

### 5. 排版系统升级与代码字体规范（Typography & Monospace）
* **彻底清退 10px 字体**：
  - 标题（Title）：`15px` / `Font-Weight: 600` / `-0.015em` 字间距；
  - 正文（Body）：`13px` / `Line-Height: 1.55`；
  - 辅助说明 / 时间戳（Caption）：最低不低于 `12px`；
* **代码字体黄金组合**：
  - 终端与路径优先选用 macOS 专属等宽字体：`"SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`；
  - 开启等宽数字对齐（`font-variant-numeric: tabular-nums`），保证耗时、时间戳、文件行数完全垂直对齐。

### 6. 极简开发风格空状态（Crafted Empty States）
* 淘汰纯文本的“No sessions yet”；
* 采用极简的等宽线框示意图（ASCII Line Art 或极轻量 SVG）+ 快捷键提示：
  > 📦 **等待任务指派**  
  > 在 Codex 中输入 `$relay 用 DeepSeek 重构登录模块`，任务流将实时投射于此处。

---

## 六、工程代码层面的直接收益

合并为单一主窗口不仅大幅提升用户体验，还将**显著降低代码复杂度与维护成本**：
1. **删除 `src-tauri/src/panel.rs` 中 300+ 行复杂窗口定位代码**：
   彻底移除 `place_under_tray`、屏幕边缘贴靠计算（`clamp`）、失焦自动隐藏、窗口尺寸防溢出逻辑。
2. **状态层彻底合流（消除双 Store 维护）**：
   无需再维护 `Store`（Inspector）与 `PanelStore`（Control Panel）两套割裂状态，整个前端收敛为一套响应式上下文，杜绝重复监听与同步开销。
3. **消除路由分流开销**：
   无需在入口处做 `dom::is_panel_path(&pathname)` 的分流渲染，Trunk 模块仅维护一个干净完整的 Desktop Shell。

---

## 七、分阶段实施路线图 (Roadmap)

- [ ] **Phase 1: 后端存储与 API 补齐（删除能力）**
  - 在 `crates/relay-storage` 实现会话与其关联事件的物理级联删除；
  - 在 `apps/relayd` 开放 `DELETE /api/sessions/:id` 路由；
  - 编写单元测试验证清理有效性。
- [ ] **Phase 2: Tauri 主窗口定义与恢复 Dock 栏常驻**
  - 移除 `Info.plist` 中的 `LSUIElement`，调整为 `ActivationPolicy::Regular` 恢复 Dock 图标与 `Cmd + Tab`；
  - 将窗口调整为标准原生窗口（默认尺寸 `1050 × 720`，支持自由缩放/记忆位置，启用 `overlay` 一体化顶栏）；
  - 托盘图标绑定左键点击显隐主窗口，右键保留原生快捷菜单；
  - 清理 `panel.rs` 中多余的 300+ 行窗口边缘定位代码。
- [ ] **Phase 3: Leptos 统一 AppShell 与左侧 Rail 导航**
  - 引入 `thaw`、`leptos_icons` 与 `leptos-use` 替换原先手写的组件与 SVG；
  - 重构 `main.rs`，建立包含 `Rail` 导航栏的统一布局结构；
  - 统一全局状态管理器（合并 `Store` 与 `PanelStore`）。
- [ ] **Phase 4: 任务流（Sessions）沉浸式时间线实现**
  - 左侧实现带搜索、状态灯、悬浮删除的会话栏；
  - 顶部实现基于 Step 的进度指示条兼平滑锚点定位；
  - 主视口实现单页连续 Section 流，内联折叠终端胶囊（`ansi_to_html`）与代码 Diff（`diff2html`）；
  - 实现基于 `leptos-use::use_scroll` 的智能吸底跟滚与回底按钮。
- [ ] **Phase 5: 配置管理视图（Agents / Policy / Runtimes）宽屏重排**
  - 改造 Agent 编辑器为宽屏左右分栏，开关 2×2 排布，操作栏吸底；
  - 优化全局与工作区 Policy 并列展示；
  - 全局样式统一清理 10px 字体，调优深色模式对比度。
