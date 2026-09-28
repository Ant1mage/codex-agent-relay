//! Message catalogue, ported from `packages/i18n/src/index.ts`.
//!
//! Keys are unchanged so an existing translation memory, the tray and the panel
//! keep matching. Only the messages this bundle renders are carried over; a
//! missing key falls back to English and then to the key itself, mirroring the
//! TypeScript `translate()`.

use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    ZhCn,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhCn => "zh-CN",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Locale::En),
            "zh-CN" => Some(Locale::ZhCn),
            _ => None,
        }
    }

    /// The other language, for the inspector's single toggle button.
    pub fn toggled(self) -> Self {
        match self {
            Locale::En => Locale::ZhCn,
            Locale::ZhCn => Locale::En,
        }
    }
}

/// `resolveLocale` from the TypeScript package: anything Chinese is zh-CN.
pub fn resolve_locale(language: &str) -> Locale {
    if language.to_lowercase().starts_with("zh") {
        Locale::ZhCn
    } else {
        Locale::En
    }
}

/// key, English, Simplified Chinese.
const MESSAGES: &[(&str, &str, &str)] = &[
    ("app.name", "Relay", "Relay"),
    ("nav.sessions", "Sessions", "会话"),
    ("nav.agents", "Agents", "智能体"),
    ("sessions.empty", "No sessions yet", "还没有会话"),
    (
        "sessions.emptyHint",
        "Start a Relay delegation from Codex to see it here.",
        "从 Codex 发起一次 Relay 委派后，会话会显示在这里。",
    ),
    ("sessions.delete", "Delete session", "删除会话"),
    ("sessions.deleteConfirm", "Delete this session and all its history?", "确定删除此会话及其所有历史记录？"),
    ("sessions.deleted", "Session deleted", "会话已删除"),
    ("timeline.title", "Mission Timeline", "任务流水线"),
    ("timeline.empty", "Select a session to view its timeline", "选择一个会话查看其任务流水线"),
    ("timeline.scrollToBottom", "Jump to latest", "跳至最新"),
    ("timeline.autoScroll", "Auto-scroll", "自动滚动"),
    ("timeline.accept", "Accept", "接受"),
    ("timeline.resume", "Resume", "继续"),
    ("timeline.feedbackPlaceholder", "Add instructions or feedback for next step…", "输入补充指令或反馈意见…"),
    ("run.accepted", "Run accepted", "任务已接受"),
    ("run.resumed", "Run resumed", "任务已继续"),
    ("runs.title", "Runs", "运行任务"),
    ("steps.step", "Step", "步骤"),
    ("console.title", "Console", "控制台"),
    ("console.empty", "No observable activity for this step yet.", "此步骤还没有可观察活动。"),
    ("console.rawOutput", "Raw Output", "原始输出"),
    ("console.changes", "Changes", "改动"),
    ("console.noRawOutput", "No raw output was recorded.", "没有记录原始输出。"),
    ("console.noChanges", "No file changes were reported.", "没有报告文件改动。"),
    ("console.read", "Read", "读取"),
    ("console.search", "Search", "搜索"),
    ("console.edit", "Edit", "编辑"),
    ("console.command", "Command", "命令"),
    ("console.test", "Test", "测试"),
    ("console.result", "Result", "结果"),
    ("console.error", "Error", "错误"),
    ("console.warning", "Warning", "警告"),
    ("console.status", "Status", "状态"),
    ("console.files", "files", "个文件"),
    ("cliInfo.runtime", "Runtime", "运行时"),
    ("cliInfo.started", "Started", "开始时间"),
    ("agents.title", "Agents", "智能体"),
    ("agents.authRequired", "Authentication required", "需要认证"),
    ("agents.notInstalled", "Not installed", "未安装"),
    ("agents.edit", "Edit profile", "编辑 Profile"),
    ("agents.name", "Name", "名称"),
    ("agents.description", "Description", "说明"),
    ("agents.model", "Model", "模型"),
    ("agents.modelHint", "For example Flash or Pro", "例如 Flash 或 Pro"),
    ("agents.reasoning", "Reasoning", "推理强度"),
    ("agents.enabled", "Enabled", "启用"),
    ("agents.read", "Read workspace", "读取工作区"),
    ("agents.write", "Write workspace", "写入工作区"),
    ("agents.shell", "Execute commands", "执行命令"),
    ("agents.network", "Network access", "访问网络"),
    ("agents.permissions", "Permissions", "权限"),
    ("agents.runtimeDefault", "Runtime default", "运行时默认"),
    ("agents.readingRuntime", "Reading this runtime’s options…", "正在读取该运行时的选项…"),
    ("agents.noModelList", "This CLI does not publish a model list", "该 CLI 未公开模型列表"),
    (
        "agents.noReasoningLevels",
        "This CLI does not expose reasoning levels",
        "该 CLI 未提供推理强度选项",
    ),
    ("onboarding.check.codex-cli", "Codex CLI detection", "Codex CLI 检测"),
    ("onboarding.check.relay-mcp", "Relay MCP configuration", "Relay MCP 配置"),
    ("onboarding.check.relay-skill", "Relay skill installation", "Relay skill 安装"),
    ("onboarding.check.relay-plugin", "Relay plugin installation", "Relay 插件安装"),
    ("onboarding.check.relay-hooks", "Relay hooks registration", "Relay hooks 注册"),
    ("onboarding.back", "Back", "返回"),
    ("panel.addRuntime", "Add runtime", "添加运行时"),
    ("panel.runtime.edit", "Edit runtime", "编辑运行时"),
    ("panel.runtime.name", "Name", "名称"),
    ("panel.runtime.nameHint", "Optional label shown in Relay.", "可选，只用于 Relay 中显示。"),
    ("panel.runtime.sourceDetected", "Detected", "自动检测"),
    ("panel.runtime.sourceManual", "Manual", "手动添加"),
    ("panel.runtime.health.available", "Available", "可用"),
    ("panel.runtime.health.authentication_required", "Sign in required", "需要登录"),
    ("panel.runtime.health.unavailable", "Unavailable", "不可用"),
    ("panel.runtime.capabilities", "capabilities", "项能力"),
    ("panel.runtime.onThisMac", "runtimes on this Mac", "个运行时"),
    ("panel.runtime.diagnostics", "Scan diagnostics", "扫描诊断"),
    ("panel.runtime.deleteTitle", "Delete runtime?", "删除运行时？"),
    (
        "panel.runtime.deleteConfirm",
        "This removes the manually registered runtime. The CLI itself will not be uninstalled.",
        "这会移除手动注册的运行时，但不会卸载 CLI 本身。",
    ),
    (
        "panel.runtime.pathHint",
        "Absolute path to the CLI executable. Relay runs it once with --version before saving.",
        "CLI 可执行文件的绝对路径。保存前 Relay 会用 --version 跑一次验证。",
    ),
    ("panel.runtime.path", "Executable", "可执行文件"),
    ("panel.runtime.check", "Test", "检测"),
    ("panel.runtime.checking", "Testing…", "检测中…"),
    ("panel.runtime.probeOk", "Executable responded", "可执行文件已响应"),
    ("panel.policy", "Policy", "策略"),
    ("panel.runtime", "Runtimes", "运行时"),
    ("panel.status", "Status", "状态"),
    ("panel.newAgent", "New agent", "新建智能体"),
    ("panel.agent.profiles", "profiles", "个 Profile"),
    ("panel.delete", "Delete", "删除"),
    ("panel.agent.deleteTitle", "Delete agent profile?", "删除 Agent Profile？"),
    ("panel.deleteConfirm", "Delete this agent profile?", "确认删除这个 Agent Profile？"),
    ("panel.saved", "Saved", "已保存"),
    ("panel.saveFailed", "Could not save", "保存失败"),
    ("panel.noRuntimes", "No runtime detected", "未检测到运行时"),
    ("panel.rescan", "Rescan runtimes", "重新扫描运行时"),
    ("panel.openInspector", "Open Inspector", "打开 Inspector"),
    ("panel.install", "Install in Codex", "安装到 Codex"),
    ("panel.repair", "Repair", "修复"),
    ("panel.update", "Update", "更新"),
    ("panel.remove", "Remove from Codex", "从 Codex 移除"),
    ("panel.codex.removeTitle", "Remove Relay from Codex?", "从 Codex 移除 Relay？"),
    (
        "panel.removeConfirm",
        "Remove Relay from Codex? The MCP server, plugin, skill and hooks will all be uninstalled.",
        "从 Codex 中移除 Relay？MCP server、插件、skill 与 hooks 都会被卸载。",
    ),
    ("panel.instructions", "Instructions", "Instructions"),
    ("panel.modelAuto", "Runtime default", "跟随运行时默认"),
    ("panel.workspace.clear", "Clear this override", "清除该工作区覆盖"),
    ("panel.workspace.none", "No workspace override yet", "还没有工作区覆盖"),
    ("panel.daemonDown", "Relay service is unreachable", "无法连接 Relay 服务"),
    ("panel.agents.empty", "No agent profile yet. Create one above.", "还没有 Agent Profile，点上面的按钮新建一个。"),
    ("panel.close", "Close", "关闭"),
    ("panel.actions", "Actions", "操作"),
    ("settings.diagnosticsEmpty", "No diagnostics reported.", "没有诊断信息。"),
    ("settings.global", "Global defaults", "全局默认值"),
    ("settings.maxRuns", "Maximum concurrent runs", "最大并发任务数"),
    ("settings.maxWriters", "Maximum concurrent writers", "最大并发写入数"),
    ("settings.requireWorktree", "Require worktrees for parallel writers", "并行写入时要求 worktree"),
    ("settings.workspaceSection", "Workspace", "工作区"),
    ("settings.workspace", "Workspace override", "工作区覆盖"),
    ("settings.allowWrite", "Allow workspace writes", "允许写入工作区"),
    ("settings.allowCommands", "Allow command execution", "允许执行命令"),
    ("settings.allowNetwork", "Allow network access", "允许访问网络"),
    (
        "settings.policyAdvisory",
        "No detected runtime enforces these switches itself: Relay refuses to start a run that breaks them, but the agent CLI still runs with its own permissions.",
        "当前检测到的 runtime 都无法自行执行这些开关：Relay 会拒绝启动违反策略的任务，但 Agent CLI 仍以它自己的权限运行。",
    ),
    (
        "settings.policyEnforced",
        "Workspace access is enforced by the runtime sandbox for: ",
        "工作区访问由 runtime 沙箱实际执行，适用于：",
    ),
    ("inspector.title", "Relay Inspector", "Relay 检查器"),
    ("inspector.live", "Live", "实时"),
    ("inspector.connecting", "Connecting…", "连接中…"),
    ("inspector.offline", "Disconnected", "已断开"),
    ("inspector.tokenTitle", "Open the inspector from the Relay menu bar", "请从 Relay 菜单栏打开检查器"),
    (
        "inspector.tokenBody",
        "This page needs the daemon token. Use \"Open inspector\" in the Relay menu bar, or copy the token from ~/.relay/server.json into the URL fragment, for example http://127.0.0.1:7352/#t=<token>.",
        "此页面需要 daemon 令牌。请使用 Relay 菜单栏里的“打开检查器”，或把 ~/.relay/server.json 中的 token 拼进 URL 片段，例如 http://127.0.0.1:7352/#t=<token>。",
    ),
    ("inspector.codexFixInMenuBar", "Fix this in the Relay menu bar → Codex integration", "请在 Relay 菜单栏 → Codex 集成 中处理"),
    ("inspector.copyDiagnostics", "Copy diagnostics", "复制诊断信息"),
    ("inspector.diagnosticsCopied", "Diagnostics copied to the clipboard", "诊断信息已复制到剪贴板"),
    ("inspector.codexMissing", "Codex is not wired up yet", "Codex 尚未接入"),
    (
        "inspector.codexMissingBody",
        "Relay needs its MCP server and skill in Codex before delegations can appear here.",
        "需要先把 Relay 的 MCP server 与 skill 安装到 Codex，委派才会出现在这里。",
    ),
    ("inspector.noRun", "No run in this session yet", "此会话还没有运行任务"),
    (
        "inspector.noRunHint",
        "Runs appear here as soon as Codex delegates a task through Relay.",
        "当 Codex 通过 Relay 委派任务后，运行记录会出现在这里。",
    ),
    ("inspector.stopWorker", "Stop worker", "停止 worker"),
    ("inspector.stopped", "Cancellation requested", "已请求取消"),
    ("inspector.events", "events", "条事件"),
    ("inspector.steps", "Steps", "步骤"),
    ("menu.codex", "Codex integration", "Codex 集成"),
    ("menu.codexConnected", "Connected", "连接正常"),
    ("menu.codexMissing", "Not configured", "未配置"),
    ("common.active", "Active", "活跃"),
    ("common.refresh", "Refresh", "刷新"),
    ("run.status.queued", "Queued", "排队中"),
    ("run.status.starting", "Starting", "启动中"),
    ("run.status.running", "Running", "运行中"),
    ("run.status.awaiting_host", "Awaiting Codex", "等待 Codex"),
    ("run.status.completed", "Completed", "已完成"),
    ("run.status.failed", "Failed", "失败"),
    ("run.status.cancelled", "Cancelled", "已取消"),
    ("run.status.interrupted", "Interrupted", "已中断"),
    ("run.status.orphaned", "Orphaned", "已失联"),
    ("action.cancel", "Cancel", "取消"),
    ("action.save", "Save", "保存"),
    ("language.en", "English", "英语"),
    ("language.zh-CN", "Simplified Chinese", "简体中文"),
    // Status tab: a surface the old panel did not have.
    ("status.daemon", "Daemon", "守护进程"),
    ("status.version", "Version", "版本"),
    ("status.port", "Port", "端口"),
    ("status.database", "Database", "数据库"),
    ("status.pid", "PID", "进程号"),
    ("status.started", "Started", "启动时间"),
    ("status.healthy", "Healthy", "运行正常"),
    ("status.unreachable", "Unreachable", "无法连接"),
    ("status.sessions", "Sessions", "会话"),
    ("status.runs", "Runs", "运行任务"),
    ("status.runtimes", "Runtimes", "运行时"),
    ("status.profiles", "Profiles", "Profile"),
    ("status.counts", "State", "状态统计"),
];

/// Looks a key up for one locale: exact match, then English, then the key.
pub fn translate(locale: Locale, key: &str) -> String {
    let entry = MESSAGES.iter().find(|(candidate, _, _)| *candidate == key);
    match (entry, locale) {
        (Some((_, _, chinese)), Locale::ZhCn) if !chinese.is_empty() => (*chinese).to_string(),
        (Some((_, english, _)), _) => (*english).to_string(),
        (None, _) => key.to_string(),
    }
}

/// A translator bound to the locale signal, so a view re-renders in the new
/// language the moment the signal changes.
#[derive(Clone, Copy)]
pub struct Translator {
    pub locale: RwSignal<Locale>,
}

impl Translator {
    pub fn new(locale: RwSignal<Locale>) -> Self {
        Self { locale }
    }

    pub fn t(&self, key: &str) -> String {
        translate(self.locale.get(), key)
    }
}
