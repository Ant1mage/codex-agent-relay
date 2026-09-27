//! The tray's own strings.
//!
//! Ported from `packages/i18n`: the menu is localized on the desktop side (the
//! wire stays neutral), so only the keys the shell renders are carried here —
//! the ones `apps/menu-bar/src/menu-model.ts` used, plus the Codex check labels
//! `relay_api::CodexCheckId::label_key()` hands back. Placeholders are written
//! `{name}` exactly as in the TypeScript resources, so a message with a version
//! or a count stays one string per locale.

/// Every key the desktop shell can render.
pub mod key {
    pub const MENU_STATUS_STARTING: &str = "menu.status.starting";
    pub const MENU_STATUS_DAEMON_DOWN: &str = "menu.status.daemonDown";
    pub const MENU_STATUS_READY: &str = "menu.status.ready";
    pub const MENU_STATUS_RUNNING: &str = "menu.status.running";
    pub const MENU_STATUS_AWAITING: &str = "menu.status.awaiting";
    pub const MENU_STATUS_NEEDS_SETUP: &str = "menu.status.needsSetup";
    pub const MENU_STATUS_NO_RUNTIME: &str = "menu.status.noRuntime";
    pub const MENU_START_DAEMON: &str = "menu.startDaemon";
    pub const MENU_RESTART_DAEMON: &str = "menu.restartDaemon";
    pub const MENU_STARTING: &str = "menu.starting";
    pub const MENU_CHECK_UPDATES: &str = "menu.checkUpdates";
    pub const MENU_DOWNLOAD_UPDATE: &str = "menu.downloadUpdate";
    pub const MENU_INSTALL_UPDATE: &str = "menu.installUpdate";
    pub const MENU_UPDATE_AVAILABLE: &str = "menu.updateAvailable";
    pub const MENU_UPDATE_NONE: &str = "menu.updateNone";
    pub const MENU_UPDATE_CHECKING: &str = "menu.updateChecking";
    pub const MENU_UPDATE_UNSUPPORTED: &str = "menu.updateUnsupported";
    pub const MENU_DAEMON_STALE: &str = "menu.daemonStale";
    pub const MENU_OPEN_PANEL: &str = "menu.openPanel";
    pub const MENU_CODEX_SETTINGS: &str = "menu.codexSettings";
    pub const MENU_REPAIR_CODEX: &str = "menu.repairCodex";
    pub const MENU_INSTALL_CODEX: &str = "menu.installCodex";
    pub const MENU_OPEN_INSPECTOR: &str = "menu.openInspector";
    pub const MENU_SHOW_IN_INSPECTOR: &str = "menu.showInInspector";
    pub const MENU_REFRESH: &str = "menu.refresh";
    pub const MENU_DIAGNOSTICS: &str = "menu.diagnostics";
    pub const MENU_ACTIVE: &str = "menu.active";
    pub const MENU_SESSIONS: &str = "menu.sessions";
    pub const MENU_AGENTS: &str = "menu.agents";
    pub const MENU_CODEX: &str = "menu.codex";
    pub const MENU_CODEX_CONNECTED: &str = "menu.codexConnected";
    pub const MENU_CODEX_MISSING: &str = "menu.codexMissing";
    pub const MENU_CANCEL_WORKER: &str = "menu.cancelWorker";
    pub const MENU_STOP_ALL: &str = "menu.stopAll";
    pub const MENU_OPEN_WORKSPACE: &str = "menu.openWorkspace";
    pub const MENU_COPY_SESSION_ID: &str = "menu.copySessionId";
    pub const MENU_LAUNCH_AT_LOGIN: &str = "menu.launchAtLogin";
    pub const MENU_QUIT: &str = "menu.quit";
    pub const MENU_NO_SESSIONS: &str = "menu.noSessions";
    pub const MENU_NO_AGENTS: &str = "menu.noAgents";
    pub const MENU_AGENT_DISABLED: &str = "menu.agentDisabled";
    pub const PANEL_ADD_AGENT: &str = "panel.addAgent";
    pub const PANEL_RUNTIME: &str = "panel.runtime";
    pub const PANEL_NO_RUNTIMES: &str = "panel.noRuntimes";
    pub const PANEL_ADD_RUNTIME_MENU: &str = "panel.addRuntimeMenu";
    pub const PANEL_RESCAN: &str = "panel.rescan";
    pub const AGENTS_AUTH_REQUIRED: &str = "agents.authRequired";
    pub const AGENTS_NOT_INSTALLED: &str = "agents.notInstalled";

    /// Every key above plus the contract's Codex check labels, so the resources
    /// can be checked against each other.
    pub fn all_keys() -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = vec![
            MENU_STATUS_STARTING,
            MENU_STATUS_DAEMON_DOWN,
            MENU_STATUS_READY,
            MENU_STATUS_RUNNING,
            MENU_STATUS_AWAITING,
            MENU_STATUS_NEEDS_SETUP,
            MENU_STATUS_NO_RUNTIME,
            MENU_START_DAEMON,
            MENU_RESTART_DAEMON,
            MENU_STARTING,
            MENU_CHECK_UPDATES,
            MENU_DOWNLOAD_UPDATE,
            MENU_INSTALL_UPDATE,
            MENU_UPDATE_AVAILABLE,
            MENU_UPDATE_NONE,
            MENU_UPDATE_CHECKING,
            MENU_UPDATE_UNSUPPORTED,
            MENU_DAEMON_STALE,
            MENU_OPEN_PANEL,
            MENU_CODEX_SETTINGS,
            MENU_REPAIR_CODEX,
            MENU_INSTALL_CODEX,
            MENU_OPEN_INSPECTOR,
            MENU_SHOW_IN_INSPECTOR,
            MENU_REFRESH,
            MENU_DIAGNOSTICS,
            MENU_ACTIVE,
            MENU_SESSIONS,
            MENU_AGENTS,
            MENU_CODEX,
            MENU_CODEX_CONNECTED,
            MENU_CODEX_MISSING,
            MENU_CANCEL_WORKER,
            MENU_STOP_ALL,
            MENU_OPEN_WORKSPACE,
            MENU_COPY_SESSION_ID,
            MENU_LAUNCH_AT_LOGIN,
            MENU_QUIT,
            MENU_NO_SESSIONS,
            MENU_NO_AGENTS,
            MENU_AGENT_DISABLED,
            PANEL_ADD_AGENT,
            PANEL_RUNTIME,
            PANEL_NO_RUNTIMES,
            PANEL_ADD_RUNTIME_MENU,
            PANEL_RESCAN,
            AGENTS_AUTH_REQUIRED,
            AGENTS_NOT_INSTALLED,
        ];
        keys.push(relay_api::CodexCheckId::CodexCli.label_key());
        keys.push(relay_api::CodexCheckId::RelayMcp.label_key());
        keys.push(relay_api::CodexCheckId::RelaySkill.label_key());
        keys.push(relay_api::CodexCheckId::RelayPlugin.label_key());
        keys.push(relay_api::CodexCheckId::RelayHooks.label_key());
        keys
    }
}

/// The locales Relay ships. `resolve_locale` maps everything else onto `En`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    ZhCn,
}

impl Locale {
    /// The value the panel receives as `?lang=`.
    pub fn as_str(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhCn => "zh-CN",
        }
    }
}

/// Mirrors `resolveLocale` in packages/i18n: anything `zh*` is Chinese.
pub fn resolve_locale(language: Option<&str>) -> Locale {
    match language {
        Some(value) if value.to_lowercase().starts_with("zh") => Locale::ZhCn,
        _ => Locale::En,
    }
}

/// The system language, as far as the desktop shell can tell.
///
/// The Tauri shell has no built-in locale,
/// so the platform locale is read directly. An unknown locale falls back to `En`,
/// which is what `resolveLocale(undefined)` did.
pub fn system_locale() -> Locale {
    resolve_locale(sys_locale::get_locale().as_deref())
}

/// Looks a key up, falling back to English like `translate()` did.
pub fn translate(locale: Locale, key: &'static str) -> &'static str {
    let table = match locale {
        Locale::En => EN,
        Locale::ZhCn => ZH_CN,
    };
    let found = table
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, message)| *message);
    match found {
        Some(message) => message,
        None => EN
            .iter()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, message)| *message)
            .unwrap_or(key),
    }
}

/// Renders a key for one locale, replacing `{name}` placeholders.
#[derive(Debug, Clone, Copy)]
pub struct Translator {
    locale: Locale,
}

impl Translator {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn locale(self) -> Locale {
        self.locale
    }

    pub fn t(self, key: &'static str) -> String {
        translate(self.locale, key).to_string()
    }

    /// `t('menu.status.running', &[("count", "3")])`.
    pub fn tv(self, key: &'static str, values: &[(&str, &str)]) -> String {
        let mut message = translate(self.locale, key).to_string();
        for (name, value) in values {
            message = message.replace(&format!("{{{name}}}"), value);
        }
        message
    }
}

const EN: &[(&str, &str)] = &[
    ("menu.status.starting", "Relay · Starting the log service…"),
    (
        "menu.status.daemonDown",
        "Relay · Log service is not running",
    ),
    ("menu.status.ready", "Relay · Ready"),
    ("menu.status.running", "Relay · {count} running"),
    ("menu.status.awaiting", "Relay · {count} awaiting Codex"),
    ("menu.status.needsSetup", "Relay · Setup needed"),
    ("menu.status.noRuntime", "Relay · No runtime detected"),
    ("menu.startDaemon", "Start log service"),
    ("menu.restartDaemon", "Restart log service"),
    ("menu.starting", "Starting…"),
    ("menu.checkUpdates", "Check for updates…"),
    ("menu.downloadUpdate", "Download update"),
    ("menu.installUpdate", "Restart and install"),
    ("menu.updateAvailable", "Update {version} available"),
    ("menu.updateNone", "Relay is up to date"),
    ("menu.updateChecking", "Checking for updates…"),
    ("menu.updateUnsupported", "Updates need an installed build"),
    (
        "menu.daemonStale",
        "Log service is version {running} (app {app}) — restart it",
    ),
    ("menu.openPanel", "Control panel…"),
    ("menu.codexSettings", "Codex integration settings…"),
    ("menu.repairCodex", "Repair integration"),
    ("menu.installCodex", "Install in Codex…"),
    ("menu.openInspector", "Open inspector"),
    ("menu.showInInspector", "Show in inspector"),
    ("menu.refresh", "Refresh"),
    ("menu.diagnostics", "Copy diagnostics"),
    ("menu.active", "Active delegations"),
    ("menu.sessions", "Sessions"),
    ("menu.agents", "Agents"),
    ("menu.codex", "Codex integration"),
    ("menu.codexConnected", "Connected"),
    ("menu.codexMissing", "Not configured"),
    ("menu.cancelWorker", "Cancel worker"),
    ("menu.stopAll", "Stop all workers"),
    ("menu.openWorkspace", "Open workspace"),
    ("menu.copySessionId", "Copy session ID"),
    ("menu.launchAtLogin", "Launch at login"),
    ("menu.quit", "Quit Relay"),
    ("menu.noSessions", "No sessions yet"),
    ("menu.noAgents", "No agents configured"),
    ("menu.agentDisabled", "disabled"),
    ("panel.addAgent", "New agent…"),
    ("panel.runtime", "Runtimes"),
    ("panel.noRuntimes", "No runtime detected"),
    ("panel.addRuntimeMenu", "Add runtime…"),
    ("panel.rescan", "Rescan runtimes"),
    ("agents.authRequired", "Authentication required"),
    ("agents.notInstalled", "Not installed"),
    ("onboarding.check.codex-cli", "Codex detected"),
    ("onboarding.check.relay-mcp", "Relay MCP configured"),
    ("onboarding.check.relay-skill", "Relay skill installed"),
    ("onboarding.check.relay-plugin", "Relay plugin installed"),
    ("onboarding.check.relay-hooks", "Relay hooks registered"),
];

const ZH_CN: &[(&str, &str)] = &[
    ("menu.status.starting", "Relay · 正在启动日志服务…"),
    ("menu.status.daemonDown", "Relay · 日志服务未运行"),
    ("menu.status.ready", "Relay · 就绪"),
    ("menu.status.running", "Relay · {count} 个运行中"),
    ("menu.status.awaiting", "Relay · {count} 个等待 Codex"),
    ("menu.status.needsSetup", "Relay · 需要配置"),
    ("menu.status.noRuntime", "Relay · 未检测到 Runtime"),
    ("menu.startDaemon", "启动日志服务"),
    ("menu.restartDaemon", "重启日志服务"),
    ("menu.starting", "正在启动…"),
    ("menu.checkUpdates", "检查更新…"),
    ("menu.downloadUpdate", "下载更新"),
    ("menu.installUpdate", "重启并安装"),
    ("menu.updateAvailable", "发现新版本 {version}"),
    ("menu.updateNone", "Relay 已是最新版本"),
    ("menu.updateChecking", "正在检查更新…"),
    ("menu.updateUnsupported", "需要安装版才能检查更新"),
    (
        "menu.daemonStale",
        "日志服务版本 {running}（App {app}）— 建议重启",
    ),
    ("menu.openPanel", "打开配置面板…"),
    ("menu.codexSettings", "Codex 集成设置…"),
    ("menu.repairCodex", "修复集成"),
    ("menu.installCodex", "安装到 Codex…"),
    ("menu.openInspector", "打开检查器"),
    ("menu.showInInspector", "在检查器中查看"),
    ("menu.refresh", "刷新"),
    ("menu.diagnostics", "复制诊断信息"),
    ("menu.active", "进行中的委派"),
    ("menu.sessions", "会话"),
    ("menu.agents", "智能体"),
    ("menu.codex", "Codex 集成"),
    ("menu.codexConnected", "已连接"),
    ("menu.codexMissing", "未配置"),
    ("menu.cancelWorker", "取消 worker"),
    ("menu.stopAll", "停止所有 worker"),
    ("menu.openWorkspace", "打开工作区"),
    ("menu.copySessionId", "复制会话 ID"),
    ("menu.launchAtLogin", "登录时启动"),
    ("menu.quit", "退出 Relay"),
    ("menu.noSessions", "还没有会话"),
    ("menu.noAgents", "还没有配置智能体"),
    ("menu.agentDisabled", "已停用"),
    ("panel.addAgent", "新建智能体…"),
    ("panel.runtime", "运行时"),
    ("panel.noRuntimes", "未检测到运行时"),
    ("panel.addRuntimeMenu", "添加运行时…"),
    ("panel.rescan", "重新扫描运行时"),
    ("agents.authRequired", "需要认证"),
    ("agents.notInstalled", "未安装"),
    ("onboarding.check.codex-cli", "已检测到 Codex"),
    ("onboarding.check.relay-mcp", "已配置 Relay MCP"),
    ("onboarding.check.relay-skill", "已安装 Relay skill"),
    ("onboarding.check.relay-plugin", "已安装 Relay 插件"),
    ("onboarding.check.relay-hooks", "已注册 Relay hooks"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_resolve_like_the_typescript_translator() {
        assert_eq!(resolve_locale(Some("zh-CN")), Locale::ZhCn);
        assert_eq!(resolve_locale(Some("zh-Hans-CN")), Locale::ZhCn);
        assert_eq!(resolve_locale(Some("ZH")), Locale::ZhCn);
        assert_eq!(resolve_locale(Some("en-US")), Locale::En);
        assert_eq!(resolve_locale(None), Locale::En);
        assert_eq!(Locale::ZhCn.as_str(), "zh-CN");
        assert_eq!(Locale::En.as_str(), "en");
    }

    #[test]
    fn every_menu_key_exists_in_every_locale() {
        for key in key::all_keys() {
            for locale in [Locale::En, Locale::ZhCn] {
                let table = match locale {
                    Locale::En => EN,
                    Locale::ZhCn => ZH_CN,
                };
                assert!(
                    table.iter().any(|(candidate, _)| *candidate == key),
                    "{key} is missing from {locale:?}"
                );
            }
        }
    }

    /// The `{name}` placeholders a message declares, in order of appearance.
    fn placeholders(message: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = message;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            found.push(&rest[start + 1..start + end]);
            rest = &rest[start + end + 1..];
        }
        found
    }

    #[test]
    fn placeholders_are_filled_and_kept_identical_across_locales() {
        // A message whose placeholders drifted would silently render `{count}`.
        for key in key::all_keys() {
            let en = EN
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .unwrap()
                .1;
            let zh = ZH_CN
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .unwrap()
                .1;
            assert_eq!(
                placeholders(en),
                placeholders(zh),
                "placeholders differ for {key}"
            );
        }
        let en = Translator::new(Locale::En);
        let zh = Translator::new(Locale::ZhCn);
        assert_eq!(
            en.tv(key::MENU_STATUS_RUNNING, &[("count", "3")]),
            "Relay · 3 running"
        );
        assert_eq!(
            zh.tv(key::MENU_STATUS_RUNNING, &[("count", "3")]),
            "Relay · 3 个运行中"
        );
        assert_eq!(
            en.tv(key::MENU_UPDATE_AVAILABLE, &[("version", "0.3.0")]),
            "Update 0.3.0 available"
        );
        assert_eq!(
            en.tv(
                key::MENU_DAEMON_STALE,
                &[("running", "0.1.0"), ("app", "0.2.0")]
            ),
            "Log service is version 0.1.0 (app 0.2.0) — restart it"
        );
        // An unknown key renders as itself rather than panicking.
        assert_eq!(en.t("menu.unknown"), "menu.unknown");
    }
}
