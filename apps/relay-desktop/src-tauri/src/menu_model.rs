//! The tray menu, as data.
//!
//! Ported from `apps/menu-bar/src/menu-model.ts`: nothing here touches Tauri or
//! AppKit, so the whole menu can be unit tested without a GUI session. `tray.rs`
//! turns these items into native menu objects and the shell performs the actions
//! they carry.

use relay_api::{MenuBlocked, MenuStatus, MenuView};
use serde::{Deserialize, Serialize};

use crate::i18n::{self, Translator};
use crate::updater::UpdateState;

/// The platform the menu model is built for, so the model stays comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuBarPlatform {
    Darwin,
    Win32,
    Linux,
}

impl MenuBarPlatform {
    /// The platform the shell is running on.
    pub fn current() -> Self {
        match std::env::consts::OS {
            "macos" => MenuBarPlatform::Darwin,
            "windows" => MenuBarPlatform::Win32,
            _ => MenuBarPlatform::Linux,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonStatus {
    Running,
    Stopped,
    Starting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelTab {
    Agents,
    Policy,
    Codex,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelIntent {
    NewAgent,
    EditAgent,
    AddRuntime,
    CodexActions,
}

/// Everything a menu entry can do. The tray never decides which agent should run:
/// each action is either an HTTP call or an OS-level switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MenuBarAction {
    OpenInspector {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
    OpenPanel {
        tab: PanelTab,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<PanelIntent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
    },
    Rescan,
    RepairCodex,
    StartDaemon,
    RestartDaemon,
    CancelWorker {
        worker_session_id: String,
    },
    CancelSession {
        host_session_id: String,
    },
    OpenWorkspace {
        cwd: String,
    },
    CopySessionId {
        host_session_id: String,
    },
    CopyDiagnostics,
    /// Kept for parity with the TypeScript union. The menu offers
    /// `repair-codex` for both "repair" and "install": the install path is idempotent.
    InstallCodex,
    Refresh,
    CheckUpdates,
    ToggleLaunchAtLogin,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItemKind {
    Normal,
    Header,
    Separator,
    Checkbox,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuBarItem {
    pub kind: MenuItemKind,
    pub label: String,
    pub checked: Option<bool>,
    pub enabled: Option<bool>,
    pub accelerator: Option<String>,
    pub action: Option<MenuBarAction>,
    pub submenu: Option<Vec<MenuBarItem>>,
}

impl MenuBarItem {
    fn separator() -> Self {
        Self {
            kind: MenuItemKind::Separator,
            label: String::new(),
            checked: None,
            enabled: None,
            accelerator: None,
            action: None,
            submenu: None,
        }
    }

    /// A header is a disabled entry: the menu shows it, the user cannot pick it.
    fn header(label: impl Into<String>) -> Self {
        Self {
            kind: MenuItemKind::Header,
            label: label.into(),
            checked: None,
            enabled: Some(false),
            accelerator: None,
            action: None,
            submenu: None,
        }
    }

    fn normal(label: impl Into<String>) -> Self {
        Self {
            kind: MenuItemKind::Normal,
            label: label.into(),
            checked: None,
            enabled: None,
            accelerator: None,
            action: None,
            submenu: None,
        }
    }

    fn checkbox(label: impl Into<String>, checked: bool) -> Self {
        Self {
            kind: MenuItemKind::Checkbox,
            label: label.into(),
            checked: Some(checked),
            enabled: None,
            accelerator: None,
            action: None,
            submenu: None,
        }
    }

    fn action(mut self, action: MenuBarAction) -> Self {
        self.action = Some(action);
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    fn accelerator(mut self, accelerator: impl Into<String>) -> Self {
        self.accelerator = Some(accelerator.into());
        self
    }

    fn submenu(mut self, submenu: Vec<MenuBarItem>) -> Self {
        self.submenu = Some(submenu);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMismatch {
    pub running: String,
    pub app: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarView {
    /// The locale the tray renders in, and the `?lang=` the panel receives.
    pub locale: String,
    pub platform: MenuBarPlatform,
    pub daemon: DaemonStatus,
    /// Absent while the daemon is not reachable; the menu then offers to start it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<MenuView>,
    /// Why the daemon is not reachable, shown as the disabled status line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Tray↔daemon version skew, which an app update can create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_version_mismatch: Option<VersionMismatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateState>,
    pub launch_at_login: bool,
    /// Browser the inspector opens in, for the menu's own labelling.
    pub browser: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_workers: Option<usize>,
}

impl MenuBarView {
    pub fn translator(&self) -> Translator {
        Translator::new(i18n::resolve_locale(Some(&self.locale)))
    }

    /// The model as the string that decides whether the menu has to be rebuilt.
    pub fn serialized(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

pub const DEFAULT_MAX_MENU_SESSIONS: usize = 8;
pub const DEFAULT_MAX_MENU_WORKERS: usize = 12;

/// How many characters of an error the disabled header keeps.
const ERROR_HEADER_CHARS: usize = 80;

/// The first line of the menu: what is happening, or why nothing can.
pub fn menu_bar_status_label(view: &MenuBarView) -> String {
    let t = view.translator();
    if view.daemon == DaemonStatus::Starting {
        return t.t(i18n::key::MENU_STATUS_STARTING);
    }
    let Some(menu) = view
        .menu
        .as_ref()
        .filter(|_| view.daemon == DaemonStatus::Running)
    else {
        return t.t(i18n::key::MENU_STATUS_DAEMON_DOWN);
    };
    if menu.running_workers > 0 {
        return t.tv(
            i18n::key::MENU_STATUS_RUNNING,
            &[("count", &menu.running_workers.to_string())],
        );
    }
    if menu.awaiting_host > 0 {
        return t.tv(
            i18n::key::MENU_STATUS_AWAITING,
            &[("count", &menu.awaiting_host.to_string())],
        );
    }
    if menu.status == MenuStatus::NoRuntime {
        return t.t(i18n::key::MENU_STATUS_NO_RUNTIME);
    }
    if menu.status == MenuStatus::NeedsSetup {
        return t.t(i18n::key::MENU_STATUS_NEEDS_SETUP);
    }
    t.t(i18n::key::MENU_STATUS_READY)
}

fn with_count(label: &str, count: usize) -> String {
    format!("{label} ({count})")
}

/// Truncates like `String.slice(0, 80)`, but on character boundaries so a
/// non-ASCII error message cannot be cut mid-codepoint.
fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    value.chars().take(limit).collect()
}

/// The wire spelling of a value that serializes as a string
/// (`RuntimeHealth::AuthenticationRequired` → `authentication_required`).
fn wire_string<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The App update section. It is deliberately independent of the daemon: a new
/// Relay can be available while the log service is down, and the Codex
/// integration has its own lifecycle.
fn update_items(t: Translator, view: &MenuBarView) -> Vec<MenuBarItem> {
    let mut items = vec![MenuBarItem::header(t.tv(
        i18n::key::MENU_VERSION,
        &[("version", env!("CARGO_PKG_VERSION"))],
    ))];
    let update = view.update.as_ref();
    match update.map(|update| update.status) {
        Some(crate::updater::UpdateStatus::Available) => {
            let update = update.expect("update state");
            items.push(
                MenuBarItem::normal(t.tv(
                    i18n::key::MENU_UPDATE_AVAILABLE,
                    &[("version", update.version.as_deref().unwrap_or(""))],
                ))
                .enabled(false),
            );
        }
        Some(crate::updater::UpdateStatus::Checking) => {
            items.push(MenuBarItem::header(t.t(i18n::key::MENU_UPDATE_CHECKING)));
        }
        Some(crate::updater::UpdateStatus::None) => {
            items.push(MenuBarItem::header(t.t(i18n::key::MENU_UPDATE_NONE)));
        }
        Some(crate::updater::UpdateStatus::Error) => {
            let message = update
                .and_then(|update| update.message.clone())
                .unwrap_or_else(|| "update error".to_string());
            items.push(MenuBarItem::header(format!("⚠︎ {message}")));
        }
        // `idle` renders nothing at all.
        _ => {}
    }
    items.push(
        MenuBarItem::normal(t.t(i18n::key::MENU_CHECK_UPDATES))
            .enabled(
                update.map(|update| update.status) != Some(crate::updater::UpdateStatus::Checking),
            )
            .action(MenuBarAction::CheckUpdates),
    );
    items
}

/// Builds the whole menu. Everything is either a projection of the daemon's
/// snapshot or an OS-level switch.
pub fn build_menu_bar_items(view: &MenuBarView) -> Vec<MenuBarItem> {
    let t = view.translator();
    let max_sessions = view.max_sessions.unwrap_or(DEFAULT_MAX_MENU_SESSIONS);
    let max_workers = view.max_workers.unwrap_or(DEFAULT_MAX_MENU_WORKERS);
    let menu = view.menu.as_ref();
    let mut items = vec![MenuBarItem::header(menu_bar_status_label(view))];

    let Some(menu) = menu.filter(|_| view.daemon == DaemonStatus::Running) else {
        // The daemon-not-running variant: recover the log service, then the
        // actions that do not need it.
        items.push(
            MenuBarItem::normal(if view.daemon == DaemonStatus::Starting {
                t.t(i18n::key::MENU_STARTING)
            } else {
                t.t(i18n::key::MENU_START_DAEMON)
            })
            .enabled(view.daemon != DaemonStatus::Starting)
            .action(MenuBarAction::StartDaemon),
        );
        items.push(
            MenuBarItem::normal(t.t(i18n::key::MENU_RESTART_DAEMON))
                .enabled(view.daemon != DaemonStatus::Starting)
                .action(MenuBarAction::RestartDaemon),
        );
        if let Some(error) = view.error.as_deref() {
            items.push(MenuBarItem::header(format!(
                "⚠︎ {}",
                truncate_chars(error, ERROR_HEADER_CHARS)
            )));
        }
        items.push(MenuBarItem::separator());
        items.push(
            MenuBarItem::normal(t.t(i18n::key::MENU_DIAGNOSTICS))
                .action(MenuBarAction::CopyDiagnostics),
        );
        items.push(MenuBarItem::separator());
        // App updates do not depend on the log service being up.
        items.extend(update_items(t, view));
        items.push(MenuBarItem::separator());
        items.push(
            MenuBarItem::normal(t.t(i18n::key::MENU_QUIT))
                .accelerator("CmdOrCtrl+Q")
                .action(MenuBarAction::Quit),
        );
        return items;
    };

    items.push(
        MenuBarItem::normal(t.t(i18n::key::MENU_OPEN_INSPECTOR))
            .accelerator("CmdOrCtrl+O")
            .action(MenuBarAction::OpenInspector {
                host_session_id: None,
                run_id: None,
            }),
    );
    items.push(
        MenuBarItem::normal(t.t(i18n::key::MENU_OPEN_PANEL))
            .accelerator("CmdOrCtrl+,")
            .action(MenuBarAction::OpenPanel {
                tab: PanelTab::Agents,
                intent: None,
                profile_id: None,
            }),
    );
    items.push(MenuBarItem::separator());

    let active_sessions: Vec<&relay_api::MenuSession> = menu
        .sessions
        .iter()
        .filter(|session| !session.active_workers.is_empty())
        .collect();
    if !active_sessions.is_empty() {
        let mut submenu = Vec::new();
        let mut shown = 0usize;
        let mut hidden = 0usize;
        for session in &active_sessions {
            for worker in &session.active_workers {
                if shown >= max_workers {
                    hidden += 1;
                    continue;
                }
                shown += 1;
                submenu.push(
                    MenuBarItem::normal(format!("{} — {}", session.display_name, worker.label))
                        .submenu(vec![
                            MenuBarItem::normal(t.t(i18n::key::MENU_SHOW_IN_INSPECTOR)).action(
                                MenuBarAction::OpenInspector {
                                    host_session_id: Some(session.id.clone()),
                                    run_id: Some(worker.run_id.clone()),
                                },
                            ),
                            MenuBarItem::normal(t.t(i18n::key::MENU_CANCEL_WORKER)).action(
                                MenuBarAction::CancelWorker {
                                    worker_session_id: worker.worker_session_id.clone(),
                                },
                            ),
                        ]),
                );
            }
            if session.active_workers.len() > 1 {
                submenu.push(
                    MenuBarItem::normal(format!(
                        "{} · {}",
                        t.t(i18n::key::MENU_STOP_ALL),
                        session.display_name
                    ))
                    .action(MenuBarAction::CancelSession {
                        host_session_id: session.id.clone(),
                    }),
                );
            }
        }
        if hidden > 0 {
            submenu.push(MenuBarItem::header(format!("+{hidden}")));
        }
        items.push(
            MenuBarItem::normal(with_count(&t.t(i18n::key::MENU_ACTIVE), shown + hidden))
                .submenu(submenu),
        );
        items.push(MenuBarItem::separator());
    }

    let sessions: Vec<&relay_api::MenuSession> = menu.sessions.iter().take(max_sessions).collect();
    items.push(MenuBarItem::normal(t.t(i18n::key::MENU_SESSIONS)).submenu(
        if sessions.is_empty() {
            vec![MenuBarItem::header(t.t(i18n::key::MENU_NO_SESSIONS))]
        } else {
            sessions
                .into_iter()
                .map(|session| {
                    MenuBarItem::normal(session.display_name.clone()).submenu(vec![
                        MenuBarItem::normal(t.t(i18n::key::MENU_SHOW_IN_INSPECTOR)).action(
                            MenuBarAction::OpenInspector {
                                host_session_id: Some(session.id.clone()),
                                run_id: None,
                            },
                        ),
                        MenuBarItem::normal(t.t(i18n::key::MENU_STOP_ALL))
                            .enabled(!session.active_workers.is_empty())
                            .action(MenuBarAction::CancelSession {
                                host_session_id: session.id.clone(),
                            }),
                        MenuBarItem::separator(),
                        MenuBarItem::normal(t.t(i18n::key::MENU_OPEN_WORKSPACE)).action(
                            MenuBarAction::OpenWorkspace {
                                cwd: session.cwd.clone(),
                            },
                        ),
                        MenuBarItem::normal(t.t(i18n::key::MENU_COPY_SESSION_ID)).action(
                            MenuBarAction::CopySessionId {
                                host_session_id: session.id.clone(),
                            },
                        ),
                    ])
                })
                .collect()
        },
    ));

    let mut agents = vec![
        MenuBarItem::normal(t.t(i18n::key::PANEL_ADD_AGENT)).action(MenuBarAction::OpenPanel {
            tab: PanelTab::Agents,
            intent: Some(PanelIntent::NewAgent),
            profile_id: None,
        }),
        MenuBarItem::separator(),
    ];
    if menu.agents.is_empty() {
        agents.push(MenuBarItem::header(t.t(i18n::key::MENU_NO_AGENTS)));
    } else {
        agents.extend(menu.agents.iter().map(|agent| {
            let reason = match agent.blocked {
                Some(MenuBlocked::Auth) => Some(t.t(i18n::key::AGENTS_AUTH_REQUIRED)),
                Some(MenuBlocked::Missing) => Some(t.t(i18n::key::AGENTS_NOT_INSTALLED)),
                Some(MenuBlocked::Disabled) => Some(t.t(i18n::key::MENU_AGENT_DISABLED)),
                None => None,
            };
            // A name is a shortcut into the editor; the switches live in the
            // panel because a menu cannot carry a form.
            MenuBarItem::normal(match reason {
                Some(reason) => format!("{} · {reason}", agent.name),
                None => agent.name.clone(),
            })
            .action(MenuBarAction::OpenPanel {
                tab: PanelTab::Agents,
                intent: Some(PanelIntent::EditAgent),
                profile_id: Some(agent.id.clone()),
            })
        }));
    }
    items.push(MenuBarItem::normal(t.t(i18n::key::MENU_AGENTS)).submenu(agents));

    let mut runtimes: Vec<MenuBarItem> = if menu.runtimes.is_empty() {
        vec![MenuBarItem::header(t.t(i18n::key::PANEL_NO_RUNTIMES))]
    } else {
        menu.runtimes
            .iter()
            .map(|runtime| {
                let version = match runtime.version.as_deref() {
                    Some(version) if !version.is_empty() => format!(" · {version}"),
                    _ => String::new(),
                };
                MenuBarItem::header(format!(
                    "{} · {}{version}",
                    runtime.adapter_id,
                    wire_string(&runtime.health)
                ))
            })
            .collect()
    };
    runtimes.push(MenuBarItem::separator());
    runtimes.push(
        MenuBarItem::normal(t.t(i18n::key::PANEL_ADD_RUNTIME_MENU)).action(
            MenuBarAction::OpenPanel {
                tab: PanelTab::Runtime,
                intent: Some(PanelIntent::AddRuntime),
                profile_id: None,
            },
        ),
    );
    runtimes.push(MenuBarItem::normal(t.t(i18n::key::PANEL_RESCAN)).action(MenuBarAction::Rescan));
    runtimes.push(MenuBarItem::normal(t.t(i18n::key::MENU_OPEN_PANEL)).action(
        MenuBarAction::OpenPanel {
            tab: PanelTab::Runtime,
            intent: None,
            profile_id: None,
        },
    ));
    items.push(MenuBarItem::normal(t.t(i18n::key::PANEL_RUNTIME)).submenu(runtimes));

    let mut codex = vec![MenuBarItem::header(if menu.codex.configured {
        t.t(i18n::key::MENU_CODEX_CONNECTED)
    } else {
        t.t(i18n::key::MENU_CODEX_MISSING)
    })];
    codex.extend(menu.codex.checks.iter().map(|check| {
        // The check id is the stable kebab name; `label_key()` is the same map the
        // the menu, kept by hand.
        let label = t.t(check.id.label_key());
        let status = if check.ok {
            String::new()
        } else {
            format!(" — {}", wire_string(&check.status))
        };
        MenuBarItem::header(format!(
            "{} {label}{status}",
            if check.ok { '✓' } else { '✗' }
        ))
    }));
    let broken_checks = menu.codex.checks.iter().filter(|check| !check.ok).count();
    codex.push(MenuBarItem::separator());
    codex.push(
        MenuBarItem::normal(t.t(i18n::key::MENU_CODEX_SETTINGS)).action(MenuBarAction::OpenPanel {
            tab: PanelTab::Codex,
            intent: Some(PanelIntent::CodexActions),
            profile_id: None,
        }),
    );
    codex.push(
        MenuBarItem::normal(if broken_checks > 0 {
            t.t(i18n::key::MENU_REPAIR_CODEX)
        } else {
            t.t(i18n::key::MENU_INSTALL_CODEX)
        })
        .action(MenuBarAction::RepairCodex),
    );
    items.push(MenuBarItem::normal(t.t(i18n::key::MENU_CODEX)).submenu(codex));

    items.push(MenuBarItem::separator());
    if let Some(error) = view.error.as_deref() {
        items.push(MenuBarItem::header(format!(
            "⚠︎ {}",
            truncate_chars(error, ERROR_HEADER_CHARS)
        )));
    }
    if view.daemon == DaemonStatus::Running {
        if let Some(mismatch) = view.daemon_version_mismatch.as_ref() {
            items.push(MenuBarItem::header(t.tv(
                i18n::key::MENU_DAEMON_STALE,
                &[("running", &mismatch.running), ("app", &mismatch.app)],
            )));
        }
    }
    items.push(MenuBarItem::normal(t.t(i18n::key::MENU_REFRESH)).action(MenuBarAction::Refresh));
    items.push(
        MenuBarItem::normal(t.t(i18n::key::MENU_DIAGNOSTICS))
            .action(MenuBarAction::CopyDiagnostics),
    );
    if matches!(
        view.platform,
        MenuBarPlatform::Darwin | MenuBarPlatform::Win32
    ) {
        items.push(
            MenuBarItem::checkbox(t.t(i18n::key::MENU_LAUNCH_AT_LOGIN), view.launch_at_login)
                .action(MenuBarAction::ToggleLaunchAtLogin),
        );
    }
    items.push(MenuBarItem::separator());
    // App updates remain actionable while relayd is healthy; they are not a
    // daemon-recovery-only feature.
    items.extend(update_items(t, view));
    items.push(MenuBarItem::separator());
    items.push(
        MenuBarItem::normal(t.t(i18n::key::MENU_QUIT))
            .accelerator("CmdOrCtrl+Q")
            .action(MenuBarAction::Quit),
    );
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::{UpdateState, UpdateStatus};
    use serde_json::json;

    /// A `MenuView` built from the wire shape, so the fixtures also pin the
    /// contract the daemon serves.
    fn menu_view(value: serde_json::Value) -> MenuView {
        serde_json::from_value(value).expect("menu fixture matches the wire contract")
    }

    fn runtime(id: &str, adapter: &str, health: &str, version: Option<&str>) -> serde_json::Value {
        json!({
            "id": id,
            "adapterId": adapter,
            "executablePath": format!("/usr/local/bin/{adapter}"),
            "version": version,
            "health": health,
            "capabilities": {
                "nonInteractive": true,
                "structuredEvents": false,
                "cwd": true,
                "resume": false,
                "send": false,
                "cancel": false,
                "childSessions": false,
            },
        })
    }

    fn agent(id: &str, name: &str, blocked: Option<&str>) -> serde_json::Value {
        json!({ "id": id, "name": name, "blocked": blocked })
    }

    fn worker(session: &str, run: &str, label: &str) -> serde_json::Value {
        json!({ "workerSessionId": format!("{session}:{run}"), "runId": run, "label": label })
    }

    fn session(id: &str, name: &str, workers: Vec<serde_json::Value>) -> serde_json::Value {
        json!({ "id": id, "displayName": name, "cwd": format!("/tmp/{id}"), "activeWorkers": workers })
    }

    fn empty_menu(status: &str) -> MenuView {
        menu_view(json!({
            "status": status,
            "runningWorkers": 0,
            "awaitingHost": 0,
            "sessions": [],
            "agents": [],
            "runtimes": [],
            "codex": { "checks": [], "configured": false },
        }))
    }

    fn view(daemon: DaemonStatus, menu: Option<MenuView>) -> MenuBarView {
        MenuBarView {
            locale: "en".into(),
            platform: MenuBarPlatform::Darwin,
            daemon,
            menu,
            error: None,
            daemon_version_mismatch: None,
            update: None,
            launch_at_login: false,
            browser: "Google Chrome".into(),
            max_sessions: None,
            max_workers: None,
        }
    }

    fn labels(items: &[MenuBarItem]) -> Vec<String> {
        items.iter().map(|item| item.label.clone()).collect()
    }

    fn find<'a>(items: &'a [MenuBarItem], label: &str) -> &'a MenuBarItem {
        items
            .iter()
            .find(|item| item.label == label)
            .unwrap_or_else(|| panic!("no menu item labelled {label:?} in {:?}", labels(items)))
    }

    #[test]
    fn status_label_precedence_is_starting_down_workers_awaiting_then_setup() {
        // starting wins over everything, even a healthy snapshot.
        let mut starting = view(DaemonStatus::Starting, Some(empty_menu("ready")));
        assert_eq!(
            menu_bar_status_label(&starting),
            "Relay · Starting the log service…"
        );

        // no daemon (or no snapshot) is next.
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Stopped, None)),
            "Relay · Log service is not running"
        );
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Stopped, Some(empty_menu("ready")))),
            "Relay · Log service is not running"
        );
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Running, None)),
            "Relay · Log service is not running"
        );

        // running workers, then awaiting host, both before the snapshot status.
        let busy = menu_view(json!({
            "status": "noRuntime",
            "runningWorkers": 2,
            "awaitingHost": 3,
            "sessions": [], "agents": [], "runtimes": [],
            "codex": { "checks": [], "configured": false },
        }));
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Running, Some(busy))),
            "Relay · 2 running"
        );

        let awaiting = menu_view(json!({
            "status": "needsSetup",
            "runningWorkers": 0,
            "awaitingHost": 3,
            "sessions": [], "agents": [], "runtimes": [],
            "codex": { "checks": [], "configured": false },
        }));
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Running, Some(awaiting))),
            "Relay · 3 awaiting Codex"
        );

        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Running, Some(empty_menu("noRuntime")))),
            "Relay · No runtime detected"
        );
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Running, Some(empty_menu("needsSetup")))),
            "Relay · Setup needed"
        );
        assert_eq!(
            menu_bar_status_label(&view(DaemonStatus::Running, Some(empty_menu("ready")))),
            "Relay · Ready"
        );

        // The Chinese resources are used when the locale says so.
        starting.locale = "zh-CN".into();
        assert_eq!(
            menu_bar_status_label(&starting),
            "Relay · 正在启动日志服务…"
        );
    }

    #[test]
    fn the_daemon_down_variant_offers_recovery_and_updates_only() {
        let items = build_menu_bar_items(&view(DaemonStatus::Stopped, None));
        assert_eq!(
            labels(&items),
            vec![
                "Relay · Log service is not running",
                "Start log service",
                "Restart log service",
                "",
                "Copy diagnostics",
                "",
                concat!("Version ", env!("CARGO_PKG_VERSION")),
                "Check for updates…",
                "",
                "Quit Relay",
            ]
        );
        assert_eq!(
            find(&items, "Start log service").action,
            Some(MenuBarAction::StartDaemon)
        );
        assert_eq!(
            find(&items, "Quit Relay").accelerator.as_deref(),
            Some("CmdOrCtrl+Q")
        );

        // While starting, both daemon actions are visible but disabled.
        let starting = build_menu_bar_items(&view(DaemonStatus::Starting, None));
        assert_eq!(find(&starting, "Starting…").enabled, Some(false));
        assert_eq!(find(&starting, "Restart log service").enabled, Some(false));
    }

    #[test]
    fn the_daemon_running_variant_carries_every_section() {
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(empty_menu("ready"))));
        assert_eq!(
            labels(&items),
            vec![
                "Relay · Ready",
                "Open inspector",
                "Control panel…",
                "",
                "Sessions",
                "Agents",
                "Runtimes",
                "Codex integration",
                "",
                "Refresh",
                "Copy diagnostics",
                "Launch at login",
                "",
                concat!("Version ", env!("CARGO_PKG_VERSION")),
                "Check for updates…",
                "",
                "Quit Relay",
            ]
        );
        assert_eq!(
            find(&items, "Open inspector").action,
            Some(MenuBarAction::OpenInspector {
                host_session_id: None,
                run_id: None
            })
        );
        assert_eq!(
            find(&items, "Open inspector").accelerator.as_deref(),
            Some("CmdOrCtrl+O")
        );
        assert_eq!(
            find(&items, "Control panel…").accelerator.as_deref(),
            Some("CmdOrCtrl+,")
        );
        assert!(matches!(
            find(&items, "Control panel…").action,
            Some(MenuBarAction::OpenPanel {
                tab: PanelTab::Agents,
                intent: None,
                profile_id: None
            })
        ));
        // Launch at login is a checkbox only on the platforms that have it.
        assert_eq!(find(&items, "Launch at login").kind, MenuItemKind::Checkbox);
        assert_eq!(find(&items, "Launch at login").checked, Some(false));

        let mut linux = view(DaemonStatus::Running, Some(empty_menu("ready")));
        linux.platform = MenuBarPlatform::Linux;
        assert!(!labels(&build_menu_bar_items(&linux)).contains(&"Launch at login".to_string()));

        // Empty snapshots render their empty-state headers rather than nothing.
        assert_eq!(
            find(&items, "Sessions").submenu.as_ref().unwrap()[0].label,
            "No sessions yet"
        );
        assert_eq!(
            find(&items, "Agents").submenu.as_ref().unwrap()[2].label,
            "No agents configured"
        );
        assert_eq!(
            find(&items, "Runtimes").submenu.as_ref().unwrap()[0].label,
            "No runtime detected"
        );
    }

    #[test]
    fn sessions_and_workers_are_capped_with_an_overflow_header() {
        let sessions: Vec<serde_json::Value> = (0..10)
            .map(|index| {
                session(
                    &format!("s{index}"),
                    &format!("Session {index}"),
                    vec![worker(&format!("s{index}"), "run", "codex")],
                )
            })
            .collect();
        let menu = menu_view(json!({
            "status": "ready",
            "runningWorkers": 10,
            "awaitingHost": 0,
            "sessions": sessions,
            "agents": [], "runtimes": [],
            "codex": { "checks": [], "configured": false },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(menu)));

        // Eight sessions are listed, two are dropped without a header of their own.
        let session_items = find(&items, "Sessions").submenu.as_ref().unwrap();
        assert_eq!(session_items.len(), DEFAULT_MAX_MENU_SESSIONS);
        assert_eq!(session_items[0].label, "Session 0");
        assert_eq!(session_items[7].label, "Session 7");

        // Thirteen workers: twelve are listed, the header counts the whole set
        // and the overflow is summarised as `+N`.
        let crowded: Vec<serde_json::Value> = (0..7)
            .map(|index| {
                let workers = if index < 6 {
                    vec![
                        worker(&format!("s{index}"), "run-1", "codex"),
                        worker(&format!("s{index}"), "run-2", "claude"),
                    ]
                } else {
                    vec![worker(&format!("s{index}"), "run-1", "codex")]
                };
                session(&format!("s{index}"), &format!("Session {index}"), workers)
            })
            .collect();
        let menu = menu_view(json!({
            "status": "ready",
            "runningWorkers": 13,
            "awaitingHost": 0,
            "sessions": crowded,
            "agents": [], "runtimes": [],
            "codex": { "checks": [], "configured": false },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(menu)));
        let active = find(&items, "Active delegations (13)")
            .submenu
            .as_ref()
            .unwrap();
        // Six two-worker sessions contribute a "stop all" entry each, so the
        // twelve capped workers plus six stop-alls plus the overflow header.
        assert_eq!(active.len(), DEFAULT_MAX_MENU_WORKERS + 6 + 1);
        assert_eq!(active[DEFAULT_MAX_MENU_WORKERS + 6].label, "+1");
        // The twelfth shown worker is the last one before the cap; the last
        // session's worker is the hidden one, and its "stop all" is not emitted
        // because that session only has one worker.
        assert_eq!(active[16].label, "Session 5 — claude");
        assert_eq!(active[17].label, "Stop all workers · Session 5");
        assert!(!labels(active)
            .iter()
            .any(|label| label.contains("Session 6 —")));

        // A session with several workers also offers "stop all".
        let two = menu_view(json!({
            "status": "ready",
            "runningWorkers": 2,
            "awaitingHost": 0,
            "sessions": [session("s1", "Session 1", vec![worker("s1", "run-1", "codex"), worker("s1", "run-2", "claude")])],
            "agents": [], "runtimes": [],
            "codex": { "checks": [], "configured": false },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(two)));
        let active = find(&items, "Active delegations (2)")
            .submenu
            .as_ref()
            .unwrap();
        assert_eq!(active.len(), 3);
        assert_eq!(active[0].label, "Session 1 — codex");
        assert_eq!(active[2].label, "Stop all workers · Session 1");
        assert_eq!(
            active[2].action,
            Some(MenuBarAction::CancelSession {
                host_session_id: "s1".into()
            })
        );
        assert_eq!(
            active[0].submenu.as_ref().unwrap()[1].action,
            Some(MenuBarAction::CancelWorker {
                worker_session_id: "s1:run-1".into()
            })
        );
    }

    #[test]
    fn blocked_agents_say_why_and_open_the_editor() {
        let menu = menu_view(json!({
            "status": "needsSetup",
            "runningWorkers": 0,
            "awaitingHost": 0,
            "sessions": [],
            "agents": [
                agent("a1", "Codex", None),
                agent("a2", "Claude", Some("auth")),
                agent("a3", "Gemini", Some("missing")),
                agent("a4", "Local", Some("disabled")),
            ],
            "runtimes": [], "codex": { "checks": [], "configured": false },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(menu)));
        let agents = find(&items, "Agents").submenu.as_ref().unwrap();
        assert_eq!(agents[0].label, "New agent…");
        assert_eq!(agents[1].kind, MenuItemKind::Separator);
        assert_eq!(agents[2].label, "Codex");
        assert_eq!(agents[3].label, "Claude · Authentication required");
        assert_eq!(agents[4].label, "Gemini · Not installed");
        assert_eq!(agents[5].label, "Local · disabled");
        assert_eq!(
            agents[3].action,
            Some(MenuBarAction::OpenPanel {
                tab: PanelTab::Agents,
                intent: Some(PanelIntent::EditAgent),
                profile_id: Some("a2".into()),
            })
        );

        // The Chinese resources localize the same reasons.
        let mut zh = view(
            DaemonStatus::Running,
            Some(menu_view(json!({
                "status": "needsSetup",
                "runningWorkers": 0,
                "awaitingHost": 0,
                "sessions": [],
                "agents": [agent("a2", "Claude", Some("auth"))],
                "runtimes": [], "codex": { "checks": [], "configured": false },
            }))),
        );
        zh.locale = "zh-CN".into();
        let items = build_menu_bar_items(&zh);
        assert_eq!(
            find(&items, "智能体").submenu.as_ref().unwrap()[2].label,
            "Claude · 需要认证"
        );
    }

    #[test]
    fn runtimes_render_adapter_health_and_version() {
        let menu = menu_view(json!({
            "status": "ready",
            "runningWorkers": 0,
            "awaitingHost": 0,
            "sessions": [], "agents": [],
            "runtimes": [
                runtime("r1", "codex", "available", Some("1.2.3")),
                runtime("r2", "claude", "authentication_required", None),
                runtime("r3", "gemini", "unavailable", Some("")),
            ],
            "codex": { "checks": [], "configured": false },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(menu)));
        let runtimes = find(&items, "Runtimes").submenu.as_ref().unwrap();
        assert_eq!(runtimes[0].label, "codex · available · 1.2.3");
        assert_eq!(runtimes[0].kind, MenuItemKind::Header);
        assert_eq!(runtimes[1].label, "claude · authentication_required");
        assert_eq!(runtimes[2].label, "gemini · unavailable");
        assert_eq!(runtimes[3].kind, MenuItemKind::Separator);
        assert_eq!(runtimes[4].label, "Add runtime…");
        assert_eq!(runtimes[5].label, "Rescan runtimes");
        assert_eq!(runtimes[5].action, Some(MenuBarAction::Rescan));
        assert!(matches!(
            runtimes[6].action,
            Some(MenuBarAction::OpenPanel {
                tab: PanelTab::Runtime,
                intent: None,
                profile_id: None
            })
        ));
    }

    #[test]
    fn codex_checks_render_tick_cross_and_the_repair_label() {
        let menu = menu_view(json!({
            "status": "needsSetup",
            "runningWorkers": 0,
            "awaitingHost": 0,
            "sessions": [], "agents": [], "runtimes": [],
            "codex": {
                "configured": false,
                "checks": [
                    { "id": "codex-cli", "ok": true, "status": "ok", "detail": "1.2.3" },
                    { "id": "relay-mcp", "ok": false, "status": "missing", "detail": "not configured" },
                    { "id": "relay-hooks", "ok": false, "status": "stale", "detail": "old" },
                ],
            },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(menu)));
        let codex = find(&items, "Codex integration").submenu.as_ref().unwrap();
        assert_eq!(codex[0].label, "Not configured");
        assert_eq!(codex[1].label, "✓ Codex CLI detection");
        assert_eq!(codex[2].label, "✗ Relay MCP configuration — missing");
        assert_eq!(codex[3].label, "✗ Relay hooks registration — stale");
        assert_eq!(codex[4].kind, MenuItemKind::Separator);
        assert_eq!(codex[5].label, "Codex integration settings…");
        // Any broken check turns the last entry into "repair".
        assert_eq!(codex[6].label, "Repair integration");
        assert_eq!(codex[6].action, Some(MenuBarAction::RepairCodex));

        let healthy = menu_view(json!({
            "status": "ready",
            "runningWorkers": 0, "awaitingHost": 0,
            "sessions": [], "agents": [], "runtimes": [],
            "codex": {
                "configured": true,
                "checks": [{ "id": "codex-cli", "ok": true, "status": "ok", "detail": "1.2.3" }],
            },
        }));
        let items = build_menu_bar_items(&view(DaemonStatus::Running, Some(healthy)));
        let codex = find(&items, "Codex integration").submenu.as_ref().unwrap();
        assert_eq!(codex[0].label, "Connected");
        assert_eq!(codex[4].label, "Install in Codex…");
    }

    #[test]
    fn error_and_version_mismatch_headers_are_truncated_and_explicit() {
        let mut down = view(DaemonStatus::Stopped, None);
        down.error = Some("health request failed".into());
        let items = build_menu_bar_items(&down);
        assert!(labels(&items).contains(&"⚠︎ health request failed".to_string()));

        let mut broken = view(DaemonStatus::Running, Some(empty_menu("ready")));
        broken.error = Some("x".repeat(200));
        let items = build_menu_bar_items(&broken);
        let header = items
            .iter()
            .find(|item| item.label.starts_with("⚠︎ "))
            .expect("error header");
        assert_eq!(header.label.chars().count(), "⚠︎ ".chars().count() + 80);
        assert_eq!(header.kind, MenuItemKind::Header);

        // Multi-byte errors are cut on a character boundary, not a byte one.
        let mut unicode = view(DaemonStatus::Running, Some(empty_menu("ready")));
        unicode.error = Some("错".repeat(100));
        let items = build_menu_bar_items(&unicode);
        let header = items
            .iter()
            .find(|item| item.label.starts_with("⚠︎ "))
            .unwrap();
        assert_eq!(header.label.chars().count(), "⚠︎ ".chars().count() + 80);

        let mut stale = view(DaemonStatus::Running, Some(empty_menu("ready")));
        stale.daemon_version_mismatch = Some(VersionMismatch {
            running: "0.0.9".into(),
            app: "0.1.0".into(),
        });
        let items = build_menu_bar_items(&stale);
        assert!(labels(&items)
            .contains(&"Log service is version 0.0.9 (app 0.1.0) — restart it".to_string()));

        // A stopped daemon never shows the mismatch line.
        let mut stopped = view(DaemonStatus::Stopped, None);
        stopped.daemon_version_mismatch = Some(VersionMismatch {
            running: "0.0.9".into(),
            app: "0.1.0".into(),
        });
        assert!(!labels(&build_menu_bar_items(&stopped))
            .iter()
            .any(|label| label.contains("0.1.0")));
    }

    #[test]
    fn the_update_block_matches_the_documented_menu() {
        let base = view(DaemonStatus::Running, Some(empty_menu("ready")));

        // idle renders nothing but the check item.
        let mut idle = base.clone();
        idle.update = Some(UpdateState {
            status: UpdateStatus::Idle,
            ..UpdateState::default()
        });
        let items = build_menu_bar_items(&idle);
        assert!(labels(&items).contains(&"Check for updates…".to_string()));
        assert_eq!(
            labels(&items)
                .iter()
                .filter(|label| label.as_str() != "Check for updates…")
                .filter(|label| label.to_lowercase().contains("update"))
                .count(),
            0
        );

        let mut available = base.clone();
        available.update = Some(UpdateState {
            status: UpdateStatus::Available,
            version: Some("0.3.0".into()),
            ..UpdateState::default()
        });
        let items = build_menu_bar_items(&available);
        assert!(labels(&items).contains(&"Update 0.3.0 available".to_string()));
        assert_eq!(find(&items, "Update 0.3.0 available").enabled, Some(false));

        let mut checking = base.clone();
        checking.update = Some(UpdateState {
            status: UpdateStatus::Checking,
            ..UpdateState::default()
        });
        let items = build_menu_bar_items(&checking);
        assert!(labels(&items).contains(&"Checking for updates…".to_string()));
        // "Check for updates…" is disabled while a check is in flight.
        assert_eq!(find(&items, "Check for updates…").enabled, Some(false));

        let mut none = base.clone();
        none.update = Some(UpdateState {
            status: UpdateStatus::None,
            ..UpdateState::default()
        });
        let items = build_menu_bar_items(&none);
        assert!(labels(&items).contains(&"Relay is up to date".to_string()));
        assert_eq!(find(&items, "Check for updates…").enabled, Some(true));

        let mut error = base;
        error.update = Some(UpdateState {
            status: UpdateStatus::Error,
            message: Some("network unreachable".into()),
            ..UpdateState::default()
        });
        let items = build_menu_bar_items(&error);
        assert!(labels(&items).contains(&"⚠︎ network unreachable".to_string()));
    }

    #[test]
    fn the_serialized_model_only_changes_when_something_visible_changes() {
        let base = view(DaemonStatus::Running, Some(empty_menu("ready")));
        let mut same = base.clone();
        same.browser = "Google Chrome".into();
        assert_eq!(base.serialized(), same.serialized());

        let mut changed = base.clone();
        changed.update = Some(UpdateState {
            status: UpdateStatus::None,
            ..UpdateState::default()
        });
        assert_ne!(base.serialized(), changed.serialized());

        let mut error = base.clone();
        error.error = Some("relayd failed to start".into());
        assert_ne!(base.serialized(), error.serialized());
    }

    #[test]
    fn actions_round_trip_through_the_menu_id() {
        // The tray encodes actions into native menu item ids and parses them back.
        for action in [
            MenuBarAction::OpenInspector {
                host_session_id: Some("codex:s1".into()),
                run_id: Some("r1".into()),
            },
            MenuBarAction::OpenPanel {
                tab: PanelTab::Codex,
                intent: Some(PanelIntent::CodexActions),
                profile_id: None,
            },
            MenuBarAction::CancelWorker {
                worker_session_id: "w1".into(),
            },
            MenuBarAction::Quit,
        ] {
            let encoded = serde_json::to_string(&action).unwrap();
            assert_eq!(
                serde_json::from_str::<MenuBarAction>(&encoded).unwrap(),
                action
            );
        }
        assert_eq!(
            serde_json::to_value(MenuBarAction::RestartDaemon).unwrap(),
            json!({ "type": "restart-daemon" })
        );
        assert_eq!(
            serde_json::to_value(MenuBarAction::OpenPanel {
                tab: PanelTab::Agents,
                intent: Some(PanelIntent::NewAgent),
                profile_id: None
            })
            .unwrap(),
            json!({ "type": "open-panel", "tab": "agents", "intent": "new-agent" })
        );
    }
}
